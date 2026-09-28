use std::collections::{BTreeMap, BTreeSet};

use document_application::{
    InvocationKind, ManagementCommand, ManagementOperationId, VerifiedActorContext,
    canonical_command_bytes, canonical_json_bytes, management_command_digest,
};
use document_domain::{DocumentId, PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn context() -> VerifiedActorContext {
    let principal = PrincipalRef::new("windows", "alice").unwrap();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "windows", "alice").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn metadata_command(set: BTreeMap<String, Value>) -> ManagementCommand {
    ManagementCommand::UpdateDocumentMetadata {
        operation_id: ManagementOperationId::try_from_uuid(
            Uuid::parse_str("0199a8ad-cf25-7f22-8fd5-5facbb735015").unwrap(),
        )
        .unwrap(),
        document_id: DocumentId::from_uuid(Uuid::from_u128(5)),
        expected_document_revision: 2,
        set,
        unset: BTreeSet::new(),
        reason: "classification update".into(),
    }
}

#[test]
fn digest_ignores_map_order_not_command_content() {
    let mut first = Map::new();
    first.insert("z".into(), json!(2));
    first.insert("a".into(), json!({"y": true, "x": "あ"}));
    let mut second = Map::new();
    second.insert("a".into(), json!({"x": "あ", "y": true}));
    second.insert("z".into(), json!(2));
    let a = metadata_command(BTreeMap::from([(
        "extensions".into(),
        Value::Object(first),
    )]));
    let b = metadata_command(BTreeMap::from([(
        "extensions".into(),
        Value::Object(second),
    )]));
    assert_eq!(
        canonical_command_bytes(&context(), &a).unwrap(),
        canonical_command_bytes(&context(), &b).unwrap()
    );
    assert_eq!(
        management_command_digest(&context(), &a).unwrap(),
        management_command_digest(&context(), &b).unwrap()
    );
    let changed = metadata_command(BTreeMap::from([(
        "extensions".into(),
        json!({"a": {"x": "い", "y": true}, "z": 2}),
    )]));
    assert_ne!(
        management_command_digest(&context(), &a).unwrap(),
        management_command_digest(&context(), &changed).unwrap()
    );
}

#[test]
fn independent_canonical_vector_and_command_digest_are_fixed() {
    let vector = json!({"z": 2, "a": {"y": true, "x": "あ"}});
    let bytes = canonical_json_bytes(&vector).unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        "{\"a\":{\"x\":\"あ\",\"y\":true},\"z\":2}"
    );
    let vector_digest =
        Sha256::digest([b"document-management-basics-v0\0".as_slice(), &bytes].concat());
    assert_eq!(
        hex(&vector_digest),
        "9c13119acee0ec840ee6af42b4d6f80a4cbdd5d650206d7070db7aef20f3bc30"
    );

    let command = metadata_command(BTreeMap::from([("category".into(), json!("risk"))]));
    assert_eq!(
        hex(&management_command_digest(&context(), &command).unwrap()),
        "6531fbf7571790766ed918da49ca98e0c3edac04dde9623b5e8d5adaab0a7cb8"
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn uuid_v4_and_conflicting_patch_are_rejected() {
    assert!(ManagementOperationId::try_from_uuid(Uuid::from_u128(1)).is_err());
    let mut command = metadata_command(BTreeMap::from([("category".into(), json!("risk"))]));
    if let ManagementCommand::UpdateDocumentMetadata { unset, .. } = &mut command {
        unset.insert("category".into());
    }
    assert!(canonical_command_bytes(&context(), &command).is_err());
}

#[test]
fn expired_context_and_oversized_reason_are_rejected() {
    let principal = PrincipalRef::new("windows", "alice").unwrap();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "windows", "alice").unwrap();
    assert!(
        VerifiedActorContext::from_trusted_adapter(
            principal,
            vec![subject],
            OffsetDateTime::now_utc() - Duration::seconds(1),
            InvocationKind::HumanInteractive,
            None
        )
        .is_err()
    );
    let mut command = metadata_command(BTreeMap::new());
    if let ManagementCommand::UpdateDocumentMetadata { reason, .. } = &mut command {
        *reason = "あ".repeat(342);
    }
    assert!(canonical_command_bytes(&context(), &command).is_err());
}

#[test]
fn patch_bounds_and_numeric_values_are_checked() {
    let oversize = metadata_command(BTreeMap::from([(
        "category".into(),
        json!("x".repeat(65_536)),
    )]));
    assert!(canonical_command_bytes(&context(), &oversize).is_err());
    let invalid_number = metadata_command(BTreeMap::from([("category".into(), json!(1))]));
    assert!(canonical_command_bytes(&context(), &invalid_number).is_err());
    let mut nested = json!("leaf");
    for _ in 0..17 {
        nested = json!({"child": nested});
    }
    let too_deep = metadata_command(BTreeMap::from([("extensions".into(), nested)]));
    assert!(canonical_command_bytes(&context(), &too_deep).is_err());
}
