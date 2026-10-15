//! Envelope-level contract: closed CloudEvents attributes, payload v1 rules and
//! the rejection table shared with the schema relation test.

mod common;

use audit_core::catalog::FieldSpec;
use audit_core::kinds::{Kind, MAX_CONTROL_LIST, MAX_DB_ROLE_BYTES, MAX_UUID_LIST};
use audit_core::{
    AuditEnvelope, Catalog, EventSpec, JSONB_TEXT_LIMIT, Origin, RejectionCode as C,
    jsonb_text_len, parse_unique, validate_envelope,
};
use common::*;
use serde_json::{Map, Value, json};

#[test]
fn every_rejection_case_is_rejected_with_its_code() {
    let cases = envelope_rejections();
    assert!(cases.len() >= 85, "rejection table shrank");
    for case in cases {
        let result = match &case.input {
            Input::Value(value) => {
                let direct = validate_envelope(value, case.path);
                let built = AuditEnvelope::from_value(value.clone(), case.path).map(|_| ());
                assert_eq!(direct, built, "{}", case.name);
                direct
            }
            Input::Text(text) => AuditEnvelope::from_json(text, case.path).map(|_| ()),
        };
        let rejection = result.expect_err(case.name);
        assert_eq!(rejection, case.expected, "{}: got {rejection}", case.name);
    }
}

#[test]
fn projected_envelopes_round_trip_through_json_text() {
    for fixture in accepted_fixtures() {
        let envelope = audit_core::project(&fixture.row).expect("projects");
        let text = envelope.to_json_string();
        let reparsed = AuditEnvelope::from_json(&text, Origin::Relay).expect(fixture.name);
        assert_eq!(reparsed, envelope, "{}", fixture.name);
        assert_eq!(reparsed.to_json_string(), text, "{}", fixture.name);
        assert_eq!(reparsed.id(), envelope.id());
        assert_eq!(reparsed.spec().event_type, fixture.row.event_type);
        // A pretty-printed rendering is the same envelope.
        let pretty = serde_json::to_string_pretty(envelope.as_value()).expect("pretty");
        assert_eq!(
            AuditEnvelope::from_json(&pretty, Origin::Relay).expect("pretty parses"),
            envelope
        );
    }
}

#[test]
fn accessors_expose_cloudevents_attributes() {
    let envelope =
        audit_core::project(&fixture_named("document.file.access_granted/diff_display").row)
            .expect("projects");
    assert_eq!(envelope.event_type(), "document.file.access_granted");
    assert_eq!(envelope.source(), audit_core::catalog::DOCUMENT_SOURCE);
    assert_eq!(
        envelope.subject(),
        format!("document/{DOC}/version/{VER}/representation/{REP}")
    );
    assert_eq!(envelope.time(), OCCURRED);
    assert_eq!(envelope.data()["details"]["purpose"], "history");
    assert_eq!(envelope.origin(), Origin::Relay);
}

#[test]
fn from_value_canonicalizes_member_order() {
    let value = envelope_of("document.version.published/scheduled");
    let mut reversed = serde_json::Map::new();
    for (key, member) in value.as_object().expect("object").iter().rev() {
        reversed.insert(key.clone(), member.clone());
    }
    let envelope = AuditEnvelope::from_value(serde_json::Value::Object(reversed), Origin::Relay)
        .expect("valid");
    assert!(envelope.to_json_string().starts_with("{\"data\":"));
}

#[test]
fn size_limit_is_measured_on_the_jsonb_rendering() {
    let envelope =
        audit_core::project(&fixture_named("document.diff.result_access_granted/display").row)
            .expect("projects");
    let text = envelope.to_json_string();
    let jsonb = jsonb_text_len(envelope.as_value());
    assert!(jsonb > text.len(), "jsonb adds separator spaces");
    assert!(
        jsonb < JSONB_TEXT_LIMIT / 4,
        "realistic envelopes stay small"
    );
    let oversized = json!({"padding": "x".repeat(JSONB_TEXT_LIMIT)});
    assert_eq!(
        validate_envelope(&oversized, Origin::Relay),
        Err(bare(C::EnvelopeTooLarge))
    );
}

/// The largest value of a kind: longest digits, escape-heavy strings (each
/// `"` renders as two bytes), full lists.
fn maximal_value(field: &FieldSpec, salt: usize) -> Value {
    let escaped = |bytes: usize, salt: usize| {
        let mut text = "\"".repeat(bytes - 3);
        text.push_str(&format!("{:03}", salt % 1000));
        text
    };
    let uuid = |n: usize| format!("0199a1b2-0000-7000-8000-{n:012x}");
    // The longest event type name (128 bytes), distinct per index.
    let event_type = |n: usize| format!("{}.x{:0>63}", "e".repeat(63), n);
    match field.kind {
        Kind::Uuid | Kind::NullableUuid => json!(uuid(salt + 1)),
        Kind::Counter
        | Kind::NullableCounter
        | Kind::PositiveCounter
        | Kind::NullablePositiveCounter => json!(i64::MAX),
        Kind::SafeCounter | Kind::PositiveSafeCounter => {
            json!(audit_core::kinds::MAX_SAFE_INTEGER)
        }
        Kind::Boolean => json!(false),
        Kind::Enum | Kind::NullableEnum => {
            json!(field.values.iter().max_by_key(|v| v.len()).expect("values"))
        }
        Kind::EnumList => json!(field.values),
        Kind::Digest | Kind::NullableDigest => Value::Array(vec![json!(255); 32]),
        Kind::Principal => json!({
            "identityProvider": escaped(256, salt),
            "principalId": escaped(256, salt + 1)
        }),
        Kind::LegacyTime => json!([-9999, 365, 23, 59, 59, 999_999_999, -25, -59, -59]),
        Kind::UtcTimestamp | Kind::NullableUtcTimestamp => json!(OCCURRED),
        Kind::UuidList => json!(
            (0..MAX_UUID_LIST)
                .map(|i| uuid(1000 + i))
                .collect::<Vec<_>>()
        ),
        Kind::HexDigest | Kind::NullableHexDigest => json!("f".repeat(64)),
        Kind::ResourceRef => json!(uuid(salt + 1)),
        Kind::EventType => json!(event_type(salt)),
        Kind::EventTypeList => json!((0..MAX_CONTROL_LIST).map(event_type).collect::<Vec<_>>()),
        Kind::SourceUrn => json!(field.values.iter().max_by_key(|v| v.len()).expect("values")),
        Kind::SourceList => json!(field.values),
        Kind::DbRole | Kind::NullableDbRole => json!("r".repeat(MAX_DB_ROLE_BYTES)),
        Kind::PrincipalRef => json!(escaped(256, salt)),
        Kind::Int8Text => json!(i64::MIN.to_string()),
        Kind::Code => json!("c".repeat(64)),
    }
}

/// The largest envelope a catalog entry admits.
fn maximal_envelope(spec: &EventSpec) -> Value {
    let details: Map<String, Value> = spec
        .detail_fields()
        .enumerate()
        .map(|(i, (name, field))| (name.to_owned(), maximal_value(field, i)))
        .collect();
    let mut value = if spec.origin == Origin::Relay {
        let fixture = accepted_fixtures()
            .into_iter()
            .find(|f| f.row.event_type == spec.event_type)
            .expect("fixture");
        audit_core::project(&fixture.row)
            .expect("projects")
            .into_value()
    } else {
        control_envelope(spec, true)
    };
    let principal = json!({"issuer": "\"".repeat(256), "principal_id": "\"".repeat(256)});
    value["data"]["actor"] = principal.clone();
    if spec.service_executor_field.is_some() {
        value["data"]["service_executor"] = principal;
    }
    // Keep bound fields (resource, subject, correlation) consistent.
    let mut merged = value["data"]["details"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (name, member) in details {
        let bound = spec.bindings.iter().any(|b| b.field == name)
            || spec
                .subjects
                .iter()
                .any(|s| s.template.contains(&format!("{{details.{name}}}")))
            || [
                &spec.operation_id_field,
                &spec.publish_operation_id_field,
                &spec.reason_code_field,
            ]
            .iter()
            .any(|f| f.as_deref() == Some(name.as_str()));
        if !bound || !merged.contains_key(&name) {
            merged.insert(name, member);
        }
    }
    value["data"]["details"] = Value::Object(merged);
    if spec.reason == audit_core::catalog::ReasonPolicy::CallerText {
        value["data"]["reason"]["utf8_bytes"] = json!(1_048_576);
    }
    if spec.origin == Origin::Relay {
        value["data"]["correlation"]["source_correlation_id"] = json!(CORR);
    }
    value
}

#[test]
fn maximal_envelopes_of_every_entry_fit_the_jsonb_limit() {
    let mut largest = 0;
    let mut largest_control = 0;
    for spec in Catalog::embedded().events() {
        let value = maximal_envelope(spec);
        if let Err(rejection) = validate_envelope(&value, spec.origin) {
            panic!(
                "{}: maximal envelope rejected: {rejection}",
                spec.event_type
            );
        }
        let len = jsonb_text_len(&value);
        assert!(len <= JSONB_TEXT_LIMIT, "{}: {len} bytes", spec.event_type);
        if spec.origin == Origin::Relay {
            largest = largest.max(len);
        } else {
            largest_control = largest_control.max(len);
        }
    }
    assert!(
        largest * 2 < JSONB_TEXT_LIMIT,
        "relay envelopes keep a 2x margin ({largest} bytes)"
    );
    assert!(
        largest_control * 2 < JSONB_TEXT_LIMIT,
        "closed control kinds keep a 2x margin ({largest_control} bytes)"
    );
}

#[test]
fn raw_json_admission_rejects_ambiguous_input() {
    assert_eq!(
        parse_unique(r#"{"a":1,"a":1}"#).expect_err("dup").code,
        C::DuplicateKey
    );
    assert_eq!(
        parse_unique("{} []").expect_err("trailing").code,
        C::InvalidJson
    );
    assert_eq!(parse_unique("").expect_err("empty").code, C::InvalidJson);
    assert_eq!(
        AuditEnvelope::from_json("[]", Origin::Relay)
            .expect_err("array")
            .code,
        C::InvalidEnvelope
    );
    let deep = format!("{}1{}", "[".repeat(9), "]".repeat(9));
    assert_eq!(parse_unique(&deep).expect_err("deep").code, C::InvalidJson);
}

#[test]
fn rejection_display_never_contains_values() {
    for case in envelope_rejections() {
        let rejection = match &case.input {
            Input::Value(value) => validate_envelope(value, case.path),
            Input::Text(text) => AuditEnvelope::from_json(text, case.path).map(|_| ()),
        }
        .expect_err(case.name);
        let rendered = format!("{rejection} {rejection:?}");
        for value in [
            DOC, OTHER_DOC, PRINCIPAL, "free", "why", "\u{7}", "ééé", "req-123", "nimda",
        ] {
            assert!(!rendered.contains(value), "{}: leaked {value:?}", case.name);
        }
    }
}

#[test]
fn control_envelopes_validate_only_on_their_own_path() {
    let envelopes = control_envelopes();
    assert_eq!(envelopes.len(), 28, "14 control types, minimal and full");
    for (name, origin, value) in envelopes {
        let envelope = AuditEnvelope::from_value(value.clone(), origin)
            .unwrap_or_else(|r| panic!("{name}: {r}"));
        assert_eq!(envelope.origin(), origin, "{name}");
        assert_eq!(
            validate_envelope(&value, Origin::Relay),
            Err(at(C::ControlTypeForbidden, "type")),
            "{name}: the relay path refuses control events"
        );
        let other = if origin == Origin::Store {
            Origin::RelayControl
        } else {
            Origin::Store
        };
        assert_eq!(
            validate_envelope(&value, other),
            Err(at(C::ControlTypeForbidden, "type")),
            "{name}: wrong control path"
        );
    }
}

#[test]
fn every_control_entry_refuses_forged_provenance_resource_and_commitment() {
    let catalog = Catalog::embedded();
    let controls: Vec<&EventSpec> = catalog
        .events()
        .iter()
        .filter(|spec| spec.origin != Origin::Relay)
        .collect();
    assert_eq!(controls.len(), 14);
    for spec in controls {
        let name = &spec.event_type;
        let base = control_envelope(spec, false);
        let other_format = if spec.origin == Origin::Store {
            "audit-relay-control-v1"
        } else {
            "audit-store-control-v1"
        };
        let mut format = base.clone();
        format["data"]["provenance"]["source_format"] = json!(other_format);
        let mut legacy_format = base.clone();
        legacy_format["data"]["provenance"]["source_format"] = json!("document-audit-outbox-v0");
        let mut commitment = base.clone();
        commitment["data"]["provenance"]["source_commitment"] = json!("ab".repeat(32));
        let mut registration = base.clone();
        registration["data"]["provenance"]["registration"] = json!("trigger");
        let mut resource_id = base.clone();
        resource_id["data"]["resource"]["id"] = json!(DOC);
        let mut resource_name = base.clone();
        resource_name["data"]["resource"]["id"] = json!("audit-store-2");
        let mut version = base.clone();
        version["data"]["resource"]["version_id"] = json!(VER);
        let mut source = base.clone();
        source["source"] = json!(audit_core::catalog::DOCUMENT_SOURCE);
        let mut trace = base.clone();
        trace["data"]["correlation"]["trace_id"] = json!("4bf92f3577b34da6a3ce929d0e0e4736");
        let provenance = at(C::InvalidProvenance, "data.provenance");
        let resource = at(C::InvalidResource, "data.resource");
        for (label, value, expected) in [
            ("other control format", format, provenance),
            ("legacy format", legacy_format, provenance),
            ("commitment", commitment, provenance),
            ("registration", registration, provenance),
            ("resource id uuid", resource_id, resource),
            ("resource id other name", resource_name, resource),
            ("version id", version, resource),
            ("document source", source, at(C::InvalidSource, "source")),
            (
                "trace id",
                trace,
                at(C::InvalidCorrelation, "data.correlation.trace_id"),
            ),
        ] {
            assert_eq!(
                validate_envelope(&value, spec.origin),
                Err(expected),
                "{name}: {label}"
            );
        }
    }
}

#[test]
fn control_envelopes_are_closed() {
    let spec = audit_core::Catalog::embedded()
        .get("audit.access.denied")
        .expect("control type");
    let base = control_envelope(spec, false);
    let mut unknown = base.clone();
    unknown["data"]["details"]["free_text"] = json!("x");
    let mut bad_code = base.clone();
    bad_code["data"]["details"]["denial_code"] = json!("other");
    let mut bad_role = base.clone();
    bad_role["data"]["details"]["session_role"] = json!("a\u{7}");
    let mut wrong_subject = base.clone();
    wrong_subject["subject"] = json!("audit-store/other");
    let mut actor = base;
    actor["data"]["actor"]["principal_id"] = json!("op\u{202e}");
    for (name, value, expected) in [
        (
            "unknown detail",
            unknown,
            at(C::UnknownField, "data.details"),
        ),
        ("enum value", bad_code, at(C::InvalidField, "denial_code")),
        (
            "identifier control char",
            bad_role,
            at(C::InvalidField, "session_role"),
        ),
        ("subject", wrong_subject, at(C::InvalidSubject, "subject")),
        ("actor charset", actor, at(C::InvalidActor, "data.actor")),
    ] {
        assert_eq!(
            validate_envelope(&value, Origin::Store),
            Err(expected),
            "{name}"
        );
    }
    // Control events carry no client-chosen ids: a nil UUID is an invalid
    // field, never the producer-facing nil_client_id.
    assert_eq!(
        validate_envelope(
            &control_with("audit.body.purged", "target_event_id", json!(NIL)),
            Origin::Store
        ),
        Err(at(C::InvalidField, "target_event_id"))
    );
    assert_eq!(
        validate_envelope(
            &control_with(
                "audit.access.intent_opened",
                "filter_event_ids",
                json!([EVENT_ID, NIL])
            ),
            Origin::Store
        ),
        Err(at(C::InvalidField, "filter_event_ids"))
    );
}

#[test]
fn closed_control_kinds_accept_the_values_the_store_records() {
    let intent = "audit.access.intent_opened";
    for (field, value) in [
        // authorization.denied rows have a nil resource id.
        ("filter_resource_id", json!(NIL)),
        ("filter_resource_id", json!("audit-store")),
        ("filter_resource_id", json!(DOC)),
        (
            "filter_source",
            json!(audit_core::catalog::AUDIT_STORE_SOURCE),
        ),
        ("filter_source", json!(audit_core::catalog::DOCUMENT_SOURCE)),
        (
            "filter_event_types",
            json!(["audit.access.denied", "document.created"]),
        ),
        ("filter_actor_issuer", json!("名前")),
    ] {
        let value = control_with(intent, field, value);
        validate_envelope(&value, Origin::Store).unwrap_or_else(|r| panic!("{field}: {r}"));
    }
    for retain_days in [Value::Null, json!(1)] {
        let value = control_with("audit.retention.policy_changed", "retain_days", retain_days);
        validate_envelope(&value, Origin::Store).expect("NULL or a positive number of days");
    }
}

#[test]
fn unbound_control_actors_are_their_session_role() {
    // Store events without a bound principal (unbound denials, bootstrap
    // from an owner login) record actor {issuer: "db_role", principal_id:
    // session_user}.
    let denied = |actor_role: &str, session_role: &str| {
        let mut value = control_with("audit.access.denied", "session_role", json!(session_role));
        value["data"]["actor"] = json!({"issuer": "db_role", "principal_id": actor_role});
        value
    };
    validate_envelope(
        &denied("audit_store_reader", "audit_store_reader"),
        Origin::Store,
    )
    .expect("db_role actor");
    assert_eq!(
        validate_envelope(&denied("other_login", "audit_store_reader"), Origin::Store),
        Err(at(C::InvalidActor, "data.actor")),
        "the db_role actor is the session role"
    );
    assert_eq!(
        validate_envelope(&denied("Odd Role", "audit_store_reader"), Origin::Store),
        Err(at(C::InvalidActor, "data.actor"))
    );
    assert_eq!(
        validate_envelope(&denied("Odd Role", "Odd Role"), Origin::Store),
        Err(at(C::InvalidField, "session_role")),
        "an odd login name is refused before it is recorded"
    );
    let relay_control = control_with(
        "audit.delivery.replay_requested",
        "session_role",
        json!("audit_relay_service"),
    );
    let mut bound = relay_control.clone();
    bound["data"]["actor"] = json!({"issuer": "db_role", "principal_id": "audit_relay_service"});
    validate_envelope(&bound, Origin::RelayControl).expect("relay control db_role actor");
    // Relay envelopes are producer data: the rule does not apply.
    let mut relay = envelope_of("document.created");
    relay["data"]["actor"] = json!({"issuer": "db_role", "principal_id": "Any Name"});
    validate_envelope(&relay, Origin::Relay).expect("relay actor");
}

/// A test catalog whose relay adapter allows `trace_id` and makes the
/// commitment optional, validated through `Catalog::validate`.
fn custom_catalog() -> &'static Catalog {
    let text = json!({
        "version": 1,
        "adapters": [{
            "source": "urn:knowledge-platform:example",
            "origin": "relay",
            "source_format": "example-adapter-v3",
            "adapter_version": 3,
            "commitment": "optional",
            "registration": "forbidden",
            "trace_id": true
        }],
        "events": [{
            "type": "example.thing.done",
            "source": "urn:knowledge-platform:example",
            "origin": "relay",
            "event_class": "CONTENT_LIFECYCLE",
            "resources": ["Document"],
            "version_required": false,
            "results": ["success"],
            "subjects": ["document/{resource.id}"],
            "fields": {"documentId": {"kind": "uuid"}},
            "required": ["documentId"],
            "reason": "absent",
            "bindings": [{"field": "documentId", "equals": "resource.id"}]
        }]
    })
    .to_string();
    Box::leak(Box::new(Catalog::from_json(&text).expect("custom catalog")))
}

fn custom_envelope() -> Value {
    json!({
        "specversion": "1.0",
        "id": EVENT_ID,
        "source": "urn:knowledge-platform:example",
        "type": "example.thing.done",
        "subject": format!("document/{DOC}"),
        "time": OCCURRED,
        "datacontenttype": "application/json",
        "dataschema": "urn:knowledge-platform:audit:payload:v1",
        "data": {
            "schema_version": 1,
            "event_class": "CONTENT_LIFECYCLE",
            "action": "example.thing.done",
            "actor": {"issuer": ISSUER, "principal_id": PRINCIPAL},
            "resource": {"type": "Document", "id": DOC},
            "result": "success",
            "correlation": {"trace_id": "4bf92f3577b34da6a3ce929d0e0e4736"},
            "details": {"documentId": DOC},
            "extensions": {},
            "provenance": {"source_format": "example-adapter-v3", "adapter_version": 3}
        }
    })
}

#[test]
fn catalog_validate_reads_the_adapter_contract() {
    let catalog = custom_catalog();
    let base = custom_envelope();
    let spec = catalog.validate(&base, Origin::Relay).expect("valid");
    assert_eq!(spec.event_type, "example.thing.done");
    let mut with_commitment = base.clone();
    with_commitment["data"]["provenance"]["source_commitment"] = json!("ab".repeat(32));
    assert!(catalog.validate(&with_commitment, Origin::Relay).is_ok());
    let mut registration = base.clone();
    registration["data"]["provenance"]["registration"] = json!("trigger");
    let mut old_version = base.clone();
    old_version["data"]["provenance"]["adapter_version"] = json!(2);
    let mut zero_trace = base.clone();
    zero_trace["data"]["correlation"]["trace_id"] = json!("0".repeat(32));
    for (label, value, expected) in [
        (
            "registration forbidden",
            registration,
            at(C::InvalidProvenance, "data.provenance"),
        ),
        (
            "adapter version must be current",
            old_version,
            at(C::InvalidProvenance, "data.provenance"),
        ),
        (
            "trace id must be W3C",
            zero_trace,
            at(C::InvalidCorrelation, "data.correlation.trace_id"),
        ),
    ] {
        assert_eq!(
            catalog.validate(&value, Origin::Relay).map(|_| ()),
            Err(expected),
            "{label}"
        );
    }
    // The embedded catalog does not know the type.
    assert_eq!(
        validate_envelope(&base, Origin::Relay),
        Err(at(C::UnknownEventType, "type"))
    );
}
