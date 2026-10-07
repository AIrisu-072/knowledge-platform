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
