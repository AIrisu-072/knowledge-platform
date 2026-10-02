use audit_core::{AuditEnvelope, LegacyAuditRow, ValidationError, schema_validate};
use serde_json::{Value, json};
const ROOT: &str = "00000000-0000-7000-8000-000000000001";

fn bootstrap() -> Value {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/audit-contract/conformance.json"
    ))
    .unwrap();
    let mut row = cases
        .iter()
        .find(|c| c["name"] == "access_policy.changed")
        .unwrap()["legacy"]
        .clone();
    row["resource_type"] = json!("Folder");
    row["resource_id"] = json!(ROOT);
    row["subject"] = json!(format!("folder/{ROOT}"));
    row["data"]["target_type"] = json!("Folder");
    row["data"]["target_id"] = json!(ROOT);
    row["data"]["bootstrap"] = json!(true);
    row["data"]["policy_revision"] = json!(1);
    row["data"]["access_revision"] = json!(1);
    row
}
fn adapt(row: &Value) -> Result<AuditEnvelope, ValidationError> {
    AuditEnvelope::from_legacy(
        LegacyAuditRow::from_json(&serde_json::to_vec(row).unwrap()).unwrap(),
    )
}

#[test]
fn normal_acl_reason_omission_cannot_be_misclassified_as_complete_evidence() {
    let mut outcomes = Vec::new();
    for resource_type in ["Document", "Folder"] {
        for flag in [None, Some(json!(false))] {
            let mut row = bootstrap();
            row["resource_type"] = json!(resource_type);
            row["subject"] = json!(format!("{}/{}", resource_type.to_ascii_lowercase(), ROOT));
            row["data"]["target_type"] = json!(resource_type);
            match flag {
                Some(v) => {
                    row["data"]["bootstrap"] = v;
                }
                None => {
                    row["data"].as_object_mut().unwrap().remove("bootstrap");
                }
            }
            outcomes.push(adapt(&row).err());
            let mut event = adapt(&bootstrap()).unwrap().as_value().clone();
            event["subject"] = row["subject"].clone();
            event["data"]["resource"]["type"] = row["resource_type"].clone();
            event["data"]["metadata"] = row["data"].clone();
            outcomes.push(AuditEnvelope::from_json(&serde_json::to_vec(&event).unwrap()).err());
        }
    }
    assert_eq!(
        outcomes,
        vec![Some(ValidationError::LegacyReasonContractUnqualified); 8]
    );
}

#[test]
fn only_the_actual_root_bootstrap_profile_is_schema_eligible() {
    let actual = adapt(&bootstrap()).unwrap();
    assert!(schema_validate(actual.as_value()).unwrap());
    assert_eq!(
        audit_core::event_digest(&actual).unwrap(),
        [
            226, 128, 124, 180, 147, 143, 246, 10, 185, 112, 62, 128, 138, 81, 156, 193, 135, 162,
            186, 100, 221, 115, 128, 0, 122, 236, 123, 116, 225, 240, 179, 186
        ]
    );
    let mut cases = Vec::new();
    let mut missing = actual.as_value().clone();
    missing["data"]["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("bootstrap");
    cases.push(missing);
    for (pointer, bad) in [
        ("/data/metadata/bootstrap", json!(false)),
        ("/data/metadata/target_type", json!("Document")),
        (
            "/data/metadata/target_id",
            json!("0198aa00-0000-7000-8000-000000000002"),
        ),
        ("/data/metadata/policy_id", Value::Null),
        ("/data/metadata/policy_revision", json!(2)),
        ("/data/metadata/access_revision", json!(0)),
    ] {
        let mut value = actual.as_value().clone();
        *value.pointer_mut(pointer).unwrap() = bad;
        cases.push(value);
    }
    let outcomes: Vec<bool> = cases
        .iter()
        .map(|value| schema_validate(value).unwrap())
        .collect();
    assert_eq!(outcomes, vec![false; 7]);
    for value in cases {
        assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut fake_document = bootstrap();
    fake_document["resource_type"] = json!("Document");
    fake_document["data"]["target_type"] = json!("Document");
    fake_document["subject"] = json!(format!("document/{ROOT}"));
    assert!(adapt(&fake_document).is_err());
}

#[test]
fn all_other_reason_required_management_variants_stay_deferred_even_if_text_is_missing() {
    for kind in [
        "document.metadata.changed",
        "document.moved",
        "folder.created",
        "folder.renamed",
        "folder.moved",
    ] {
        let mut row = bootstrap();
        row["event_type"] = json!(kind);
        row["data"] = json!({});
        assert_eq!(
            adapt(&row).unwrap_err(),
            ValidationError::LegacyReasonContractUnqualified
        );
        row["data"]["reason"] = json!("SYNTHETIC_PRIVATE_MARKER");
        assert_eq!(
            adapt(&row).unwrap_err(),
            ValidationError::LegacyReasonContractUnqualified
        );
    }
}
