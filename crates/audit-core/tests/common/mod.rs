//! Synthetic fixtures mirroring every Document audit producer shape on main.
//! All identifiers and principals are synthetic.
#![allow(dead_code)]

use std::collections::BTreeMap;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::chain::to_hex;
use audit_core::kinds::is_hex_digest;
use audit_core::{DocumentStagingProjection, Origin, envelope_digest, project};
use serde_json::{Map, Value, json};

pub const EVENT_ID: &str = "0199a1b2-0000-7000-8000-00000000e001";
pub const DOC: &str = "0199a1b2-0000-7000-8000-00000000d001";
pub const OTHER_DOC: &str = "0199a1b2-0000-7000-8000-00000000d002";
pub const VER: &str = "0199a1b2-0000-7000-8000-00000000a001";
pub const BASE_VER: &str = "0199a1b2-0000-7000-8000-00000000a000";
pub const FOLDER: &str = "0199a1b2-0000-7000-8000-00000000f001";
pub const PARENT: &str = "0199a1b2-0000-7000-8000-00000000f000";
pub const OTHER_FOLDER: &str = "0199a1b2-0000-7000-8000-00000000f002";
pub const ROOT_FOLDER: &str = "00000000-0000-7000-8000-000000000001";
pub const OP: &str = "0199a1b2-0000-7000-8000-00000000c001";
pub const PUBLISH_OP: &str = "0199a1b2-0000-7000-8000-00000000b001";
pub const POLICY: &str = "0199a1b2-0000-7000-8000-00000000ac01";
pub const ITEM: &str = "0199a1b2-0000-7000-8000-000000001c01";
pub const REP: &str = "0199a1b2-0000-7000-8000-000000002e01";
pub const CORR: &str = "0199a1b2-0000-7000-8000-00000000cc01";
pub const REV_BASE: &str = "0199a1b2-0000-7000-8000-00000000ee00";
pub const REV_TARGET: &str = "0199a1b2-0000-7000-8000-00000000ee01";
pub const NIL: &str = "00000000-0000-0000-0000-000000000000";
pub const OCCURRED: &str = "2026-10-07T01:02:03.456789Z";
pub const ISSUER: &str = "poc";
pub const PRINCIPAL: &str = "poc-human";
/// The `session_user` recorded by synthetic control events.
pub const SESSION_ROLE: &str = "audit_store_reader";

pub fn commitment() -> String {
    "ab".repeat(32)
}

pub fn legacy_time() -> Value {
    json!([2026, 280, 1, 2, 3, 456_789_000, 0, 0, 0])
}

pub fn bytes(byte: u8) -> Value {
    Value::Array(vec![json!(byte); 32])
}

pub fn scheduler() -> Value {
    json!({"identityProvider": "service", "principalId": "scheduler"})
}

pub fn row_actor() -> Value {
    json!({"identityProvider": ISSUER, "principalId": PRINCIPAL})
}

pub fn version_subject() -> String {
    format!("document/{DOC}/version/{VER}")
}

pub fn doc_subject() -> String {
    format!("document/{DOC}")
}

pub fn row(
    event_type: &str,
    subject: &str,
    resource: (&str, &str, Option<&str>),
    result: &str,
    data: Value,
) -> DocumentStagingProjection {
    DocumentStagingProjection {
        event_id: EVENT_ID.to_owned(),
        event_type: event_type.to_owned(),
        source: DOCUMENT_SOURCE.to_owned(),
        subject: subject.to_owned(),
        actor_identity_provider: ISSUER.to_owned(),
        actor_principal_id: PRINCIPAL.to_owned(),
        resource_type: resource.0.to_owned(),
        resource_id: resource.1.to_owned(),
        resource_version_id: resource.2.map(str::to_owned),
        result: result.to_owned(),
        trace_id: None,
        occurred_at: OCCURRED.to_owned(),
        oversize: false,
        data: Some(data),
        data_kind: "object".to_owned(),
        reason_kind: None,
        reason_bytes: None,
        source_intact: true,
        source_commitment: commitment(),
        registration_kind: "trigger".to_owned(),
    }
}

fn reason(mut row: DocumentStagingProjection, bytes: i64) -> DocumentStagingProjection {
    row.reason_kind = Some("string".to_owned());
    row.reason_bytes = Some(bytes);
    row
}

fn traced(mut row: DocumentStagingProjection, trace: &str) -> DocumentStagingProjection {
    row.trace_id = Some(trace.to_owned());
    row
}

/// Expected projection results, written out explicitly per fixture.
#[derive(Clone, Default)]
pub struct Expect {
    /// Source data keys that must not appear in `details`.
    pub removed: &'static [&'static str],
    pub correlation: Value,
    pub reason_bytes: Option<i64>,
    pub service_executor: Option<Value>,
    pub reason_code: Option<&'static str>,
}

pub struct Fixture {
    pub name: &'static str,
    pub row: DocumentStagingProjection,
    pub expect: Expect,
}

fn fixture(name: &'static str, row: DocumentStagingProjection, expect: Expect) -> Fixture {
    Fixture { name, row, expect }
}

fn none() -> Expect {
    Expect {
        correlation: json!({}),
        ..Expect::default()
    }
}

fn op(operation: &str) -> Expect {
    Expect {
        correlation: json!({"operation_id": operation}),
        ..Expect::default()
    }
}

fn op_reason(operation: &str, bytes: i64) -> Expect {
    Expect {
        reason_bytes: Some(bytes),
        ..op(operation)
    }
}

fn publish_ops() -> Value {
    json!({"operation_id": PUBLISH_OP, "publish_operation_id": PUBLISH_OP})
}

const DOCUMENT: &str = "Document";
const FOLDER_TYPE: &str = "Folder";

/// One accepted claim row per producer variant on main.
pub fn accepted_fixtures() -> Vec<Fixture> {
    let version = (DOCUMENT, DOC, Some(VER));
    let unversioned = (DOCUMENT, DOC, None);
    let folder = (FOLDER_TYPE, FOLDER, None);
    let vs = version_subject();
    let ds = doc_subject();
    let fs = format!("folder/{FOLDER}");
    vec![
        fixture(
            "document.created",
            row(
                "document.created",
                &ds,
                unversioned,
                "success",
                json!({"documentId": DOC}),
            ),
            none(),
        ),
        fixture(
            "document.version.created/initial",
            row(
                "document.version.created",
                &ds,
                version,
                "success",
                json!({"documentId": DOC, "documentVersionId": VER}),
            ),
            none(),
        ),
        fixture(
            "document.version.created/later",
            row(
                "document.version.created",
                &vs,
                version,
                "success",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "versionNo": 2,
                    "baseDocumentVersionId": BASE_VER, "resultingDocumentRevision": 5
                }),
            ),
            none(),
        ),
        fixture(
            "document.version.updated/null_base",
            row(
                "document.version.updated",
                &vs,
                version,
                "success",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "versionNo": 1,
                    "baseDocumentVersionId": null, "resultingDocumentRevision": 1
                }),
            ),
            none(),
        ),
        fixture(
            "document.version.rebased",
            row(
                "document.version.rebased",
                &vs,
                version,
                "success",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "versionNo": 3,
                    "baseDocumentVersionId": BASE_VER, "resultingDocumentRevision": 12
                }),
            ),
            none(),
        ),
        fixture(
            "document.version.published/manual",
            row(
                "document.version.published",
                &vs,
                version,
                "success",
                json!({
                    "publishOperationId": PUBLISH_OP, "expectedDocumentRevision": 3,
                    "resultingDocumentRevision": 4, "result": "success", "publishedAt": legacy_time()
                }),
            ),
            Expect {
                correlation: publish_ops(),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.published/scheduled",
            row(
                "document.version.published",
                &vs,
                version,
                "success",
                json!({
                    "publishOperationId": PUBLISH_OP, "expectedDocumentRevision": 3,
                    "resultingDocumentRevision": 4, "result": "success",
                    "publishedAt": legacy_time(), "serviceExecutor": scheduler()
                }),
            ),
            Expect {
                removed: &["serviceExecutor"],
                correlation: publish_ops(),
                service_executor: Some(json!({"issuer": "service", "principal_id": "scheduler"})),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.publication.scheduled",
            row(
                "document.version.publication.scheduled",
                &vs,
                version,
                "success",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "publishOperationId": PUBLISH_OP,
                    "scheduledPublishAt": [2026, 281, 9, 0, 0, 0, 9, 0, 0],
                    "acceptedDocumentRevision": 6
                }),
            ),
            Expect {
                correlation: publish_ops(),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.publication.cancelled",
            row(
                "document.version.publication.cancelled",
                &vs,
                version,
                "success",
                json!({
                    "documentId": DOC, "documentVersionId": VER,
                    "publishOperationId": PUBLISH_OP, "resultingDocumentRevision": 7
                }),
            ),
            Expect {
                correlation: json!({"publish_operation_id": PUBLISH_OP}),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.publication.terminal/scheduler",
            row(
                "document.version.publication.terminal",
                &vs,
                version,
                "failure",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "publishOperationId": PUBLISH_OP,
                    "terminalReason": "authorization_revoked", "resultingDocumentRevision": 8,
                    "serviceExecutor": scheduler()
                }),
            ),
            Expect {
                removed: &["serviceExecutor"],
                correlation: publish_ops(),
                service_executor: Some(json!({"issuer": "service", "principal_id": "scheduler"})),
                reason_code: Some("authorization_revoked"),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.publication.terminal/no_executor",
            row(
                "document.version.publication.terminal",
                &vs,
                version,
                "failure",
                json!({
                    "documentId": DOC, "documentVersionId": VER, "publishOperationId": PUBLISH_OP,
                    "terminalReason": "publish_quality_rejected", "resultingDocumentRevision": 8
                }),
            ),
            Expect {
                correlation: publish_ops(),
                reason_code: Some("publish_quality_rejected"),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.withdrawn/withheld",
            reason(
                row(
                    "document.version.withdrawn",
                    &vs,
                    version,
                    "success",
                    json!({
                        "documentId": DOC, "withdrawnDocumentVersionId": VER,
                        "formerCurrentVersionId": VER, "resultingCurrentVersionId": null,
                        "actor": row_actor(), "restorationWithheldByValidation": true,
                        "restorationWithheldReason": "base_inspection_unavailable",
                        "invalidatedScheduleCount": 1
                    }),
                ),
                42,
            ),
            Expect {
                removed: &["actor"],
                correlation: json!({}),
                reason_bytes: Some(42),
                ..Expect::default()
            },
        ),
        fixture(
            "document.version.withdrawn/restored",
            reason(
                row(
                    "document.version.withdrawn",
                    &vs,
                    version,
                    "success",
                    json!({
                        "documentId": DOC, "withdrawnDocumentVersionId": VER,
                        "formerCurrentVersionId": VER, "resultingCurrentVersionId": BASE_VER,
                        "actor": row_actor(), "restorationWithheldByValidation": false,
                        "restorationWithheldReason": null, "invalidatedScheduleCount": 0
                    }),
                ),
                7,
            ),
            Expect {
                removed: &["actor"],
                correlation: json!({}),
                reason_bytes: Some(7),
                ..Expect::default()
            },
        ),
        fixture(
            "document.publication.ended",
            reason(
                row(
                    "document.publication.ended",
                    &ds,
                    version,
                    "success",
                    json!({
                        "operationId": OP, "documentId": DOC, "formerCurrentVersionId": VER,
                        "resultingCurrentVersionId": null, "resultingDocumentRevision": 9,
                        "actor": row_actor(), "invalidatedScheduleCount": 2, "endedAt": legacy_time()
                    }),
                ),
                16,
            ),
            Expect {
                removed: &["actor"],
                ..op_reason(OP, 16)
            },
        ),
        fixture(
            "document.metadata.changed",
            reason(
                row(
                    "document.metadata.changed",
                    &ds,
                    unversioned,
                    "success",
                    json!({
                        "operation_id": OP, "document_id": DOC,
                        "changed_keys": ["category", "extensions"], "document_revision": 10
                    }),
                ),
                12,
            ),
            op_reason(OP, 12),
        ),
        fixture(
            "document.moved",
            reason(
                row(
                    "document.moved",
                    &ds,
                    unversioned,
                    "success",
                    json!({
                        "operation_id": OP, "document_id": DOC, "from_folder_id": FOLDER,
                        "to_folder_id": OTHER_FOLDER, "document_revision": 11,
                        "access_revision": 3, "visibility_changed": true
                    }),
                ),
                5,
            ),
            op_reason(OP, 5),
        ),
        fixture(
            "folder.created",
            reason(
                row(
                    "folder.created",
                    &fs,
                    folder,
                    "success",
                    json!({
                        "folder_id": FOLDER, "parent_folder_id": PARENT,
                        "folder_revision": 0, "operation_id": OP
                    }),
                ),
                9,
            ),
            op_reason(OP, 9),
        ),
        fixture(
            "folder.renamed",
            reason(
                row(
                    "folder.renamed",
                    &fs,
                    folder,
                    "success",
                    json!({"folder_id": FOLDER, "folder_revision": 1, "operation_id": OP}),
                ),
                3,
            ),
            op_reason(OP, 3),
        ),
        fixture(
            "folder.moved",
            reason(
                row(
                    "folder.moved",
                    &fs,
                    folder,
                    "success",
                    json!({
                        "operation_id": OP, "folder_id": FOLDER, "from_parent_id": PARENT,
                        "to_parent_id": OTHER_FOLDER, "folder_revision": 2,
                        "access_revision": 4, "subtree_affected": 3
                    }),
                ),
                4,
            ),
            op_reason(OP, 4),
        ),
        fixture(
            "access_policy.changed/document",
            row(
                "access_policy.changed",
                &ds,
                unversioned,
                "success",
                json!({
                    "operation_id": OP, "target_type": "Document", "target_id": DOC,
                    "policy_id": POLICY, "policy_revision": 2, "access_revision": 5
                }),
            ),
            op(OP),
        ),
        fixture(
            "access_policy.changed/folder_inherit",
            row(
                "access_policy.changed",
                &fs,
                folder,
                "success",
                json!({
                    "operation_id": OP, "target_type": "Folder", "target_id": FOLDER,
                    "policy_id": null, "policy_revision": 3, "access_revision": 6
                }),
            ),
            op(OP),
        ),
        fixture("access_policy.changed/bootstrap", bootstrap_row(), op(OP)),
        fixture(
            "document.version.read_confirmed",
            row(
                "document.version.read_confirmed",
                &ds,
                version,
                "success",
                json!({"document_version_id": VER}),
            ),
            none(),
        ),
        fixture(
            "document.file.access_granted/diff_display",
            traced(
                row(
                    "document.file.access_granted",
                    &format!("document/{DOC}/version/{VER}/representation/{REP}"),
                    version,
                    "success",
                    json!({"content_item_id": ITEM, "representation_id": REP, "purpose": "history"}),
                ),
                CORR,
            ),
            Expect {
                correlation: json!({"source_correlation_id": CORR}),
                ..Expect::default()
            },
        ),
        fixture(
            "document.file.access_granted/download",
            row(
                "document.file.access_granted",
                &format!("document/{DOC}/version/{VER}/representation/{REP}"),
                version,
                "success",
                json!({"content_item_id": ITEM, "representation_id": REP, "purpose": "published"}),
            ),
            none(),
        ),
        fixture(
            "document.diff.result_access_granted/compare",
            row(
                "document.diff.result_access_granted",
                &format!("document/{DOC}/diff"),
                version,
                "success",
                diff_data("different", "partial", false),
            ),
            none(),
        ),
        fixture(
            "document.diff.result_access_granted/display",
            traced(
                row(
                    "document.diff.result_access_granted",
                    &format!("document/{DOC}/diff"),
                    version,
                    "success",
                    diff_data("same", "full", true),
                ),
                CORR,
            ),
            Expect {
                correlation: json!({"source_correlation_id": CORR}),
                ..Expect::default()
            },
        ),
        fixture(
            "document.revision_comparison.result_access_granted/same_version",
            row(
                "document.revision_comparison.result_access_granted",
                &format!("document/{DOC}/revision-comparison"),
                version,
                "success",
                json!({
                    "base_revision_id": REV_BASE, "target_revision_id": REV_TARGET,
                    "content_comparison_status": "sameAuthoritativeVersion",
                    "content_result_digest": null, "content_audit_event_id": null,
                    "metadata_comparison_status": "unavailableLegacy",
                    "base_metadata_snapshot_digest": null, "target_metadata_snapshot_digest": null
                }),
            ),
            none(),
        ),
        fixture(
            "document.revision_comparison.result_access_granted/different",
            traced(
                row(
                    "document.revision_comparison.result_access_granted",
                    &format!("document/{DOC}/revision-comparison"),
                    version,
                    "success",
                    json!({
                        "base_revision_id": REV_BASE, "target_revision_id": REV_TARGET,
                        "content_comparison_status": "differentAuthoritativeVersions",
                        "content_result_digest": bytes(9), "content_audit_event_id": CORR,
                        "metadata_comparison_status": "different",
                        "base_metadata_snapshot_digest": bytes(4),
                        "target_metadata_snapshot_digest": bytes(5)
                    }),
                ),
                CORR,
            ),
            Expect {
                correlation: json!({"source_correlation_id": CORR}),
                ..Expect::default()
            },
        ),
        fixture(
            "authorization.denied/management",
            denied_row("set_access_policy"),
            Expect {
                correlation: json!({}),
                reason_code: Some("forbidden"),
                ..Expect::default()
            },
        ),
        fixture(
            "authorization.denied/read_state",
            denied_row("mark_version_read"),
            Expect {
                correlation: json!({}),
                reason_code: Some("forbidden"),
                ..Expect::default()
            },
        ),
    ]
}

fn bootstrap_row() -> DocumentStagingProjection {
    let mut bootstrap = row(
        "access_policy.changed",
        &format!("folder/{ROOT_FOLDER}"),
        (FOLDER_TYPE, ROOT_FOLDER, None),
        "success",
        json!({
            "operation_id": OP, "target_type": "Folder", "target_id": ROOT_FOLDER,
            "policy_id": POLICY, "policy_revision": 1, "access_revision": 1, "bootstrap": true
        }),
    );
    bootstrap.actor_principal_id = "bootstrap-admin".to_owned();
    bootstrap
}

fn denied_row(action: &str) -> DocumentStagingProjection {
    row(
        "authorization.denied",
        "authorization/denied",
        ("AccessPolicy", NIL, None),
        "denied",
        json!({"action_code": action, "reason_code": "forbidden"}),
    )
}

fn diff_data(verdict: &str, coverage: &str, cache_hit: bool) -> Value {
    json!({
        "base_version_id": BASE_VER, "target_version_id": VER,
        "base_snapshot_digest": bytes(1), "target_snapshot_digest": bytes(2),
        "comparison_profile": "document-diff-v0", "resource_profile": "diff-resource-v0",
        "result_digest": bytes(3), "verdict": verdict, "coverage": coverage, "cache_hit": cache_hit
    })
}

pub fn fixture_named(name: &str) -> Fixture {
    accepted_fixtures()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// The projected envelope of a named fixture, as a JSON value.
pub fn envelope_of(name: &str) -> Value {
    project(&fixture_named(name).row)
        .unwrap_or_else(|r| panic!("{name} must project: {r}"))
        .into_value()
}

/// A sample value of a catalog kind (control fixtures).
pub fn kind_sample(field: &audit_core::catalog::FieldSpec) -> Value {
    use audit_core::kinds::Kind;
    match field.kind {
        Kind::Uuid | Kind::NullableUuid => json!(EVENT_ID),
        Kind::Counter
        | Kind::NullableCounter
        | Kind::PositiveCounter
        | Kind::NullablePositiveCounter => json!(7),
        Kind::Boolean => json!(true),
        Kind::Enum | Kind::NullableEnum => json!(field.values[0]),
        Kind::EnumList => json!([field.values[0]]),
        Kind::Digest | Kind::NullableDigest => bytes(1),
        Kind::Principal => row_actor(),
        Kind::LegacyTime => legacy_time(),
        Kind::UtcTimestamp => json!(OCCURRED),
        Kind::UuidList => json!([EVENT_ID, DOC]),
        Kind::HexDigest | Kind::NullableHexDigest => json!(commitment()),
        Kind::NullableUtcTimestamp => json!(OCCURRED),
        Kind::ResourceRef => json!(DOC),
        Kind::EventType => json!("document.created"),
        Kind::EventTypeList => json!(["document.created", "folder.moved"]),
        Kind::SourceUrn => json!(DOCUMENT_SOURCE),
        Kind::SourceList => json!([DOCUMENT_SOURCE]),
        Kind::DbRole | Kind::NullableDbRole => json!(SESSION_ROLE),
        Kind::PrincipalRef => json!(PRINCIPAL),
        Kind::Int8Text => json!("7301234567890123456"),
        Kind::Code => json!("delivery_unknown_at_limit"),
    }
}

/// A synthetic control envelope as the Store's SQL builds it. `full` also
/// populates every optional field.
pub fn control_envelope(spec: &audit_core::EventSpec, full: bool) -> Value {
    let details: serde_json::Map<String, Value> = spec
        .detail_fields()
        .filter(|(name, _)| full || spec.required.iter().any(|r| r == name))
        .map(|(name, field)| (name.to_owned(), kind_sample(field)))
        .collect();
    json!({
        "specversion": "1.0",
        "id": "0199a1b2-0000-7000-8000-00000000c0c1",
        "source": spec.source,
        "type": spec.event_type,
        "subject": "audit-store",
        "time": OCCURRED,
        "datacontenttype": "application/json",
        "dataschema": "urn:knowledge-platform:audit:payload:v1",
        "data": {
            "schema_version": 1,
            "event_class": spec.event_class.as_str(),
            "action": spec.event_type,
            "actor": {"issuer": "synthetic-idp", "principal_id": "synthetic-operator"},
            "resource": {"type": "AuditStore", "id": "audit-store"},
            "result": spec.results[0],
            "correlation": {},
            "details": details,
            "extensions": {},
            "provenance": {
                "source_format": adapter_of(spec).source_format,
                "adapter_version": adapter_of(spec).adapter_version
            }
        }
    })
}

/// The catalog adapter of an entry's source.
pub fn adapter_of(spec: &audit_core::EventSpec) -> &'static audit_core::AdapterSpec {
    audit_core::Catalog::embedded()
        .adapter(&spec.source)
        .expect("every source has an adapter")
}

/// Every control entry, minimal and fully populated.
pub fn control_envelopes() -> Vec<(String, Origin, Value)> {
    audit_core::Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin != Origin::Relay)
        .flat_map(|spec| {
            [false, true].into_iter().map(move |full| {
                (
                    format!(
                        "{}/{}",
                        spec.event_type,
                        if full { "full" } else { "minimal" }
                    ),
                    spec.origin,
                    control_envelope(spec, full),
                )
            })
        })
        .collect()
}

/// Input to envelope-level validation.
pub enum Input {
    Value(Value),
    Text(String),
}

/// An envelope the Rust validator must reject. `rust_only` names the
/// category when the generated JSON Schema cannot express the constraint and
/// therefore accepts the instance.
pub struct EnvelopeCase {
    pub name: &'static str,
    pub input: Input,
    pub path: Origin,
    pub expected: audit_core::Rejection,
    pub rust_only: Option<&'static str>,
}

/// `Rejection::at(code, field)`.
pub fn at(code: audit_core::RejectionCode, field: &'static str) -> audit_core::Rejection {
    audit_core::Rejection::at(code, field)
}

/// `Rejection::new(code)` (no location).
pub fn bare(code: audit_core::RejectionCode) -> audit_core::Rejection {
    audit_core::Rejection::new(code)
}

fn set(mut value: Value, pointer: &str, new: Value) -> Value {
    let (parent, key) = pointer.rsplit_once('/').expect("pointer");
    let target = value.pointer_mut(parent).expect("parent exists");
    match target {
        Value::Object(map) => {
            map.insert(key.to_owned(), new);
        }
        Value::Array(items) => {
            items[key.parse::<usize>().expect("index")] = new;
        }
        _ => panic!("not a container"),
    }
    value
}

fn remove(mut value: Value, pointer: &str) -> Value {
    let (parent, key) = pointer.rsplit_once('/').expect("pointer");
    value
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .expect("parent object")
        .remove(key)
        .expect("key existed");
    value
}

pub fn envelope_rejections() -> Vec<EnvelopeCase> {
    use audit_core::RejectionCode as C;
    let created = || envelope_of("document.version.created/later");
    let published = || envelope_of("document.version.published/scheduled");
    let terminal = || envelope_of("document.version.publication.terminal/scheduler");
    let withdrawn = || envelope_of("document.version.withdrawn/withheld");
    let acl_folder = || envelope_of("access_policy.changed/folder_inherit");
    let metadata = || envelope_of("document.metadata.changed");
    let diff = || envelope_of("document.diff.result_access_granted/compare");
    let doc_created = || envelope_of("document.created");
    let folder_created = || envelope_of("folder.created");
    let case = |name, value: Value, expected, rust_only| EnvelopeCase {
        name,
        input: Input::Value(value),
        path: Origin::Relay,
        expected,
        rust_only,
    };
    let text_case = |name, text: String, expected, rust_only| EnvelopeCase {
        name,
        input: Input::Text(text),
        path: Origin::Relay,
        expected,
        rust_only,
    };
    let compact = |value: &Value| serde_json::to_string(value).expect("serialize");
    let details = "data.details";
    let actor = "data.actor";
    let executor = "data.service_executor";
    let resource = "data.resource";
    let correlation = "data.correlation";
    let provenance = "data.provenance";
    let charset = Some("principal_charset");
    let mut cases = vec![
        case(
            "free_text_note",
            set(created(), "/data/details/note", json!("x")),
            at(C::UnknownField, details),
            None,
        ),
        case(
            "free_text_body",
            set(created(), "/data/details/body", json!("x")),
            at(C::UnknownField, details),
            None,
        ),
        case(
            "free_text_query",
            set(diff(), "/data/details/query", json!("q")),
            at(C::UnknownField, details),
            None,
        ),
        case(
            "credential_token",
            set(published(), "/data/details/token", json!("t")),
            at(C::UnknownField, details),
            None,
        ),
        case(
            "reason_text_in_details",
            set(withdrawn(), "/data/details/reason", json!("why")),
            at(C::UnknownField, details),
            None,
        ),
        case(
            "unknown_payload_member",
            set(created(), "/data/note", json!("x")),
            at(C::UnknownField, "data"),
            None,
        ),
        case(
            "unknown_attribute",
            set(created(), "/traceparent", json!("00-x")),
            bare(C::InvalidEnvelope),
            None,
        ),
        case(
            "missing_attribute",
            remove(created(), "/dataschema"),
            bare(C::InvalidEnvelope),
            None,
        ),
        case(
            "specversion",
            set(created(), "/specversion", json!("1.1")),
            at(C::InvalidEnvelope, "specversion"),
            None,
        ),
        case(
            "datacontenttype",
            set(created(), "/datacontenttype", json!("text/plain")),
            at(C::InvalidEnvelope, "datacontenttype"),
            None,
        ),
        case(
            "id_uppercase",
            set(created(), "/id", json!(EVENT_ID.to_uppercase())),
            at(C::InvalidEnvelope, "id"),
            None,
        ),
        case(
            "time_offset",
            set(
                created(),
                "/time",
                json!("2026-10-07T01:02:03.456789+00:00"),
            ),
            at(C::InvalidEnvelope, "time"),
            None,
        ),
        case(
            "time_calendar",
            set(created(), "/time", json!("2026-02-30T00:00:00.000000Z")),
            at(C::InvalidEnvelope, "time"),
            Some("calendar"),
        ),
        case(
            "schema_version",
            set(created(), "/data/schema_version", json!(2)),
            at(C::InvalidEnvelope, "data.schema_version"),
            None,
        ),
        case(
            "action_mismatch",
            set(created(), "/data/action", json!("document.created")),
            at(C::InvalidEnvelope, "data.action"),
            None,
        ),
        case(
            "event_class",
            set(created(), "/data/event_class", json!("SECURITY")),
            at(C::InvalidEnvelope, "data.event_class"),
            None,
        ),
        case(
            "wrong_type_counter",
            set(created(), "/data/details/versionNo", json!("2")),
            at(C::InvalidField, "versionNo"),
            None,
        ),
        case(
            "float_counter",
            set(created(), "/data/details/versionNo", json!(2.0)),
            at(C::InvalidField, "versionNo"),
            Some("float_integer"),
        ),
        case(
            "negative_counter",
            set(
                created(),
                "/data/details/resultingDocumentRevision",
                json!(-1),
            ),
            at(C::InvalidField, "resultingDocumentRevision"),
            None,
        ),
        case(
            "uuid_detail_uppercase",
            set(
                created(),
                "/data/details/baseDocumentVersionId",
                json!(BASE_VER.to_uppercase()),
            ),
            at(C::InvalidField, "baseDocumentVersionId"),
            None,
        ),
        case(
            "missing_required_detail",
            remove(created(), "/data/details/documentVersionId"),
            at(C::MissingField, "documentVersionId"),
            None,
        ),
        case(
            "digest_byte_range",
            set(diff(), "/data/details/result_digest/0", json!(256)),
            at(C::InvalidField, "result_digest"),
            None,
        ),
        case(
            "enum_value",
            set(diff(), "/data/details/verdict", json!("maybe")),
            at(C::InvalidField, "verdict"),
            None,
        ),
        case(
            "enum_list_duplicate",
            set(
                metadata(),
                "/data/details/changed_keys",
                json!(["category", "category"]),
            ),
            at(C::InvalidField, "changed_keys"),
            None,
        ),
        case(
            "legacy_time_mixed_sign",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 280, 1, 2, 3, 0, 0, -30, 15]),
            ),
            at(C::InvalidField, "publishedAt"),
            Some("calendar"),
        ),
        case(
            "legacy_time_ordinal",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 366, 1, 2, 3, 0, 0, 0, 0]),
            ),
            at(C::InvalidField, "publishedAt"),
            Some("calendar"),
        ),
        case(
            "legacy_time_float",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 280, 1, 2, 3, 0.5, 0, 0, 0]),
            ),
            at(C::InvalidField, "publishedAt"),
            None,
        ),
        case(
            "actor_extra_key",
            set(created(), "/data/actor/kind", json!("human")),
            at(C::InvalidActor, actor),
            None,
        ),
        case(
            "actor_too_long",
            set(
                created(),
                "/data/actor/principal_id",
                json!("p".repeat(300)),
            ),
            at(C::InvalidActor, actor),
            None,
        ),
        case(
            "actor_utf8_bytes",
            set(
                created(),
                "/data/actor/principal_id",
                json!("é".repeat(200)),
            ),
            at(C::InvalidActor, actor),
            Some("utf8_bytes"),
        ),
        case(
            "actor_control_char",
            set(
                created(),
                "/data/actor/principal_id",
                json!("poc\u{7}human"),
            ),
            at(C::InvalidActor, actor),
            None,
        ),
        case(
            "actor_empty",
            set(created(), "/data/actor/issuer", json!("")),
            at(C::InvalidActor, actor),
            None,
        ),
        case(
            "actor_bidi_override",
            set(
                created(),
                "/data/actor/principal_id",
                json!("admin\u{202e}nimda"),
            ),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_line_separator",
            set(
                created(),
                "/data/actor/principal_id",
                json!("poc\u{2028}human"),
            ),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_tag_characters",
            set(
                created(),
                "/data/actor/issuer",
                json!("poc\u{e0041}\u{e0042}"),
            ),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_bom",
            set(created(), "/data/actor/issuer", json!("\u{feff}poc")),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_whitespace_only",
            set(created(), "/data/actor/principal_id", json!(" ")),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_padded",
            set(created(), "/data/actor/principal_id", json!(" poc-human ")),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "actor_noncharacter",
            set(created(), "/data/actor/issuer", json!("poc\u{fdd0}")),
            at(C::InvalidActor, actor),
            charset,
        ),
        case(
            "service_executor_on_created",
            set(
                doc_created(),
                "/data/service_executor",
                json!({"issuer": "service", "principal_id": "scheduler"}),
            ),
            at(C::InvalidServiceExecutor, executor),
            None,
        ),
        case(
            "service_executor_control",
            set(
                published(),
                "/data/service_executor/principal_id",
                json!("sched\u{0}uler"),
            ),
            at(C::InvalidServiceExecutor, executor),
            None,
        ),
        case(
            "service_executor_bidi",
            set(
                published(),
                "/data/service_executor/principal_id",
                json!("\u{2066}scheduler"),
            ),
            at(C::InvalidServiceExecutor, executor),
            charset,
        ),
        case(
            "service_executor_bom",
            set(
                published(),
                "/data/service_executor/issuer",
                json!("service\u{feff}"),
            ),
            at(C::InvalidServiceExecutor, executor),
            charset,
        ),
        case(
            // Document ids are server-generated: not a client id.
            "resource_nil_server_id",
            set(doc_created(), "/data/resource/id", json!(NIL)),
            at(C::InvalidResource, resource),
            None,
        ),
        case(
            "resource_nil_folder_id",
            set(folder_created(), "/data/resource/id", json!(NIL)),
            at(C::NilClientId, "data.resource.id"),
            None,
        ),
        case(
            "nil_server_detail",
            set(doc_created(), "/data/details/documentId", json!(NIL)),
            at(C::InvalidField, "documentId"),
            None,
        ),
        case(
            "resource_nil_version_id",
            set(created(), "/data/resource/version_id", json!(NIL)),
            at(C::NilClientId, "data.resource.version_id"),
            None,
        ),
        case(
            "nil_parent_folder",
            set(
                folder_created(),
                "/data/details/parent_folder_id",
                json!(NIL),
            ),
            at(C::NilClientId, "parent_folder_id"),
            None,
        ),
        case(
            "nil_nullable_base_version",
            set(created(), "/data/details/baseDocumentVersionId", json!(NIL)),
            at(C::NilClientId, "baseDocumentVersionId"),
            None,
        ),
        case(
            "resource_version_forbidden",
            set(doc_created(), "/data/resource/version_id", json!(VER)),
            at(C::InvalidResource, resource),
            None,
        ),
        case(
            "resource_version_missing",
            remove(created(), "/data/resource/version_id"),
            at(C::InvalidResource, resource),
            None,
        ),
        case(
            "resource_type_not_allowed",
            set(created(), "/data/resource/type", json!("Folder")),
            at(C::InvalidResource, resource),
            None,
        ),
        case(
            "audit_store_resource",
            set(created(), "/data/resource/type", json!("AuditStore")),
            at(C::ControlTypeForbidden, "data.resource.type"),
            None,
        ),
        case(
            "result",
            set(created(), "/data/result", json!("failure")),
            at(C::InvalidResult, "data.result"),
            None,
        ),
        case(
            "subject_other_document",
            set(
                created(),
                "/subject",
                json!(format!("document/{OTHER_DOC}/version/{VER}")),
            ),
            at(C::InvalidSubject, "subject"),
            Some("subject_binding"),
        ),
        case(
            "subject_shape",
            set(created(), "/subject", json!(format!("folder/{DOC}"))),
            at(C::InvalidSubject, "subject"),
            None,
        ),
        case(
            "subject_resource_scope",
            set(
                acl_folder(),
                "/subject",
                json!(format!("document/{FOLDER}")),
            ),
            at(C::InvalidSubject, "subject"),
            None,
        ),
        case(
            "detail_resource_binding",
            set(created(), "/data/details/documentId", json!(OTHER_DOC)),
            at(C::InvalidField, "documentId"),
            Some("resource_binding"),
        ),
        case(
            "detail_type_binding",
            set(acl_folder(), "/data/details/target_type", json!("Document")),
            at(C::InvalidField, "target_type"),
            Some("resource_binding"),
        ),
        case(
            "correlation_binding",
            set(published(), "/data/correlation/operation_id", json!(OP)),
            at(C::InvalidCorrelation, correlation),
            Some("correlation_binding"),
        ),
        case(
            "correlation_unexpected",
            set(created(), "/data/correlation/operation_id", json!(OP)),
            at(C::InvalidCorrelation, correlation),
            None,
        ),
        case(
            "correlation_unknown_member",
            set(
                created(),
                "/data/correlation/legacy_correlation_id",
                json!(CORR),
            ),
            at(C::InvalidCorrelation, correlation),
            None,
        ),
        case(
            "source_correlation_not_uuid",
            set(
                created(),
                "/data/correlation/source_correlation_id",
                json!("req-123"),
            ),
            at(
                C::InvalidSourceCorrelation,
                "data.correlation.source_correlation_id",
            ),
            None,
        ),
        case(
            "source_correlation_w3c",
            set(
                created(),
                "/data/correlation/source_correlation_id",
                json!("4bf92f3577b34da6a3ce929d0e0e4736"),
            ),
            at(
                C::InvalidSourceCorrelation,
                "data.correlation.source_correlation_id",
            ),
            None,
        ),
        case(
            "trace_id_zero",
            set(
                created(),
                "/data/correlation/trace_id",
                json!("0".repeat(32)),
            ),
            at(C::InvalidCorrelation, "data.correlation.trace_id"),
            None,
        ),
        case(
            "trace_id_on_legacy_adapter",
            set(
                created(),
                "/data/correlation/trace_id",
                json!("4bf92f3577b34da6a3ce929d0e0e4736"),
            ),
            at(C::InvalidCorrelation, "data.correlation.trace_id"),
            None,
        ),
        case(
            "reason_code_binding",
            set(terminal(), "/data/reason_code", json!("identity_invalid")),
            at(C::InvalidField, "data.reason_code"),
            Some("reason_code_binding"),
        ),
        case(
            "reason_code_unexpected",
            set(created(), "/data/reason_code", json!("forbidden")),
            at(C::InvalidField, "data.reason_code"),
            None,
        ),
        case(
            "reason_summary_with_text",
            set(withdrawn(), "/data/reason/text", json!("why")),
            at(C::InvalidReason, "data.reason"),
            None,
        ),
        case(
            "reason_summary_missing",
            remove(withdrawn(), "/data/reason"),
            at(C::InvalidReason, "data.reason"),
            None,
        ),
        case(
            "reason_summary_negative",
            set(withdrawn(), "/data/reason/utf8_bytes", json!(-1)),
            at(C::InvalidReason, "data.reason"),
            None,
        ),
        case(
            "reason_summary_over_body_limit",
            set(withdrawn(), "/data/reason/utf8_bytes", json!(1_048_577)),
            at(C::InvalidReason, "data.reason"),
            None,
        ),
        case(
            "reason_on_acl",
            set(
                acl_folder(),
                "/data/reason",
                json!({"provided": true, "utf8_bytes": 3, "text_retained": "source_systems"}),
            ),
            at(C::InvalidReason, "data.reason"),
            None,
        ),
        case(
            "extensions_non_empty",
            set(created(), "/data/extensions/org.work.v1", json!({})),
            at(C::InvalidExtensions, "data.extensions"),
            None,
        ),
        case(
            "provenance_registration",
            set(created(), "/data/provenance/registration", json!("manual")),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "provenance_registration_missing",
            remove(created(), "/data/provenance/registration"),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "provenance_commitment",
            set(
                created(),
                "/data/provenance/source_commitment",
                json!("AB".repeat(32)),
            ),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "provenance_commitment_missing",
            remove(created(), "/data/provenance/source_commitment"),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "provenance_format",
            set(
                created(),
                "/data/provenance/source_format",
                json!("audit-store-control-v1"),
            ),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "provenance_future_adapter_version",
            set(created(), "/data/provenance/adapter_version", json!(2)),
            at(C::InvalidProvenance, provenance),
            None,
        ),
        case(
            "source_other",
            set(
                created(),
                "/source",
                json!("urn:knowledge-platform:search-platform"),
            ),
            at(C::InvalidSource, "source"),
            None,
        ),
        case(
            "source_audit_store",
            set(
                created(),
                "/source",
                json!("urn:knowledge-platform:audit-store"),
            ),
            at(C::ControlTypeForbidden, "source"),
            None,
        ),
        case(
            "control_type",
            set(created(), "/type", json!("audit.access.denied")),
            at(C::ControlTypeForbidden, "type"),
            None,
        ),
        case(
            "unknown_type",
            set(created(), "/type", json!("document.version.deleted")),
            at(C::UnknownEventType, "type"),
            None,
        ),
        case(
            "envelope_too_large",
            set(created(), "/subject", json!("d".repeat(33 * 1024))),
            bare(C::EnvelopeTooLarge),
            None,
        ),
        case(
            // Compact rendering under 32 KiB, jsonb rendering over it.
            "jsonb_rendering_over_limit",
            set(
                created(),
                "/data/details/padding",
                Value::Array(vec![json!(0); 12_000]),
            ),
            bare(C::EnvelopeTooLarge),
            None,
        ),
    ];
    let base = compact(&created());
    cases.push(text_case(
        "duplicate_key",
        base.replacen(
            "\"specversion\":\"1.0\"",
            "\"specversion\":\"1.0\",\"specversion\":\"1.0\"",
            1,
        ),
        bare(C::DuplicateKey),
        Some("duplicate_key"),
    ));
    cases.push(text_case(
        "duplicate_key_escaped",
        base.replacen(
            "\"specversion\":\"1.0\"",
            "\"specversion\":\"1.0\",\"spec\\u0076ersion\":\"1.0\"",
            1,
        ),
        bare(C::DuplicateKey),
        Some("duplicate_key"),
    ));
    cases.push(text_case(
        "exponent_counter",
        base.replacen("\"versionNo\":2", "\"versionNo\":2e0", 1),
        at(C::InvalidField, "versionNo"),
        Some("float_integer"),
    ));
    cases.push(text_case(
        "trailing_data",
        format!("{base} {{}}"),
        bare(C::InvalidJson),
        None,
    ));
    cases.push(EnvelopeCase {
        name: "relay_type_on_store_path",
        input: Input::Value(created()),
        path: Origin::Store,
        expected: at(C::ControlTypeForbidden, "type"),
        rust_only: Some("origin_path"),
    });
    cases.extend(control_rejections());
    cases
}

/// A full control envelope of `event_type` with one detail replaced.
pub fn control_with(event_type: &str, field: &str, value: Value) -> Value {
    let spec = audit_core::Catalog::embedded()
        .get(event_type)
        .unwrap_or_else(|| panic!("{event_type}"));
    let mut envelope = control_envelope(spec, true);
    envelope["data"]["details"][field] = value;
    envelope
}

/// Store-path rejections: reader-supplied filters and principal-valued
/// control fields admit no free text (S2 P5 values), and control events
/// never use the producer-facing `nil_client_id`.
fn control_rejections() -> Vec<EnvelopeCase> {
    use audit_core::RejectionCode as C;
    let intent = "audit.access.intent_opened";
    let store = |name, envelope: Value, field, rust_only| EnvelopeCase {
        name,
        input: Input::Value(envelope),
        path: Origin::Store,
        expected: at(C::InvalidField, field),
        rust_only,
    };
    let charset = Some("principal_charset");
    let prose = "a sentence of free text that a reader typed into the filter";
    vec![
        store(
            "filter_principal_bidi_and_tags",
            control_with(
                intent,
                "filter_actor_principal_id",
                json!("admin\u{202e}nimda\u{e0041}\u{e0042}"),
            ),
            "filter_actor_principal_id",
            charset,
        ),
        store(
            "filter_issuer_bom",
            control_with(intent, "filter_actor_issuer", json!("\u{feff}")),
            "filter_actor_issuer",
            charset,
        ),
        store(
            "filter_resource_prose",
            control_with(
                intent,
                "filter_resource_id",
                json!("Customer ACME merger codename FALCON, card 4111 1111 1111 1111"),
            ),
            "filter_resource_id",
            None,
        ),
        store(
            "filter_event_types_prose",
            control_with(intent, "filter_event_types", json!([prose])),
            "filter_event_types",
            None,
        ),
        store(
            "filter_source_line_separator",
            control_with(
                intent,
                "filter_source",
                json!(" not a urn \u{2028} line sep"),
            ),
            "filter_source",
            None,
        ),
        store(
            "filter_source_unknown_urn",
            control_with(
                intent,
                "filter_source",
                json!("urn:knowledge-platform:search-platform"),
            ),
            "filter_source",
            None,
        ),
        store(
            "target_principal_bidi",
            control_with(
                "audit.access_policy.changed",
                "target_principal_id",
                json!("admin\u{202e}nimda"),
            ),
            "target_principal_id",
            charset,
        ),
        store(
            "session_role_not_an_identifier",
            control_with("audit.access.denied", "session_role", json!("Odd Role")),
            "session_role",
            None,
        ),
        store(
            "selector_source_unknown",
            control_with(
                "audit.retention.policy_changed",
                "selector_sources",
                json!(["urn:knowledge-platform:search-platform"]),
            ),
            "selector_sources",
            None,
        ),
        store(
            "policy_retain_days_zero",
            control_with("audit.retention.policy_changed", "retain_days", json!(0)),
            "retain_days",
            None,
        ),
        store(
            "fingerprint_not_decimal",
            control_with(
                "audit.recovery.epoch_started",
                "new_timeline",
                json!("timeline two"),
            ),
            "new_timeline",
            None,
        ),
        store(
            "control_nil_target_event_id",
            control_with("audit.body.purged", "target_event_id", json!(NIL)),
            "target_event_id",
            None,
        ),
        store(
            "control_nil_in_event_id_filter",
            control_with(intent, "filter_event_ids", json!([EVENT_ID, NIL])),
            "filter_event_ids",
            None,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Golden pins (design §7.2, §14.1): shared by tests/golden_projection.rs and
// crates/audit-store-postgres/tests/store_golden.rs. Entries are keyed by the
// input row, so an edited fixture input adds an entry (and needs a
// re-projection) instead of rewriting one.
// ---------------------------------------------------------------------------

/// Compact JSON with object keys in byte order at every depth.
pub fn canonical_text(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        canonical_text(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_text).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

/// Every column of the claim row. The exhaustive destructuring makes a new
/// column a compile error here, so the key always covers the whole input.
pub fn row_json(row: &DocumentStagingProjection) -> Value {
    let DocumentStagingProjection {
        event_id,
        event_type,
        source,
        subject,
        actor_identity_provider,
        actor_principal_id,
        resource_type,
        resource_id,
        resource_version_id,
        result,
        trace_id,
        occurred_at,
        oversize,
        data,
        data_kind,
        reason_kind,
        reason_bytes,
        source_intact,
        source_commitment,
        registration_kind,
    } = row;
    json!({
        "event_id": event_id,
        "event_type": event_type,
        "source": source,
        "subject": subject,
        "actor_identity_provider": actor_identity_provider,
        "actor_principal_id": actor_principal_id,
        "resource_type": resource_type,
        "resource_id": resource_id,
        "resource_version_id": resource_version_id,
        "result": result,
        "trace_id": trace_id,
        "occurred_at": occurred_at,
        "oversize": oversize,
        "data": data,
        "data_kind": data_kind,
        "reason_kind": reason_kind,
        "reason_bytes": reason_bytes,
        "source_intact": source_intact,
        "source_commitment": source_commitment,
        "registration_kind": registration_kind,
    })
}

pub fn entry_key(name: &str, row: &DocumentStagingProjection) -> String {
    let digest = to_hex(&envelope_digest(&canonical_text(&row_json(row))));
    format!("{name}@{}", &digest[..16])
}

pub fn is_entry_key(key: &str) -> bool {
    key.rsplit_once('@').is_some_and(|(name, hash)| {
        !name.is_empty()
            && hash.len() == 16
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

pub fn section_digest(section: &Value) -> String {
    to_hex(&envelope_digest(&canonical_text(section)))
}

/// Checks golden sections (`versions`: adapter version -> {entry key ->
/// sha256 hex}) against the computed digests of the current
/// `LEGACY_ADAPTER_VERSION`. Failure messages name only the offending
/// entries: new keys with their digest (to append), changed keys without it
/// (`bump` explains the required version bump). Shared by the Rust
/// projection pin and the Store envelope pin.
pub fn check_golden(
    versions: &Map<String, Value>,
    current: i32,
    frozen: &[(i32, &str)],
    computed: &BTreeMap<String, String>,
    bump: &str,
) -> Result<(), String> {
    let mut problems = Vec::new();
    for (key, section) in versions {
        let Some(version) = key
            .parse::<i32>()
            .ok()
            .filter(|v| (1..=current).contains(v))
        else {
            problems.push(format!(
                "{bump}: section {key} is not a version in 1..={current}"
            ));
            continue;
        };
        let Some(entries) = section.as_object().filter(|e| !e.is_empty()) else {
            problems.push(format!("section {key} must be a non-empty object"));
            continue;
        };
        for (name, digest) in entries {
            if !is_entry_key(name) || !digest.as_str().is_some_and(is_hex_digest) {
                problems.push(format!(
                    "section {key}: entry {name} must be <fixture>@<16 hex> -> sha256 hex"
                ));
            }
        }
        if version < current {
            match frozen.iter().find(|(v, _)| *v == version) {
                None => problems.push(format!(
                    "section {key} is older than {current} but has no FROZEN_SECTION_DIGESTS entry"
                )),
                Some((_, pinned)) if *pinned != section_digest(section) => problems.push(format!(
                    "section {key} changed after it was frozen; never edit an existing version's entries"
                )),
                Some(_) => {}
            }
        }
    }
    for (version, _) in frozen {
        if *version >= current || !versions.contains_key(&version.to_string()) {
            problems.push(format!(
                "frozen section {version} is missing or not older than {current}"
            ));
        }
    }
    match versions
        .get(&current.to_string())
        .and_then(Value::as_object)
    {
        None => problems.push(format!("{bump}: no section for version {current}")),
        Some(pinned) => {
            for (key, digest) in computed {
                match pinned.get(key).and_then(Value::as_str) {
                    None => problems.push(format!(
                        "new fixture input, append to section {current}: \"{key}\": \"{digest}\""
                    )),
                    Some(pinned) if pinned != digest => {
                        problems.push(format!("{bump}: {key}"));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}
