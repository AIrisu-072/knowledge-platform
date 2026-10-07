//! Legacy claim-row projection: every main producer shape is accepted with the
//! expected envelope, and every unsafe or inconsistent row is quarantined.

mod common;

use audit_core::catalog::{AUDIT_STORE_SOURCE, DOCUMENT_SOURCE};
use audit_core::envelope::{DATASCHEMA, LEGACY_SOURCE_FORMAT, MAX_ENVELOPE_BYTES};
use audit_core::{Catalog, DocumentStagingProjection, RejectionCode as C, project};
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
        assert!(text.len() <= MAX_ENVELOPE_BYTES, "{name}");
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

fn reject(row: &DocumentStagingProjection) -> audit_core::Rejection {
    project(row).expect_err("must be rejected")
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
            assert_eq!(rejection.code, C::UnknownField, "{name}/{key}");
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
    assert_eq!(rejection.code, C::UnknownField);
    assert!(!format!("{rejection:?} {rejection}").contains("SYNTHETIC"));

    let mut missing = fixture_named("document.metadata.changed").row;
    missing.reason_kind = None;
    missing.reason_bytes = None;
    assert_eq!(reject(&missing).code, C::InvalidReason);

    let mut no_bytes = fixture_named("folder.created").row;
    no_bytes.reason_bytes = None;
    assert_eq!(reject(&no_bytes).code, C::InvalidReason);

    let mut negative = fixture_named("folder.created").row;
    negative.reason_bytes = Some(-1);
    assert_eq!(reject(&negative).code, C::InvalidReason);

    for kind in ["number", "object", "array", "null", "boolean"] {
        let mut row = fixture_named("document.publication.ended").row;
        row.reason_kind = Some(kind.to_owned());
        assert_eq!(reject(&row).code, C::ReasonNotString, "{kind}");
    }

    let mut on_acl = fixture_named("access_policy.changed/document").row;
    on_acl.reason_kind = Some("string".to_owned());
    on_acl.reason_bytes = Some(3);
    assert_eq!(reject(&on_acl).code, C::UnknownField);
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
        assert_eq!(reject(&row).code, C::ActorMismatch, "{name}");
        let row = with_data(name, |data| {
            data.remove("actor");
        });
        assert_eq!(reject(&row).code, C::MissingField, "{name}");
        let row = with_data(name, |data| {
            data.insert("actor".to_owned(), json!({"identityProvider": "poc"}));
        });
        assert_eq!(reject(&row).code, C::InvalidActor, "{name}");
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
    assert_eq!(reject(&row).code, C::InvalidServiceExecutor);
    let row = with_data("document.version.publication.terminal/scheduler", |data| {
        data.insert(
            "serviceExecutor".to_owned(),
            json!({"identityProvider": "service", "principalId": "sch\u{1b}eduler"}),
        );
    });
    assert_eq!(reject(&row).code, C::InvalidServiceExecutor);
    let row = with_data("document.created", |data| {
        data.insert("serviceExecutor".to_owned(), scheduler());
    });
    assert_eq!(reject(&row).code, C::UnknownField);
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

#[test]
fn row_columns_are_validated() {
    let base = || fixture_named("document.version.created/later").row;
    let mut cases: Vec<(&str, DocumentStagingProjection, C)> = Vec::new();
    let mut r = base();
    r.trace_id = Some("request-1234".to_owned());
    cases.push(("trace not uuid", r, C::InvalidSourceCorrelation));
    let mut r = base();
    r.trace_id = Some(CORR.to_uppercase());
    cases.push(("trace uppercase", r, C::InvalidSourceCorrelation));
    let mut r = base();
    r.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned());
    cases.push(("trace w3c", r, C::InvalidSourceCorrelation));
    let mut r = base();
    r.trace_id = Some(NIL.to_owned());
    cases.push(("trace nil", r, C::InvalidSourceCorrelation));
    let mut r = base();
    r.resource_id = NIL.to_owned();
    cases.push(("nil resource", r, C::InvalidResource));
    let mut r = base();
    r.resource_version_id = None;
    cases.push(("missing version", r, C::InvalidResource));
    let mut r = fixture_named("document.created").row;
    r.resource_version_id = Some(VER.to_owned());
    cases.push(("unexpected version", r, C::InvalidResource));
    let mut r = base();
    r.resource_type = "Folder".to_owned();
    cases.push(("resource type", r, C::InvalidResource));
    let mut r = base();
    r.subject = format!("document/{OTHER_DOC}/version/{VER}");
    cases.push(("subject other document", r, C::InvalidSubject));
    let mut r = fixture_named("document.file.access_granted/download").row;
    r.subject = format!("document/{DOC}/version/{VER}/representation/{ITEM}");
    cases.push(("subject representation binding", r, C::InvalidSubject));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.subject = format!("document/{FOLDER}");
    cases.push(("subject resource scope", r, C::InvalidSubject));
    let mut r = base();
    r.result = "denied".to_owned();
    cases.push(("result", r, C::InvalidResult));
    let mut r = base();
    r.actor_principal_id = "p".repeat(257);
    cases.push(("actor too long", r, C::InvalidActor));
    let mut r = base();
    r.actor_principal_id = "poc\nhuman".to_owned();
    cases.push(("actor control", r, C::InvalidActor));
    let mut r = base();
    r.source = "urn:knowledge-platform:search-platform".to_owned();
    cases.push(("source", r, C::InvalidSource));
    let mut r = base();
    r.source = AUDIT_STORE_SOURCE.to_owned();
    cases.push(("control source", r, C::ControlTypeForbidden));
    let mut r = base();
    r.event_type = "audit.access.denied".to_owned();
    cases.push(("control type", r, C::ControlTypeForbidden));
    let mut r = base();
    r.resource_type = "AuditStore".to_owned();
    cases.push(("control resource", r, C::ControlTypeForbidden));
    let mut r = base();
    r.event_type = "document.version.deleted".to_owned();
    cases.push(("unknown type", r, C::UnknownEventType));
    let mut r = base();
    r.event_id = EVENT_ID.to_uppercase();
    cases.push(("event id", r, C::InvalidEnvelope));
    let mut r = base();
    r.occurred_at = "2026-10-07 01:02:03.456789+00".to_owned();
    cases.push(("occurred_at", r, C::InvalidEnvelope));
    let mut r = base();
    r.registration_kind = "manual".to_owned();
    cases.push(("registration", r, C::InvalidProvenance));
    let mut r = base();
    r.source_commitment = "zz".repeat(32);
    cases.push(("commitment", r, C::InvalidProvenance));
    let mut r = base();
    r.data_kind = "array".to_owned();
    r.data = Some(json!([]));
    cases.push(("data array", r, C::InvalidField));
    let mut r = base();
    r.data = None;
    cases.push(("data null", r, C::InvalidField));
    let mut r = base();
    r.source_intact = false;
    cases.push(("source digest", r, C::SourceDigestMismatch));
    let mut r = base();
    r.oversize = true;
    r.source_intact = false;
    r.data = None;
    cases.push(("oversize first", r, C::SourceRowTooLarge));
    let mut r = fixture_named("document.created").row;
    r.data.as_mut().expect("data")["documentId"] = json!(OTHER_DOC);
    cases.push(("binding resource id", r, C::InvalidField));
    let mut r = fixture_named("document.publication.ended").row;
    r.data.as_mut().expect("data")["formerCurrentVersionId"] = json!(BASE_VER);
    cases.push(("binding version id", r, C::InvalidField));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.data.as_mut().expect("data")["target_type"] = json!("Document");
    cases.push(("binding resource type", r, C::InvalidField));
    let mut r = fixture_named("access_policy.changed/folder_inherit").row;
    r.data.as_mut().expect("data")["target_id"] = json!(OTHER_FOLDER);
    cases.push(("binding target id", r, C::InvalidField));

    for (label, row, code) in cases {
        assert_eq!(reject(&row).code, code, "{label}");
    }
}

#[test]
fn nil_resource_is_only_for_authorization_denied() {
    project(&fixture_named("authorization.denied/management").row).expect("nil allowed");
    for name in [
        "document.created",
        "folder.created",
        "access_policy.changed/document",
    ] {
        let mut row = fixture_named(name).row;
        row.resource_id = NIL.to_owned();
        assert_eq!(reject(&row).code, C::InvalidResource, "{name}");
    }
}

#[test]
fn control_characters_and_oversized_strings_are_rejected_without_echo() {
    let mut row = fixture_named("document.created").row;
    row.subject = format!("document/{DOC}\u{0}");
    let rejection = reject(&row);
    assert_eq!(rejection.code, C::InvalidSubject);
    let mut row = fixture_named("document.created").row;
    row.actor_identity_provider = "i".repeat(600);
    let rejection = reject(&row);
    assert_eq!(rejection.code, C::InvalidActor);
    assert!(!rejection.to_string().contains("iii"));
}
