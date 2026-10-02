use audit_core::{AuditEnvelope, LegacyAuditRow, canonical_bytes, schema_validate};
use serde_json::Value;

#[test]
fn every_catalog_variant_and_derived_negative_has_the_expected_disposition() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/audit-contract/conformance.json"
    ))
    .unwrap();
    assert!(fixtures.len() >= 60);
    for case in fixtures {
        let row = LegacyAuditRow::from_json(&serde_json::to_vec(&case["legacy"]).unwrap()).unwrap();
        let result = AuditEnvelope::from_legacy(row);
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "case {}: {:?}",
            case["name"],
            result.as_ref().err()
        );
        if let Ok(event) = result {
            assert!(
                schema_validate(event.as_value()).unwrap(),
                "{}",
                case["name"]
            );
            let bytes = canonical_bytes(&event).unwrap();
            assert!(AuditEnvelope::from_json(&bytes).is_ok(), "{}", case["name"]);
        }
    }
}
#[test]
fn quarantinable_legacy_debug_never_echoes_payload() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/audit-contract/conformance.json"
    ))
    .unwrap();
    let mut raw = fixtures[0]["legacy"].clone();
    raw["data"]["reason"] = serde_json::json!("SYNTHETIC_PRIVATE_MARKER");
    let row = LegacyAuditRow::from_json(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(!format!("{row:?}").contains("SYNTHETIC_PRIVATE_MARKER"));
}

#[test]
fn cancellation_preserves_publish_intent_reference_without_inventing_cancel_operation_id() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/audit-contract/conformance.json"
    ))
    .unwrap();
    let row = &fixtures
        .iter()
        .find(|f| f["name"] == "document.version.publication.cancelled")
        .unwrap()["legacy"];
    let event = AuditEnvelope::from_legacy(
        LegacyAuditRow::from_json(&serde_json::to_vec(row).unwrap()).unwrap(),
    )
    .unwrap();
    let correlation = &event.as_value()["data"]["correlation"];
    assert_eq!(
        correlation["publish_operation_id"],
        row["data"]["publishOperationId"]
    );
    assert!(correlation.get("operation_id").is_none());
}
