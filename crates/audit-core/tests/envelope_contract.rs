use audit_core::{AuditEnvelope, LegacyAuditRow, ValidationError, canonical_bytes, event_digest};
use serde_json::{Value, json};

fn legacy_value(event_type: &str, data: Value) -> Value {
    json!({
        "event_id": "0198aa00-0000-7000-8000-000000000001",
        "event_type": event_type,
        "source": "urn:knowledge-platform:document-platform",
        "subject": "document/0198aa00-0000-7000-8000-000000000002",
        "actor_identity_provider": "synthetic-idp",
        "actor_principal_id": "synthetic-author",
        "resource_type": "Document",
        "resource_id": "0198aa00-0000-7000-8000-000000000002",
        "resource_version_id": null,
        "result": "success",
        "trace_id": null,
        "data": data,
        "occurred_at": "2026-10-02T03:00:00Z"
    })
}
fn row(event_type: &str, data: Value) -> LegacyAuditRow {
    LegacyAuditRow::from_json(&serde_json::to_vec(&legacy_value(event_type, data)).unwrap())
        .unwrap()
}
fn created() -> AuditEnvelope {
    AuditEnvelope::from_legacy(row(
        "document.created",
        json!({
            "documentId":"0198aa00-0000-7000-8000-000000000002"
        }),
    ))
    .unwrap()
}

#[test]
fn legacy_create_preserves_identity_and_cloud_events_round_trip() {
    let event = created();
    let value: Value = serde_json::from_slice(&canonical_bytes(&event).unwrap()).unwrap();
    assert_eq!(value["specversion"], "1.0");
    assert_eq!(value["id"], "0198aa00-0000-7000-8000-000000000001");
    assert_eq!(value["type"], "document.created");
    assert_eq!(value["time"], "2026-10-02T03:00:00Z");
    assert_eq!(value["source"], "urn:knowledge-platform:document-platform");
    assert_eq!(value["data"]["schema_version"], 1);
    assert_eq!(value["data"]["actor"]["kind"], "unknown");
    assert_eq!(value["data"]["actor"]["principal_id"], "synthetic-author");
    assert!(value["data"]["correlation"].get("request_id").is_none());
    let reparsed = AuditEnvelope::from_json(&canonical_bytes(&event).unwrap()).unwrap();
    assert_eq!(
        event_digest(&event).unwrap(),
        event_digest(&reparsed).unwrap()
    );
}

#[test]
fn reason_required_evidence_cannot_be_silently_projected_away() {
    let result = AuditEnvelope::from_legacy(row(
        "document.version.withdrawn",
        json!({
            "documentId":"0198aa00-0000-7000-8000-000000000002",
            "reason":"synthetic required reason"
        }),
    ));
    assert_eq!(
        result.unwrap_err(),
        ValidationError::LegacyReasonContractUnqualified
    );
}

#[test]
fn unknown_and_sensitive_metadata_are_rejected_without_error_echo() {
    for key in [
        "body",
        "query",
        "storage_locator",
        "access_token",
        "acl",
        "prompt",
        "unknown",
    ] {
        let mut input = json!({"documentId":"0198aa00-0000-7000-8000-000000000002"});
        input[key] = json!("SYNTHETIC_PRIVATE_MARKER");
        let error = AuditEnvelope::from_legacy(row("document.created", input)).unwrap_err();
        assert_eq!(error, ValidationError::InvalidMetadata);
        assert!(!format!("{error:?} {error}").contains("SYNTHETIC_PRIVATE_MARKER"));
    }
}

#[test]
fn canonical_digest_ignores_object_key_order_but_not_evidence_changes() {
    let a = created();
    let mut value: Value = serde_json::from_slice(&canonical_bytes(&a).unwrap()).unwrap();
    let before = event_digest(&a).unwrap();
    value["data"]["actor"]["principal_id"] = json!("synthetic-other");
    let b = AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_ne!(before, event_digest(&b).unwrap());
    let mut object = serde_json::Map::new();
    for (key, value) in value.as_object().unwrap().iter().rev() {
        object.insert(key.clone(), value.clone());
    }
    let c = AuditEnvelope::from_json(&serde_json::to_vec(&object).unwrap()).unwrap();
    assert_eq!(event_digest(&b).unwrap(), event_digest(&c).unwrap());
}

#[test]
fn future_versions_and_invented_taxonomy_fail_closed() {
    let original: Value = serde_json::from_slice(&canonical_bytes(&created()).unwrap()).unwrap();
    for (path, replacement) in [
        ("specversion", json!("2.0")),
        ("type", json!("work_item.completed")),
        ("dataschema", json!("urn:knowledge-platform:audit:99")),
    ] {
        let mut value = original.clone();
        value[path] = replacement;
        assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut value = original;
    value["data"]["schema_version"] = json!(2);
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn byte_limits_apply_before_json_parse_and_include_utf8() {
    assert_eq!(
        AuditEnvelope::from_json(&vec![b' '; 32769]).unwrap_err(),
        ValidationError::EnvelopeTooLarge
    );
    assert_eq!(
        LegacyAuditRow::from_json(&vec![b' '; 24577]).unwrap_err(),
        ValidationError::LegacyRowTooLarge
    );
    let mut value: Value = serde_json::from_slice(&canonical_bytes(&created()).unwrap()).unwrap();
    value["data"]["actor"]["principal_id"] = json!("界".repeat(171));
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn resource_actor_and_envelope_are_closed_and_consistent() {
    let original: Value = serde_json::from_slice(&canonical_bytes(&created()).unwrap()).unwrap();
    for (pointer, bad) in [
        ("/id", json!("00000000-0000-0000-0000-000000000000")),
        ("/source", json!("https://untrusted.invalid")),
        ("/time", json!("not-a-time")),
        ("/data/resource/type", json!("Folder")),
        (
            "/data/resource/id",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
        ("/data/actor/kind", json!("assumed-human")),
        ("/data/result", json!("denied")),
        ("/data/action", json!("different.action")),
        (
            "/data/metadata/documentId",
            json!("0198aa00-0000-7000-8000-000000000099"),
        ),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(
            AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{pointer}"
        );
    }
    for target in [
        "",
        "/data",
        "/data/actor",
        "/data/resource",
        "/data/correlation",
    ] {
        let mut value = original.clone();
        value
            .pointer_mut(target)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("raw_query".into(), json!("synthetic"));
        assert!(
            AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{target}"
        );
    }
}

#[test]
fn actual_diff_arrays_and_legacy_correlation_are_preserved() {
    let mut input = legacy_value(
        "document.diff.result_access_granted",
        json!({
            "base_version_id":"0198aa00-0000-7000-8000-000000000003",
            "target_version_id":"0198aa00-0000-7000-8000-000000000004",
            "base_snapshot_digest":vec![0;32],"target_snapshot_digest":vec![1;32],"result_digest":vec![2;32],
            "comparison_profile":"document-diff-v0","resource_profile":"diff-resource-v0",
            "verdict":"different","coverage":"partial","cache_hit":true
        }),
    );
    input["resource_version_id"] = input["data"]["target_version_id"].clone();
    for legacy in [
        "diff-test",
        "00000000000000000000000000000000",
        "ABCDEFABCDEFABCDEFABCDEFABCDEFAB",
        "abc",
    ] {
        input["trace_id"] = json!(legacy);
        let event = AuditEnvelope::from_legacy(
            LegacyAuditRow::from_json(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            event.as_value()["data"]["correlation"]["legacy_correlation_id"],
            legacy
        );
        assert!(
            event.as_value()["data"]["correlation"]
                .get("trace_id")
                .is_none()
        );
        assert_eq!(event.as_value()["data"]["metadata"], input["data"]);
    }
    input["trace_id"] = json!("0123456789abcdef0123456789abcdef");
    let event = AuditEnvelope::from_legacy(
        LegacyAuditRow::from_json(&serde_json::to_vec(&input).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        event.as_value()["data"]["correlation"]["trace_id"],
        input["trace_id"]
    );
}

#[test]
fn exact_wire_and_multibyte_boundaries_are_accepted_without_truncation() {
    let mut bytes = canonical_bytes(&created()).unwrap();
    bytes.resize(32768, b' ');
    assert!(AuditEnvelope::from_json(&bytes).is_ok());
    bytes.push(b' ');
    assert_eq!(
        AuditEnvelope::from_json(&bytes).unwrap_err(),
        ValidationError::EnvelopeTooLarge
    );
    let mut value = created().as_value().clone();
    value["data"]["actor"]["principal_id"] = json!(format!("{}aa", "界".repeat(170)));
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_ok());
}

#[test]
fn legacy_provenance_cannot_claim_an_unattested_actor_kind() {
    let mut value = created().as_value().clone();
    value["data"]["actor"]["kind"] = json!("human_interactive");
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn subject_cannot_name_a_different_resource() {
    let mut value = created().as_value().clone();
    value["subject"] = json!("document/0198aa00-0000-7000-8000-000000000099");
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn legacy_provenance_cannot_invent_request_operation_or_double_trace_correlations() {
    let mut values = vec![created().as_value().clone(); 3];
    values[0]["data"]["correlation"]["request_id"] = json!("0198aa00-0000-7000-8000-000000000099");
    values[1]["data"]["correlation"]["operation_id"] =
        json!("0198aa00-0000-7000-8000-000000000099");
    values[2]["data"]["correlation"] =
        json!({"trace_id":"0123456789abcdef0123456789abcdef","legacy_correlation_id":"diff-test"});
    let accepted: Vec<_> = values
        .iter()
        .map(|v| AuditEnvelope::from_json(&serde_json::to_vec(v).unwrap()).is_ok())
        .collect();
    assert_eq!(accepted, vec![false, false, false]);
}
