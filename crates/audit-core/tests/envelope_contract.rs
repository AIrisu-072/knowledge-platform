//! Envelope-level contract: closed CloudEvents attributes, payload v1 rules and
//! the rejection table shared with the schema relation test.

mod common;

use audit_core::envelope::MAX_ENVELOPE_BYTES;
use audit_core::{AuditEnvelope, Origin, RejectionCode as C, parse_unique, validate_envelope};
use common::*;
use serde_json::json;

#[test]
fn every_rejection_case_is_rejected_with_its_code() {
    let cases = envelope_rejections();
    assert!(cases.len() >= 60, "rejection table shrank");
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
        assert_eq!(rejection.code, case.code, "{}: got {rejection}", case.name);
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
fn size_limit_is_measured_on_the_compact_serialization() {
    let text =
        audit_core::project(&fixture_named("document.diff.result_access_granted/display").row)
            .expect("projects")
            .to_json_string();
    assert!(
        text.len() < MAX_ENVELOPE_BYTES / 8,
        "realistic envelopes stay small"
    );
    let oversized = json!({"padding": "x".repeat(MAX_ENVELOPE_BYTES)});
    assert_eq!(
        validate_envelope(&oversized, Origin::Relay)
            .expect_err("too large")
            .code,
        C::EnvelopeTooLarge
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
            DOC, OTHER_DOC, PRINCIPAL, "free", "why", "\u{7}", "ééé", "req-123",
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
            validate_envelope(&value, Origin::Relay)
                .expect_err(&name)
                .code,
            C::ControlTypeForbidden,
            "{name}: the relay path refuses control events"
        );
        let other = if origin == Origin::Store {
            Origin::RelayControl
        } else {
            Origin::Store
        };
        assert_eq!(
            validate_envelope(&value, other).expect_err(&name).code,
            C::ControlTypeForbidden,
            "{name}: wrong control path"
        );
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
    let mut commitment = base.clone();
    commitment["data"]["provenance"]["source_commitment"] = json!("ab".repeat(32));
    let mut relay_format = base.clone();
    relay_format["data"]["provenance"]["source_format"] = json!("audit-relay-control-v1");
    let mut wrong_subject = base.clone();
    wrong_subject["subject"] = json!("audit-store/other");
    let mut wrong_resource = base;
    wrong_resource["data"]["resource"]["id"] = json!(DOC);
    for (name, value, code) in [
        ("unknown detail", unknown, C::UnknownField),
        ("enum value", bad_code, C::InvalidField),
        ("identifier control char", bad_role, C::InvalidField),
        ("commitment on control", commitment, C::InvalidProvenance),
        ("relay format on store", relay_format, C::InvalidProvenance),
        ("subject", wrong_subject, C::InvalidSubject),
        ("resource id", wrong_resource, C::InvalidResource),
    ] {
        assert_eq!(
            validate_envelope(&value, Origin::Store)
                .expect_err(name)
                .code,
            code,
            "{name}"
        );
    }
}
