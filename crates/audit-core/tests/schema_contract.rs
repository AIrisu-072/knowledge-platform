use audit_core::{AuditEnvelope, LegacyAuditRow, schema_validate};
use serde_json::{Value, json};

fn created() -> Value {
    json!({"specversion":"1.0","id":"0198aa00-0000-7000-8000-000000000001",
      "source":"urn:knowledge-platform:document-platform","type":"document.created",
      "subject":"document/0198aa00-0000-7000-8000-000000000002","time":"2026-10-02T03:00:00Z",
      "datacontenttype":"application/json","dataschema":"urn:knowledge-platform:audit:event:1",
      "data":{"schema_version":1,"category":"CONTENT_LIFECYCLE","actor":{"identity_provider":"synthetic-idp","principal_id":"synthetic-author","kind":"unknown"},
        "action":"document.created","resource":{"type":"Document","id":"0198aa00-0000-7000-8000-000000000002"},"result":"success","correlation":{},
        "metadata":{"documentId":"0198aa00-0000-7000-8000-000000000002"},"provenance":{"source_format":"document-audit-outbox-v0","adapter_version":1}}})
}
#[test]
fn actual_draft2020_validator_enforces_structure_and_utf8_quota() {
    let valid = created();
    assert!(schema_validate(&valid).unwrap());
    for pointer in [
        "/id",
        "/source",
        "/specversion",
        "/time",
        "/data/resource/type",
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = json!("invalid");
        assert!(!schema_validate(&invalid).unwrap(), "{pointer}");
    }
    let mut oversized = valid;
    oversized["data"]["actor"]["principal_id"] = json!("界".repeat(171));
    assert!(!schema_validate(&oversized).unwrap());
}
#[test]
fn schema_and_runtime_accept_actual_legacy_publish_time_array() {
    let row = json!({"event_id":"0198aa00-0000-7000-8000-000000000001","event_type":"document.version.published","source":"urn:knowledge-platform:document-platform",
      "subject":"document/0198aa00-0000-7000-8000-000000000002","actor_identity_provider":"synthetic-idp","actor_principal_id":"synthetic-author","resource_type":"Document","resource_id":"0198aa00-0000-7000-8000-000000000002","resource_version_id":"0198aa00-0000-7000-8000-000000000003","result":"success","trace_id":null,"occurred_at":"2026-10-02T03:00:00Z",
      "data":{"publishOperationId":"0198aa00-0000-7000-8000-000000000004","expectedDocumentRevision":0,"resultingDocumentRevision":1,"result":"success","publishedAt":[2026,275,3,0,0,0,0,0,0]}});
    let event = AuditEnvelope::from_legacy(
        LegacyAuditRow::from_json(&serde_json::to_vec(&row).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(schema_validate(event.as_value()).unwrap());
    assert_eq!(
        event.as_value()["data"]["metadata"]["publishedAt"],
        row["data"]["publishedAt"]
    );
    let mut bad = event.as_value().clone();
    bad["data"]["metadata"]["publishedAt"][2] = json!(24);
    assert!(!schema_validate(&bad).unwrap());
    assert!(AuditEnvelope::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
}
