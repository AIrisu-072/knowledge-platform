//! Synthetic fixtures mirroring every Document audit producer shape on main.
//! All identifiers and principals are synthetic.
#![allow(dead_code)]

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{DocumentStagingProjection, Origin, project};
use serde_json::{Value, json};

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
    pub code: audit_core::RejectionCode,
    pub rust_only: Option<&'static str>,
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
    let case = |name, value: Value, code, rust_only| EnvelopeCase {
        name,
        input: Input::Value(value),
        path: Origin::Relay,
        code,
        rust_only,
    };
    let text_case = |name, text: String, code, rust_only| EnvelopeCase {
        name,
        input: Input::Text(text),
        path: Origin::Relay,
        code,
        rust_only,
    };
    let compact = |value: &Value| serde_json::to_string(value).expect("serialize");
    let mut cases = vec![
        case(
            "free_text_note",
            set(created(), "/data/details/note", json!("x")),
            C::UnknownField,
            None,
        ),
        case(
            "free_text_body",
            set(created(), "/data/details/body", json!("x")),
            C::UnknownField,
            None,
        ),
        case(
            "free_text_query",
            set(diff(), "/data/details/query", json!("q")),
            C::UnknownField,
            None,
        ),
        case(
            "credential_token",
            set(published(), "/data/details/token", json!("t")),
            C::UnknownField,
            None,
        ),
        case(
            "reason_text_in_details",
            set(withdrawn(), "/data/details/reason", json!("why")),
            C::UnknownField,
            None,
        ),
        case(
            "unknown_payload_member",
            set(created(), "/data/note", json!("x")),
            C::UnknownField,
            None,
        ),
        case(
            "unknown_attribute",
            set(created(), "/traceparent", json!("00-x")),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "missing_attribute",
            remove(created(), "/dataschema"),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "specversion",
            set(created(), "/specversion", json!("1.1")),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "datacontenttype",
            set(created(), "/datacontenttype", json!("text/plain")),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "id_uppercase",
            set(created(), "/id", json!(EVENT_ID.to_uppercase())),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "time_offset",
            set(
                created(),
                "/time",
                json!("2026-10-07T01:02:03.456789+00:00"),
            ),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "time_calendar",
            set(created(), "/time", json!("2026-02-30T00:00:00.000000Z")),
            C::InvalidEnvelope,
            Some("calendar"),
        ),
        case(
            "schema_version",
            set(created(), "/data/schema_version", json!(2)),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "action_mismatch",
            set(created(), "/data/action", json!("document.created")),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "event_class",
            set(created(), "/data/event_class", json!("SECURITY")),
            C::InvalidEnvelope,
            None,
        ),
        case(
            "wrong_type_counter",
            set(created(), "/data/details/versionNo", json!("2")),
            C::InvalidField,
            None,
        ),
        case(
            "float_counter",
            set(created(), "/data/details/versionNo", json!(2.0)),
            C::InvalidField,
            Some("float_integer"),
        ),
        case(
            "negative_counter",
            set(
                created(),
                "/data/details/resultingDocumentRevision",
                json!(-1),
            ),
            C::InvalidField,
            None,
        ),
        case(
            "uuid_detail_uppercase",
            set(
                created(),
                "/data/details/baseDocumentVersionId",
                json!(BASE_VER.to_uppercase()),
            ),
            C::InvalidField,
            None,
        ),
        case(
            "missing_required_detail",
            remove(created(), "/data/details/documentVersionId"),
            C::MissingField,
            None,
        ),
        case(
            "digest_byte_range",
            set(diff(), "/data/details/result_digest/0", json!(256)),
            C::InvalidField,
            None,
        ),
        case(
            "enum_value",
            set(diff(), "/data/details/verdict", json!("maybe")),
            C::InvalidField,
            None,
        ),
        case(
            "enum_list_duplicate",
            set(
                metadata(),
                "/data/details/changed_keys",
                json!(["category", "category"]),
            ),
            C::InvalidField,
            None,
        ),
        case(
            "legacy_time_mixed_sign",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 280, 1, 2, 3, 0, 0, -30, 15]),
            ),
            C::InvalidField,
            Some("calendar"),
        ),
        case(
            "legacy_time_ordinal",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 366, 1, 2, 3, 0, 0, 0, 0]),
            ),
            C::InvalidField,
            Some("calendar"),
        ),
        case(
            "legacy_time_float",
            set(
                published(),
                "/data/details/publishedAt",
                json!([2026, 280, 1, 2, 3, 0.5, 0, 0, 0]),
            ),
            C::InvalidField,
            None,
        ),
        case(
            "actor_extra_key",
            set(created(), "/data/actor/kind", json!("human")),
            C::InvalidActor,
            None,
        ),
        case(
            "actor_too_long",
            set(
                created(),
                "/data/actor/principal_id",
                json!("p".repeat(300)),
            ),
            C::InvalidActor,
            None,
        ),
        case(
            "actor_utf8_bytes",
            set(
                created(),
                "/data/actor/principal_id",
                json!("é".repeat(200)),
            ),
            C::InvalidActor,
            Some("utf8_bytes"),
        ),
        case(
            "actor_control_char",
            set(
                created(),
                "/data/actor/principal_id",
                json!("poc\u{7}human"),
            ),
            C::InvalidActor,
            None,
        ),
        case(
            "actor_empty",
            set(created(), "/data/actor/issuer", json!("")),
            C::InvalidActor,
            None,
        ),
        case(
            "service_executor_on_created",
            set(
                doc_created(),
                "/data/service_executor",
                json!({"issuer": "service", "principal_id": "scheduler"}),
            ),
            C::InvalidServiceExecutor,
            None,
        ),
        case(
            "service_executor_control",
            set(
                published(),
                "/data/service_executor/principal_id",
                json!("sched\u{0}uler"),
            ),
            C::InvalidServiceExecutor,
            None,
        ),
        case(
            "resource_nil_id",
            set(doc_created(), "/data/resource/id", json!(NIL)),
            C::InvalidResource,
            None,
        ),
        case(
            "resource_version_forbidden",
            set(doc_created(), "/data/resource/version_id", json!(VER)),
            C::InvalidResource,
            None,
        ),
        case(
            "resource_version_missing",
            remove(created(), "/data/resource/version_id"),
            C::InvalidResource,
            None,
        ),
        case(
            "resource_type_not_allowed",
            set(created(), "/data/resource/type", json!("Folder")),
            C::InvalidResource,
            None,
        ),
        case(
            "audit_store_resource",
            set(created(), "/data/resource/type", json!("AuditStore")),
            C::ControlTypeForbidden,
            None,
        ),
        case(
            "result",
            set(created(), "/data/result", json!("failure")),
            C::InvalidResult,
            None,
        ),
        case(
            "subject_other_document",
            set(
                created(),
                "/subject",
                json!(format!("document/{OTHER_DOC}/version/{VER}")),
            ),
            C::InvalidSubject,
            Some("subject_binding"),
        ),
        case(
            "subject_shape",
            set(created(), "/subject", json!(format!("folder/{DOC}"))),
            C::InvalidSubject,
            None,
        ),
        case(
            "subject_resource_scope",
            set(
                acl_folder(),
                "/subject",
                json!(format!("document/{FOLDER}")),
            ),
            C::InvalidSubject,
            None,
        ),
        case(
            "detail_resource_binding",
            set(created(), "/data/details/documentId", json!(OTHER_DOC)),
            C::InvalidField,
            Some("resource_binding"),
        ),
        case(
            "detail_type_binding",
            set(acl_folder(), "/data/details/target_type", json!("Document")),
            C::InvalidField,
            Some("resource_binding"),
        ),
        case(
            "correlation_binding",
            set(published(), "/data/correlation/operation_id", json!(OP)),
            C::InvalidCorrelation,
            Some("correlation_binding"),
        ),
        case(
            "correlation_unexpected",
            set(created(), "/data/correlation/operation_id", json!(OP)),
            C::InvalidCorrelation,
            None,
        ),
        case(
            "correlation_unknown_member",
            set(
                created(),
                "/data/correlation/legacy_correlation_id",
                json!(CORR),
            ),
            C::InvalidCorrelation,
            None,
        ),
        case(
            "source_correlation_not_uuid",
            set(
                created(),
                "/data/correlation/source_correlation_id",
                json!("req-123"),
            ),
            C::InvalidSourceCorrelation,
            None,
        ),
        case(
            "source_correlation_w3c",
            set(
                created(),
                "/data/correlation/source_correlation_id",
                json!("4bf92f3577b34da6a3ce929d0e0e4736"),
            ),
            C::InvalidSourceCorrelation,
            None,
        ),
        case(
            "trace_id_zero",
            set(
                created(),
                "/data/correlation/trace_id",
                json!("0".repeat(32)),
            ),
            C::InvalidCorrelation,
            None,
        ),
        case(
            "reason_code_binding",
            set(terminal(), "/data/reason_code", json!("identity_invalid")),
            C::InvalidField,
            Some("reason_code_binding"),
        ),
        case(
            "reason_code_unexpected",
            set(created(), "/data/reason_code", json!("forbidden")),
            C::InvalidField,
            None,
        ),
        case(
            "reason_summary_with_text",
            set(withdrawn(), "/data/reason/text", json!("why")),
            C::InvalidReason,
            None,
        ),
        case(
            "reason_summary_missing",
            remove(withdrawn(), "/data/reason"),
            C::InvalidReason,
            None,
        ),
        case(
            "reason_summary_negative",
            set(withdrawn(), "/data/reason/utf8_bytes", json!(-1)),
            C::InvalidReason,
            None,
        ),
        case(
            "reason_on_acl",
            set(
                acl_folder(),
                "/data/reason",
                json!({"provided": true, "utf8_bytes": 3, "text_retained": "source_systems"}),
            ),
            C::InvalidReason,
            None,
        ),
        case(
            "extensions_non_empty",
            set(created(), "/data/extensions/org.work.v1", json!({})),
            C::InvalidExtensions,
            None,
        ),
        case(
            "provenance_registration",
            set(created(), "/data/provenance/registration", json!("manual")),
            C::InvalidProvenance,
            None,
        ),
        case(
            "provenance_commitment",
            set(
                created(),
                "/data/provenance/source_commitment",
                json!("AB".repeat(32)),
            ),
            C::InvalidProvenance,
            None,
        ),
        case(
            "provenance_format",
            set(
                created(),
                "/data/provenance/source_format",
                json!("audit-store-control-v1"),
            ),
            C::InvalidProvenance,
            None,
        ),
        case(
            "source_other",
            set(
                created(),
                "/source",
                json!("urn:knowledge-platform:search-platform"),
            ),
            C::InvalidSource,
            None,
        ),
        case(
            "source_audit_store",
            set(
                created(),
                "/source",
                json!("urn:knowledge-platform:audit-store"),
            ),
            C::ControlTypeForbidden,
            None,
        ),
        case(
            "control_type",
            set(created(), "/type", json!("audit.access.denied")),
            C::ControlTypeForbidden,
            None,
        ),
        case(
            "unknown_type",
            set(created(), "/type", json!("document.version.deleted")),
            C::UnknownEventType,
            None,
        ),
        case(
            "envelope_too_large",
            set(created(), "/subject", json!("d".repeat(25 * 1024))),
            C::EnvelopeTooLarge,
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
        C::DuplicateKey,
        Some("duplicate_key"),
    ));
    cases.push(text_case(
        "duplicate_key_escaped",
        base.replacen(
            "\"specversion\":\"1.0\"",
            "\"specversion\":\"1.0\",\"spec\\u0076ersion\":\"1.0\"",
            1,
        ),
        C::DuplicateKey,
        Some("duplicate_key"),
    ));
    cases.push(text_case(
        "exponent_counter",
        base.replacen("\"versionNo\":2", "\"versionNo\":2e0", 1),
        C::InvalidField,
        Some("float_integer"),
    ));
    cases.push(text_case(
        "trailing_data",
        format!("{base} {{}}"),
        C::InvalidJson,
        None,
    ));
    cases.push(EnvelopeCase {
        name: "relay_type_on_store_path",
        input: Input::Value(created()),
        path: Origin::Store,
        code: C::ControlTypeForbidden,
        rust_only: Some("origin_path"),
    });
    cases
}
