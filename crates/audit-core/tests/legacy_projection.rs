//! Legacy claim-row projection: every main producer shape is accepted with the
//! expected envelope, and every unsafe or inconsistent row is quarantined.

mod common;

use audit_core::catalog::{AUDIT_STORE_SOURCE, DOCUMENT_SOURCE};
use audit_core::envelope::{DATASCHEMA, LEGACY_SOURCE_FORMAT};
use audit_core::{
    Catalog, DocumentStagingProjection, JSONB_TEXT_LIMIT, Rejection, RejectionCode as C,
    jsonb_text_len, project,
};
use common::*;
use serde_json::{Map, Value, json};

#[test]
fn every_producer_variant_projects_to_the_expected_envelope() {
    for fixture in accepted_fixtures() {
        let name = fixture.name;
        let row = &fixture.row;
        let envelope = project(row).unwrap_or_else(|r| panic!("{name}: rejected with {r}"));
        let spec = Catalog::embedded()
            .get(&row.event_type)
            .expect("catalog entry");
        let value = envelope.as_value();
        assert_eq!(envelope.id().to_string(), row.event_id, "{name}");
        assert_eq!(value["specversion"], "1.0", "{name}");
        assert_eq!(value["type"], row.event_type.as_str(), "{name}");
        assert_eq!(value["source"], DOCUMENT_SOURCE, "{name}");
        assert_eq!(value["subject"], row.subject.as_str(), "{name}");
        assert_eq!(value["time"], OCCURRED, "{name}");
        assert_eq!(value["datacontenttype"], "application/json", "{name}");
        assert_eq!(value["dataschema"], DATASCHEMA, "{name}");

        let data = &value["data"];
        assert_eq!(data["schema_version"], 1, "{name}");
        assert_eq!(data["action"], row.event_type.as_str(), "{name}");
        assert_eq!(data["event_class"], spec.event_class.as_str(), "{name}");
        assert_eq!(
            data["actor"],
            json!({"issuer": row.actor_identity_provider, "principal_id": row.actor_principal_id}),
            "{name}"
        );
        let mut resource = json!({"type": row.resource_type, "id": row.resource_id});
        if let Some(version) = &row.resource_version_id {
            resource["version_id"] = json!(version);
        }
        assert_eq!(data["resource"], resource, "{name}");
        assert_eq!(data["result"], row.result.as_str(), "{name}");
        assert_eq!(data["extensions"], json!({}), "{name}");
        assert_eq!(
            data["provenance"],
            json!({
                "source_format": LEGACY_SOURCE_FORMAT,
                "adapter_version": 1,
                "source_commitment": commitment(),
                "registration": "trigger"
            }),
            "{name}"
        );
        assert_eq!(data["correlation"], fixture.expect.correlation, "{name}");

        let mut details: Map<String, Value> = row
            .data
            .as_ref()
            .and_then(Value::as_object)
            .cloned()
            .expect("object data");
        for removed in fixture.expect.removed {
            assert!(details.remove(*removed).is_some(), "{name}: {removed}");
        }
        assert_eq!(data["details"], Value::Object(details), "{name}");

        match fixture.expect.reason_bytes {
            Some(bytes) => assert_eq!(
                data["reason"],
                json!({"provided": true, "utf8_bytes": bytes, "text_retained": "source_systems"}),
                "{name}"
            ),
            None => assert!(data.get("reason").is_none(), "{name}"),
        }
        assert_eq!(
            data.get("service_executor"),
            fixture.expect.service_executor.as_ref(),
            "{name}"
        );
        assert_eq!(
            data.get("reason_code").and_then(Value::as_str),
            fixture.expect.reason_code,
            "{name}"
        );
        let text = envelope.to_json_string();
        assert!(
            jsonb_text_len(envelope.as_value()) <= JSONB_TEXT_LIMIT,
            "{name}"
        );
        assert!(
            !text.contains("identityProvider"),
            "{name}: legacy principal kept"
        );
    }
}

#[test]
fn projection_is_deterministic_for_identical_input() {
    for fixture in accepted_fixtures() {
        let first = project(&fixture.row).expect("projects").to_json_string();
        let second = project(&fixture.row).expect("projects").to_json_string();
        assert_eq!(first, second, "{}", fixture.name);
        // Member order of the source data must not matter.
        let mut reordered = fixture.row.clone();
        if let Some(Value::Object(map)) = &fixture.row.data {
            let reversed: Map<String, Value> = map
                .iter()
                .rev()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            reordered.data = Some(Value::Object(reversed));
        }
        assert_eq!(
            project(&reordered).expect("projects").to_json_string(),
            first,
            "{}",
            fixture.name
        );
        // Keys are emitted in byte order regardless of serde_json features.
        assert!(
            first.starts_with("{\"data\":{\"action\":"),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn claim_payload_json_round_trips_into_the_projection_struct() {
    let fixture = fixture_named("document.version.withdrawn/withheld");
    let row = &fixture.row;
    let wire = json!({
        "event_id": row.event_id, "event_type": row.event_type, "source": row.source,
        "subject": row.subject, "actor_identity_provider": row.actor_identity_provider,
        "actor_principal_id": row.actor_principal_id, "resource_type": row.resource_type,
        "resource_id": row.resource_id, "resource_version_id": row.resource_version_id,
        "result": row.result, "trace_id": null, "occurred_at": row.occurred_at,
        "oversize": false, "data": row.data, "data_kind": "object", "reason_kind": "string",
        "reason_bytes": 42, "source_intact": true, "source_commitment": commitment(),
        "registration_kind": "trigger"
    });
    let parsed: DocumentStagingProjection = serde_json::from_value(wire.clone()).expect("parses");
    assert_eq!(&parsed, row);
    let mut extra = wire;
    extra["reason"] = json!("caller text must never be part of the claim payload");
    assert!(serde_json::from_value::<DocumentStagingProjection>(extra).is_err());
}

#[test]
fn debug_output_of_rows_and_envelopes_omits_payload() {
    let fixture = fixture_named("document.version.withdrawn/withheld");
    let row_debug = format!("{:?}", fixture.row);
    assert!(row_debug.contains(EVENT_ID));
    for leaked in [PRINCIPAL, "base_inspection_unavailable", DOC] {
        assert!(!row_debug.contains(leaked), "row Debug leaked {leaked}");
    }
    let envelope = project(&fixture.row).expect("projects");
    let envelope_debug = format!("{envelope:?}");
    assert!(!envelope_debug.contains(PRINCIPAL));
    assert!(!envelope_debug.contains(DOC));
}

#[test]
fn debug_output_of_malformed_rows_never_echoes_column_text() {
    let mut row = fixture_named("document.created").row;
    row.event_id = "SECRET-NOT-A-UUID".to_owned();
    row.event_type = "secret free text type".to_owned();
    let debug = format!("{row:?}");
    assert!(!debug.contains("SECRET"), "{debug}");
    assert!(!debug.contains("secret"), "{debug}");
    assert!(debug.contains("<invalid len=17>"), "{debug}");
    assert!(debug.contains("<invalid len=21>"), "{debug}");
    // A well-formed id and a catalog type are shown.
    let fine = format!("{:?}", fixture_named("document.created").row);
    assert!(
        fine.contains(EVENT_ID) && fine.contains("\"document.created\""),
        "{fine}"
    );
    let mut uppercase = fixture_named("document.created").row;
    uppercase.event_id = EVENT_ID.to_uppercase();
    assert!(format!("{uppercase:?}").contains("<invalid len=36>"));
}

#[test]
fn legacy_time_matches_the_workspace_time_serde_shape() {
    use time::{Date, Month, OffsetDateTime, Time, UtcOffset};
    let date = Date::from_calendar_date(2026, Month::October, 7).expect("date");
    let utc = OffsetDateTime::new_utc(
        date,
        Time::from_hms_nano(1, 2, 3, 456_789_000).expect("time"),
    );
    let utc_value = serde_json::to_value(utc).expect("serialize");
    assert_eq!(
        utc_value,
        legacy_time(),
        "workspace time serde must stay the 9-integer tuple"
    );
    let tokyo = utc.to_offset(UtcOffset::from_hms(9, 0, 0).expect("offset"));
    let tokyo_value = serde_json::to_value(tokyo).expect("serialize");
    assert_eq!(
        tokyo_value,
        json!([2026, 280, 10, 2, 3, 456_789_000, 9, 0, 0])
    );
    let west = utc.to_offset(UtcOffset::from_hms(-3, -30, 0).expect("offset"));
    let west_value = serde_json::to_value(west).expect("serialize");
    assert!(
        west_value
            .as_array()
            .is_some_and(|a| a.len() == 9 && a.iter().all(Value::is_i64))
    );

    for value in [utc_value, tokyo_value, west_value] {
        let mut fixture = fixture_named("document.version.published/manual");
        fixture.row.data.as_mut().expect("data")["publishedAt"] = value;
        project(&fixture.row).expect("validator accepts real time serde output");
    }
}

fn reject(row: &DocumentStagingProjection) -> Rejection {
    project(row).expect_err("must be rejected")
}

fn at(code: C, field: &'static str) -> Rejection {
    Rejection::at(code, field)
}

fn with_data(name: &str, edit: impl FnOnce(&mut Map<String, Value>)) -> DocumentStagingProjection {
    let mut row = fixture_named(name).row;
    edit(
        row.data
            .as_mut()
            .and_then(Value::as_object_mut)
            .expect("object"),
    );
    row
}

#[test]
fn free_text_and_unknown_members_are_quarantined() {
    for key in [
        "note", "body", "query", "token", "comment", "title", "filename",
    ] {
        for name in [
            "document.created",
            "document.version.withdrawn/withheld",
            "access_policy.changed/document",
        ] {
            let row = with_data(name, |data| {
                data.insert(key.to_owned(), json!("synthetic free text"));
            });
            let rejection = reject(&row);
            assert_eq!(rejection, at(C::UnknownField, "data"), "{name}/{key}");
            assert!(!rejection.to_string().contains("synthetic"));
        }
    }
}

#[test]
fn reason_text_never_passes_through() {
    let row = with_data("document.version.withdrawn/withheld", |data| {
        data.insert("reason".to_owned(), json!("SYNTHETIC-REASON-TEXT"));
    });
    let rejection = reject(&row);
    assert_eq!(rejection, at(C::UnknownField, "reason"));
    assert!(!format!("{rejection:?} {rejection}").contains("SYNTHETIC"));

    let invalid = at(C::InvalidReason, "reason");
    let mut missing = fixture_named("document.metadata.changed").row;
    missing.reason_kind = None;
    missing.reason_bytes = None;
    assert_eq!(reject(&missing), invalid);

    let mut no_bytes = fixture_named("folder.created").row;
    no_bytes.reason_bytes = None;
    assert_eq!(reject(&no_bytes), invalid);

    let mut negative = fixture_named("folder.created").row;
    negative.reason_bytes = Some(-1);
    assert_eq!(reject(&negative), invalid);

    // The Document HTTP body limit bounds the reported length.
    let mut at_limit = fixture_named("folder.created").row;
    at_limit.reason_bytes = Some(1_048_576);
    let envelope = project(&at_limit).expect("at the body limit");
    assert_eq!(envelope.data()["reason"]["utf8_bytes"], 1_048_576);
    let mut over_limit = fixture_named("folder.created").row;
    over_limit.reason_bytes = Some(1_048_577);
    assert_eq!(reject(&over_limit), invalid);

    for kind in ["number", "object", "array", "null", "boolean"] {
        let mut row = fixture_named("document.publication.ended").row;
        row.reason_kind = Some(kind.to_owned());
        assert_eq!(reject(&row), at(C::ReasonNotString, "reason"), "{kind}");
    }

    let mut on_acl = fixture_named("access_policy.changed/document").row;
    on_acl.reason_kind = Some("string".to_owned());
    on_acl.reason_bytes = Some(3);
    assert_eq!(reject(&on_acl), at(C::UnknownField, "reason"));
}

#[test]
fn duplicated_actor_must_match_the_row_actor() {
    for name in [
        "document.version.withdrawn/withheld",
        "document.publication.ended",
    ] {
        let row = with_data(name, |data| {
            data.insert(
                "actor".to_owned(),
                json!({"identityProvider": "poc", "principalId": "someone-else"}),
            );
        });
        assert_eq!(reject(&row), at(C::ActorMismatch, "actor"), "{name}");
        let row = with_data(name, |data| {
            data.remove("actor");
        });
        assert_eq!(reject(&row), at(C::MissingField, "actor"), "{name}");
        let row = with_data(name, |data| {
            data.insert("actor".to_owned(), json!({"identityProvider": "poc"}));
        });
        assert_eq!(reject(&row), at(C::InvalidActor, "actor"), "{name}");
        let row = with_data(name, |data| {
            data.insert(
                "actor".to_owned(),
                json!({"identityProvider": "poc", "principalId": "poc-human\u{200f}"}),
            );
        });
        assert_eq!(reject(&row), at(C::InvalidActor, "actor"), "{name}");
    }
}

#[test]
fn service_executor_shape_is_enforced() {
    let row = with_data("document.version.published/scheduled", |data| {
        data.insert(
            "serviceExecutor".to_owned(),
            json!({"identityProvider": "service"}),
        );
    });
    let invalid = at(C::InvalidServiceExecutor, "serviceExecutor");
    assert_eq!(reject(&row), invalid);
    let row = with_data("document.version.publication.terminal/scheduler", |data| {
        data.insert(
            "serviceExecutor".to_owned(),
            json!({"identityProvider": "service", "principalId": "sch\u{1b}eduler"}),
        );
    });
    assert_eq!(reject(&row), invalid);
    for hidden in [
        "\u{feff}scheduler",
        "scheduler\u{e0020}",
        "  ",
        "scheduler\u{2029}",
    ] {
        let row = with_data("document.version.published/scheduled", |data| {
            data.insert(
                "serviceExecutor".to_owned(),
                json!({"identityProvider": "service", "principalId": hidden}),
            );
        });
        assert_eq!(reject(&row), invalid, "{hidden:?}");
    }
    let row = with_data("document.created", |data| {
        data.insert("serviceExecutor".to_owned(), scheduler());
    });
    assert_eq!(reject(&row), at(C::UnknownField, "data"));
}

#[test]
fn typed_fields_reject_wrong_types_floats_and_bounds() {
    let cases: Vec<(&str, &str, Value, C)> = vec![
        (
            "document.version.created/later",
            "versionNo",
            json!("2"),
            C::InvalidField,
        ),
        (
            "document.version.created/later",
            "versionNo",
            json!(2.0),
            C::InvalidField,
        ),
        (
            "document.version.created/later",
            "versionNo",
            json!(0),
            C::InvalidField,
        ),
        (
            "document.version.created/later",
            "resultingDocumentRevision",
            json!(-5),
            C::InvalidField,
        ),
        (
            "document.version.created/later",
            "resultingDocumentRevision",
            json!(u64::MAX),
            C::InvalidField,
        ),
        (
            "document.version.updated/null_base",
            "baseDocumentVersionId",
            json!("not-a-uuid"),
            C::InvalidField,
        ),
        (
            "document.version.updated/null_base",
            "documentVersionId",
            Value::Null,
            C::InvalidField,
        ),
        (
            "document.version.published/manual",
            "publishedAt",
            json!("2026-10-07T01:02:03Z"),
            C::InvalidField,
        ),
        (
            "document.version.published/manual",
            "result",
            json!("failure"),
            C::InvalidField,
        ),
        (
            "document.diff.result_access_granted/compare",
            "result_digest",
            Value::Array(vec![json!(0); 31]),
            C::InvalidField,
        ),
        (
            "document.diff.result_access_granted/compare",
            "verdict",
            json!("x".repeat(600)),
            C::InvalidField,
        ),
        (
            "document.diff.result_access_granted/compare",
            "cache_hit",
            json!(1),
            C::InvalidField,
        ),
        (
            "document.metadata.changed",
            "changed_keys",
            json!(["title"]),
            C::InvalidField,
        ),
        (
            "document.metadata.changed",
            "changed_keys",
            json!([]),
            C::InvalidField,
        ),
        (
            "document.version.withdrawn/withheld",
            "restorationWithheldReason",
            json!("free text"),
            C::InvalidField,
        ),
        (
            "document.file.access_granted/download",
            "purpose",
            json!("export"),
            C::InvalidField,
        ),
        (
            "authorization.denied/management",
            "action_code",
            json!("publish"),
            C::InvalidField,
        ),
        (
            "document.version.publication.terminal/no_executor",
            "terminalReason",
            json!("withdrawal invalidated intent"),
            C::InvalidField,
        ),
    ];
    for (name, key, value, code) in cases {
        let row = with_data(name, |data| {
            data.insert(key.to_owned(), value);
        });
        let rejection = reject(&row);
        assert_eq!(rejection.code, code, "{name}/{key}");
        assert_eq!(rejection.field, Some(key), "{name}/{key}");
    }
    let row = with_data("document.version.updated/null_base", |data| {
        data.remove("versionNo");
    });
    assert_eq!(
        reject(&row),
        audit_core::Rejection::at(C::MissingField, "versionNo")
    );
}

/// `document.version.detail_viewed` / `marked_unread` (current_read_state.rs,
/// migration 0012): the producer's shapes project with the operation id as
/// correlation; drift from them is quarantined with the field named.
#[test]
fn read_state_rows_project_the_producer_shape_and_quarantine_drift() {
    const VIEWED: &str = "document.version.detail_viewed/first_record";
    const RECHECK: &str = "document.version.detail_viewed/after_reset";
    const UNREAD: &str = "document.version.marked_unread";
    for name in [VIEWED, RECHECK, UNREAD] {
        let envelope = project(&fixture_named(name).row).expect(name);
        let data = &envelope.as_value()["data"];
        assert_eq!(data["event_class"], "DATA_ACCESS", "{name}");
        assert_eq!(data["correlation"], json!({"operation_id": OP}), "{name}");
        assert_eq!(
            data["resource"],
            json!({"type": "Document", "id": DOC, "version_id": VER}),
            "{name}"
        );
        assert!(data.get("reason").is_none(), "{name}");
    }
    // The largest revisions the producer can stage (r = 2^53 - 1).
    let max = audit_core::kinds::MAX_SAFE_INTEGER;
    for name in [VIEWED, UNREAD] {
        let row = with_data(name, |data| {
            data.insert("expected_read_state_revision".to_owned(), json!(max - 1));
            data.insert("resulting_read_state_revision".to_owned(), json!(max));
        });
        let envelope = project(&row).unwrap_or_else(|r| panic!("{name}: {r}"));
        assert_eq!(
            envelope.as_value()["data"]["details"]["resulting_read_state_revision"],
            json!(max)
        );
    }

    let invalid = |field: &'static str| at(C::InvalidField, field);
    let expected = "expected_read_state_revision";
    let resulting = "resulting_read_state_revision";
    let mut cases: Vec<(&str, DocumentStagingProjection, Rejection)> = Vec::new();
    for name in [VIEWED, UNREAD] {
        // Keys the producer never writes (read state internals, principal
        // data, free text) are not admitted.
        for key in [
            "needs_recheck",
            "first_read_at",
            "read_state_revision",
            "principal_id",
            "document_id",
            "note",
        ] {
            let row = with_data(name, |data| {
                data.insert(key.to_owned(), json!("synthetic"));
            });
            cases.push((key, row, at(C::UnknownField, "data")));
        }
        for (field, value) in [
            (expected, json!("0")),
            (expected, json!(1.0)),
            (expected, json!(-1)),
            (expected, json!(max + 1)),
            (expected, json!(i64::MAX)),
            (expected, Value::Null),
            (resulting, json!(0)),
            (resulting, json!(max + 1)),
            (resulting, json!(u64::MAX)),
            (resulting, json!(true)),
            ("trigger", json!("detail display")),
            ("trigger", json!("DETAIL_DISPLAY")),
            ("trigger", Value::Null),
            ("operation_id", json!(OP.to_uppercase())),
            ("operation_id", json!(NIL)),
            ("operation_id", json!(42)),
            ("document_version_id", json!("not-a-uuid")),
        ] {
            let row = with_data(name, |data| {
                data.insert(field.to_owned(), value.clone());
            });
            cases.push((field, row, invalid(field)));
        }
        for field in [
            "document_version_id",
            "operation_id",
            expected,
            resulting,
            "trigger",
        ] {
            let row = with_data(name, |data| {
                data.remove(field);
            });
            cases.push((field, row, at(C::MissingField, field)));
        }
        // The version is client-chosen (VersionWrite.targetVersionId).
        let row = with_data(name, |data| {
            data.insert("document_version_id".to_owned(), json!(NIL));
        });
        cases.push((
            "nil version detail",
            row,
            at(C::NilClientId, "document_version_id"),
        ));
        let mut row = fixture_named(name).row;
        row.resource_version_id = Some(NIL.to_owned());
        cases.push((
            "nil resource version",
            row,
            at(C::NilClientId, "data.resource.version_id"),
        ));
        // The payload version must be the row's version.
        let row = with_data(name, |data| {
            data.insert("document_version_id".to_owned(), json!(BASE_VER));
        });
        cases.push(("version binding", row, invalid("document_version_id")));
        let mut row = fixture_named(name).row;
        row.resource_version_id = None;
        cases.push((
            "no resource version",
            row,
            at(C::InvalidResource, "data.resource"),
        ));
        let mut row = fixture_named(name).row;
        row.subject = format!("document/{DOC}/version/{VER}");
        cases.push(("subject", row, at(C::InvalidSubject, "subject")));
        let mut row = fixture_named(name).row;
        row.result = "failure".to_owned();
        cases.push(("result", row, at(C::InvalidResult, "data.result")));
        let mut row = fixture_named(name).row;
        row.resource_type = "Folder".to_owned();
        cases.push((
            "resource type",
            row,
            at(C::InvalidResource, "data.resource"),
        ));
        let mut row = fixture_named(name).row;
        row.reason_kind = Some("string".to_owned());
        row.reason_bytes = Some(4);
        cases.push(("reason", row, at(C::UnknownField, "reason")));
    }
    // Each type admits only its own trigger; first_record is VIEW-only and
    // always written there.
    let row = with_data(VIEWED, |data| {
        data.insert("trigger".to_owned(), json!("user_reset"));
    });
    cases.push(("view with reset trigger", row, invalid("trigger")));
    let row = with_data(UNREAD, |data| {
        data.insert("trigger".to_owned(), json!("detail_display"));
    });
    cases.push(("reset with view trigger", row, invalid("trigger")));
    for value in [json!(false), json!(true)] {
        let row = with_data(UNREAD, |data| {
            data.insert("first_record".to_owned(), value.clone());
        });
        cases.push(("first_record on reset", row, at(C::UnknownField, "data")));
    }
    let row = with_data(VIEWED, |data| {
        data.remove("first_record");
    });
    cases.push((
        "view without first_record",
        row,
        at(C::MissingField, "first_record"),
    ));
    for value in [json!("true"), json!(1), Value::Null] {
        let row = with_data(VIEWED, |data| {
            data.insert("first_record".to_owned(), value.clone());
        });
        cases.push(("first_record type", row, invalid("first_record")));
    }
    // A RESET row restaged as a VIEW type (or the reverse) does not fit.
    let mut row = fixture_named(UNREAD).row;
    row.event_type = "document.version.detail_viewed".to_owned();
    cases.push((
        "reset payload as view",
        row,
        at(C::MissingField, "first_record"),
    ));
    let mut row = fixture_named(VIEWED).row;
    row.event_type = "document.version.marked_unread".to_owned();
    cases.push(("view payload as reset", row, at(C::UnknownField, "data")));

    for (label, row, want) in cases {
        let rejection = reject(&row);
        assert_eq!(rejection, want, "{} / {label}", row.event_type);
        assert!(!rejection.to_string().contains("synthetic"));
    }
}

#[test]
fn row_columns_are_validated() {
    let base = || fixture_named("document.version.created/later").row;
    let mut cases: Vec<(&str, DocumentStagingProjection, Rejection)> = Vec::new();
    let trace = at(C::InvalidSourceCorrelation, "trace_id");
    let resource = at(C::InvalidResource, "data.resource");
    let subject = at(C::InvalidSubject, "subject");
    let actor = at(C::InvalidActor, "data.actor");
    let provenance = at(C::InvalidProvenance, "data.provenance");
    let mut r = base();
    r.trace_id = Some("request-1234".to_owned());
    cases.push(("trace not uuid", r, trace));
    let mut r = base();
    r.trace_id = Some(CORR.to_uppercase());
    cases.push(("trace uppercase", r, trace));
    let mut r = base();
    r.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned());
    cases.push(("trace w3c", r, trace));
    let mut r = base();
    r.trace_id = Some(NIL.to_owned());
    cases.push(("trace nil", r, trace));
    let mut r = base();
    r.resource_id = NIL.to_owned();
    // Document ids are server-generated: a nil one is not a client id.
    cases.push(("nil document resource", r, resource));
    let mut r = base();
    r.resource_version_id = Some(NIL.to_owned());
    cases.push((
        "nil version",
        r,
        at(C::NilClientId, "data.resource.version_id"),
    ));
    let mut r = base();
    r.resource_version_id = None;
    cases.push(("missing version", r, resource));
    let mut r = fixture_named("document.created").row;
    r.resource_version_id = Some(VER.to_owned());
    cases.push(("unexpected version", r, resource));
    let mut r = base();
    r.resource_type = "Folder".to_owned();
    cases.push(("resource type", r, resource));
    let mut r = base();
    r.subject = format!("document/{OTHER_DOC}/version/{VER}");
    cases.push(("subject other document", r, subject));
    let mut r = fixture_named("document.file.access_granted/download").row;
    r.subject = format!("document/{DOC}/version/{VER}/representation/{ITEM}");
    cases.push(("subject representation binding", r, subject));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.subject = format!("document/{FOLDER}");
    cases.push(("subject resource scope", r, subject));
    let mut r = base();
    r.result = "denied".to_owned();
    cases.push(("result", r, at(C::InvalidResult, "data.result")));
    let mut r = base();
    r.actor_principal_id = "p".repeat(257);
    cases.push(("actor too long", r, actor));
    let mut r = base();
    r.actor_principal_id = "poc\nhuman".to_owned();
    cases.push(("actor control", r, actor));
    let mut r = base();
    r.actor_principal_id = "poc\u{202d}human".to_owned();
    cases.push(("actor bidi", r, actor));
    let mut r = base();
    r.actor_identity_provider = "poc ".to_owned();
    cases.push(("actor trailing space", r, actor));
    let mut r = base();
    r.actor_identity_provider = "\u{2003}".to_owned();
    cases.push(("actor whitespace only", r, actor));
    let mut r = base();
    r.actor_principal_id = "poc\u{e0068}\u{e0069}".to_owned();
    cases.push(("actor tag smuggling", r, actor));
    let mut r = base();
    r.actor_principal_id = "poc\u{ffff}".to_owned();
    cases.push(("actor noncharacter", r, actor));
    let mut r = base();
    r.source = "urn:knowledge-platform:search-platform".to_owned();
    cases.push(("source", r, at(C::InvalidSource, "source")));
    let mut r = base();
    r.source = AUDIT_STORE_SOURCE.to_owned();
    cases.push(("control source", r, at(C::ControlTypeForbidden, "source")));
    let mut r = base();
    r.event_type = "audit.access.denied".to_owned();
    cases.push(("control type", r, at(C::ControlTypeForbidden, "event_type")));
    let mut r = base();
    r.resource_type = "AuditStore".to_owned();
    cases.push((
        "control resource",
        r,
        at(C::ControlTypeForbidden, "resource_type"),
    ));
    let mut r = base();
    r.event_type = "document.version.deleted".to_owned();
    cases.push(("unknown type", r, at(C::UnknownEventType, "event_type")));
    let mut r = base();
    r.event_id = EVENT_ID.to_uppercase();
    cases.push(("event id", r, at(C::InvalidEnvelope, "id")));
    let mut r = base();
    r.occurred_at = "2026-10-07 01:02:03.456789+00".to_owned();
    cases.push(("occurred_at", r, at(C::InvalidEnvelope, "time")));
    let mut r = base();
    r.registration_kind = "manual".to_owned();
    cases.push(("registration", r, provenance));
    let mut r = base();
    r.source_commitment = "zz".repeat(32);
    cases.push(("commitment", r, provenance));
    let mut r = base();
    r.data_kind = "array".to_owned();
    r.data = Some(json!([]));
    cases.push(("data array", r, at(C::InvalidField, "data")));
    let mut r = base();
    r.data = None;
    cases.push(("data null", r, at(C::InvalidField, "data")));
    let mut r = base();
    r.data_kind = "object".to_owned();
    r.data = Some(json!("scalar"));
    cases.push(("data scalar", r, at(C::InvalidField, "data")));
    let mut r = base();
    r.source_intact = false;
    cases.push(("source digest", r, Rejection::new(C::SourceDigestMismatch)));
    let mut r = base();
    r.oversize = true;
    r.source_intact = false;
    r.data = None;
    cases.push((
        "digest before oversize",
        r,
        Rejection::new(C::SourceDigestMismatch),
    ));
    let mut r = base();
    r.oversize = true;
    r.data = None;
    cases.push(("honest oversize", r, Rejection::new(C::SourceRowTooLarge)));
    let mut r = base();
    r.source_intact = false;
    r.data_kind = "array".to_owned();
    r.data = Some(json!([1]));
    r.event_type = "audit.access.denied".to_owned();
    r.actor_principal_id = "x\u{202e}".to_owned();
    cases.push((
        "tamper with an invalid shape",
        r,
        Rejection::new(C::SourceDigestMismatch),
    ));
    let mut r = fixture_named("document.created").row;
    r.data.as_mut().expect("data")["documentId"] = json!(OTHER_DOC);
    cases.push(("binding resource id", r, at(C::InvalidField, "documentId")));
    let mut r = fixture_named("document.publication.ended").row;
    r.data.as_mut().expect("data")["formerCurrentVersionId"] = json!(BASE_VER);
    cases.push((
        "binding version id",
        r,
        at(C::InvalidField, "formerCurrentVersionId"),
    ));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.data.as_mut().expect("data")["target_type"] = json!("Document");
    cases.push((
        "binding resource type",
        r,
        at(C::InvalidField, "target_type"),
    ));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.data.as_mut().expect("data")["target_id"] = json!(OTHER_FOLDER);
    cases.push(("binding target id", r, at(C::InvalidField, "target_id")));

    for (label, row, expected) in cases {
        assert_eq!(reject(&row), expected, "{label}");
    }
}

#[test]
fn nil_resource_ids_are_nil_client_id_only_where_clients_choose_them() {
    project(&fixture_named("authorization.denied/management").row).expect("nil allowed");
    // Folder ids are chosen by clients (CreateFolder.folderId).
    for name in [
        "folder.created",
        "folder.renamed",
        "access_policy.changed/folder_inherit",
    ] {
        let mut row = fixture_named(name).row;
        row.resource_id = NIL.to_owned();
        assert_eq!(
            reject(&row),
            at(C::NilClientId, "data.resource.id"),
            "{name}"
        );
    }
    // Document ids are server-generated UUIDv7: a nil one is not
    // producer-reachable.
    for name in ["document.created", "access_policy.changed/document"] {
        let mut row = fixture_named(name).row;
        row.resource_id = NIL.to_owned();
        assert_eq!(
            reject(&row),
            at(C::InvalidResource, "data.resource"),
            "{name}"
        );
    }
    // Version ids may be chosen by clients (VersionWrite.targetVersionId).
    for name in [
        "document.version.created/later",
        "document.version.read_confirmed",
        "document.diff.result_access_granted/compare",
    ] {
        let mut row = fixture_named(name).row;
        row.resource_version_id = Some(NIL.to_owned());
        assert_eq!(
            reject(&row),
            at(C::NilClientId, "data.resource.version_id"),
            "{name}"
        );
    }
}

#[test]
fn nil_client_chosen_ids_are_quarantined_as_nil_client_id() {
    let cases: [(&str, &str); 10] = [
        ("folder.created", "parent_folder_id"),
        ("folder.moved", "from_parent_id"),
        ("folder.moved", "to_parent_id"),
        ("document.moved", "to_folder_id"),
        ("document.version.created/later", "baseDocumentVersionId"),
        ("document.version.created/later", "documentVersionId"),
        (
            "document.version.withdrawn/restored",
            "resultingCurrentVersionId",
        ),
        (
            "document.diff.result_access_granted/compare",
            "base_version_id",
        ),
        ("document.version.read_confirmed", "document_version_id"),
        ("access_policy.changed/folder_inherit", "target_id"),
    ];
    for (name, field) in cases {
        let row = with_data(name, |data| {
            data.insert(field.to_owned(), json!(NIL));
        });
        assert_eq!(reject(&row), at(C::NilClientId, field), "{name}/{field}");
    }
    // Server-generated or UUIDv7-validated ids: a nil one is an invalid
    // field, not a producer-reachable client id.
    let server: [(&str, &str); 6] = [
        ("document.created", "documentId"),
        (
            "document.revision_comparison.result_access_granted/same_version",
            "target_revision_id",
        ),
        ("document.file.access_granted/download", "content_item_id"),
        ("access_policy.changed/document", "policy_id"),
        ("document.version.published/manual", "publishOperationId"),
        ("document.metadata.changed", "operation_id"),
    ];
    for (name, field) in server {
        let row = with_data(name, |data| {
            data.insert(field.to_owned(), json!(NIL));
        });
        assert_eq!(reject(&row), at(C::InvalidField, field), "{name}/{field}");
    }
    // A nil folder id that is also the resource id fails at the resource.
    let mut row = fixture_named("folder.renamed").row;
    row.resource_id = NIL.to_owned();
    row.subject = format!("folder/{NIL}");
    row.data.as_mut().expect("data")["folder_id"] = json!(NIL);
    assert_eq!(reject(&row), at(C::NilClientId, "data.resource.id"));
}

#[test]
fn control_characters_and_oversized_strings_are_rejected_without_echo() {
    let mut row = fixture_named("document.created").row;
    row.subject = format!("document/{DOC}\u{0}");
    let rejection = reject(&row);
    assert_eq!(rejection, at(C::InvalidSubject, "subject"));
    let mut row = fixture_named("document.created").row;
    row.actor_identity_provider = "i".repeat(600);
    let rejection = reject(&row);
    assert_eq!(rejection, at(C::InvalidActor, "data.actor"));
    assert!(!rejection.to_string().contains("iii"));
}
