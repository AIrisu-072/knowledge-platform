use audit_core::{AuditEnvelope, LegacyAuditRow, ValidationError, canonical_bytes};
use serde_json::{Value, json};

fn legacy(kind: &str) -> Value {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/audit-contract/conformance.json"
    ))
    .unwrap();
    cases.iter().find(|case| case["name"] == kind).unwrap()["legacy"].clone()
}
fn adapt(row: &Value) -> AuditEnvelope {
    AuditEnvelope::from_legacy(
        LegacyAuditRow::from_json(&serde_json::to_vec(row).unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn access_policy_bootstrap_target_must_match_the_actual_root_folder_resource() {
    let original = legacy("access_policy.changed");
    let envelope = adapt(&original);
    let mut rejected = Vec::new();
    for (key, bad) in [
        ("target_id", json!("0198aa00-0000-7000-8000-000000000099")),
        ("target_type", json!("Document")),
    ] {
        let mut raw = original.clone();
        raw["data"][key] = bad.clone();
        rejected.push(
            AuditEnvelope::from_legacy(
                LegacyAuditRow::from_json(&serde_json::to_vec(&raw).unwrap()).unwrap(),
            )
            .is_err(),
        );
        let mut event = envelope.as_value().clone();
        event["data"]["metadata"][key] = bad;
        rejected.push(AuditEnvelope::from_json(&serde_json::to_vec(&event).unwrap()).is_err());
    }
    for resource_type in ["Document", "AccessPolicy"] {
        let mut raw = original.clone();
        raw["resource_type"] = json!(resource_type);
        raw["data"]["target_type"] = json!(resource_type);
        raw["subject"] = json!(format!(
            "{}/{}",
            resource_type.to_ascii_lowercase(),
            raw["resource_id"].as_str().unwrap()
        ));
        rejected.push(
            AuditEnvelope::from_legacy(
                LegacyAuditRow::from_json(&serde_json::to_vec(&raw).unwrap()).unwrap(),
            )
            .is_err(),
        );
        let mut event = envelope.as_value().clone();
        event["data"]["resource"]["type"] = json!(resource_type);
        event["data"]["metadata"]["target_type"] = json!(resource_type);
        event["subject"] = raw["subject"].clone();
        rejected.push(AuditEnvelope::from_json(&serde_json::to_vec(&event).unwrap()).is_err());
    }
    let mut other = original;
    other["resource_id"] = json!("0198aa00-0000-7000-8000-000000000099");
    other["data"]["target_id"] = other["resource_id"].clone();
    other["subject"] = json!(format!("folder/{}", other["resource_id"].as_str().unwrap()));
    rejected.push(
        AuditEnvelope::from_legacy(
            LegacyAuditRow::from_json(&serde_json::to_vec(&other).unwrap()).unwrap(),
        )
        .is_err(),
    );
    assert_eq!(rejected, vec![true; 9]);
}

#[test]
fn raw_envelopes_reject_duplicate_members_before_canonicalization() {
    let event = adapt(&legacy("document.created"));
    let clean = String::from_utf8(canonical_bytes(&event).unwrap()).unwrap();
    let metadata = serde_json::to_string(&event.as_value()["data"]["metadata"]).unwrap();
    let needle = format!("\"metadata\":{metadata}");
    let cases = [
        format!("\"metadata\":{{\"reason\":\"SYNTHETIC_PRIVATE_MARKER\"}},{needle}"),
        format!("{needle},\"metadata\":{{\"reason\":\"SYNTHETIC_PRIVATE_MARKER\"}}"),
        format!("{needle},{needle}"),
        format!("\"metad\\u0061ta\":{{\"reason\":\"SYNTHETIC_PRIVATE_MARKER\"}},{needle}"),
    ];
    let results: Vec<_> = cases
        .iter()
        .map(|replacement| {
            AuditEnvelope::from_json(clean.replace(&needle, replacement).as_bytes()).err()
        })
        .collect();
    assert_eq!(results, vec![Some(ValidationError::InvalidEnvelope); 4]);
    assert!(AuditEnvelope::from_json(clean.as_bytes()).is_ok());
}

#[test]
fn legacy_nested_duplicates_cannot_hide_evidence() {
    let row = legacy("document.created");
    let clean = serde_json::to_string(&row).unwrap();
    let id = row["data"]["documentId"].as_str().unwrap();
    let needle = format!("\"documentId\":\"{id}\"");
    let cases = [
        format!("\"documentId\":\"SYNTHETIC_PRIVATE_MARKER\",{needle}"),
        format!("{needle},\"documentId\":\"SYNTHETIC_PRIVATE_MARKER\""),
        format!("{needle},{needle}"),
        format!("\"document\\u0049d\":\"SYNTHETIC_PRIVATE_MARKER\",{needle}"),
    ];
    let results: Vec<_> = cases
        .iter()
        .map(|replacement| {
            LegacyAuditRow::from_json(clean.replace(&needle, replacement).as_bytes()).err()
        })
        .collect();
    assert_eq!(results, vec![Some(ValidationError::InvalidEnvelope); 4]);
    assert!(LegacyAuditRow::from_json(clean.as_bytes()).is_ok());
}

#[test]
fn legacy_time_offsets_must_be_lossless_without_sign_normalization() {
    let original = adapt(&legacy("document.version.published"));
    let mut outcomes = Vec::new();
    for (offset, expected) in [
        ([0, 0, 0], true),
        ([1, 30, 0], true),
        ([-1, -30, 0], true),
        ([0, -30, -15], true),
        ([1, -30, 0], false),
        ([-1, 30, 0], false),
        ([0, 30, -15], false),
    ] {
        let mut event = original.as_value().clone();
        let tuple = json!([2026, 275, 3, 0, 0, 0, offset[0], offset[1], offset[2]]);
        event["data"]["metadata"]["publishedAt"] = tuple.clone();
        let runtime = AuditEnvelope::from_json(&serde_json::to_vec(&event).unwrap());
        let structural = audit_core::schema_validate(&event).unwrap();
        if let Ok(accepted) = &runtime {
            assert_eq!(
                accepted.as_value()["data"]["metadata"]["publishedAt"],
                tuple
            );
        }
        outcomes.push((runtime.is_ok(), structural));
        if expected {
            assert_eq!(outcomes.last(), Some(&(true, true)));
        }
    }
    assert_eq!(
        outcomes,
        vec![
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (false, false),
            (false, false),
            (false, false)
        ]
    );
}
