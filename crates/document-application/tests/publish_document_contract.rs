use document_application::{
    AUDIT_DOCUMENT_VERSION_PUBLISHED, ApplicationError, DOCUMENT_VERSION_PUBLISHED,
    PublishCommandIdentity, PublishDocumentCommand, PublishDocumentResult, PublishOperationId,
    PublishOperationRecord,
};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use time::OffsetDateTime;
use uuid::Uuid;

fn valid_operation_id(value: u8) -> PublishOperationId {
    let raw = format!("01890f7a-6f6e-7b0a-8000-{value:012x}");
    PublishOperationId::try_from_uuid(Uuid::parse_str(&raw).unwrap()).unwrap()
}

#[test]
fn publish_operation_id_requires_uuid_v7() {
    let valid = Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap();
    let invalid = Uuid::from_u128(1);

    assert_eq!(
        PublishOperationId::try_from_uuid(valid)
            .unwrap()
            .as_uuid(),
        valid
    );
    assert_eq!(
        PublishOperationId::try_from_uuid(invalid),
        Err(ApplicationError::Validation(
            "publish operation id must be UUIDv7".to_owned()
        ))
    );
}

#[test]
fn publish_command_rejects_negative_expected_revision() {
    let result = PublishDocumentCommand::new(
        valid_operation_id(1),
        DocumentId::from_uuid(Uuid::from_u128(10)),
        DocumentVersionId::from_uuid(Uuid::from_u128(11)),
        -1,
        PrincipalRef::new("test-idp", "actor-1").unwrap(),
    );

    assert_eq!(
        result.unwrap_err(),
        ApplicationError::Validation("expected document revision cannot be negative".to_owned())
    );
}

#[test]
fn publish_operation_record_matches_only_the_exact_command_identity() {
    let command = PublishDocumentCommand::new(
        valid_operation_id(2),
        DocumentId::from_uuid(Uuid::from_u128(20)),
        DocumentVersionId::from_uuid(Uuid::from_u128(21)),
        0,
        PrincipalRef::new("test-idp", "actor-1").unwrap(),
    )
    .unwrap();
    let identity = PublishCommandIdentity::from_command(&command);
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_000_010).unwrap();
    let result = PublishDocumentResult::from_persisted(
        command.publish_operation_id(),
        command.document_id(),
        command.target_document_version_id(),
        1,
        published_at,
    );
    let stored = PublishOperationRecord::new(identity.clone(), result.clone());

    assert!(stored.matches_identity(&identity));
    assert_eq!(stored.result(), &result);

    let different_actor = PublishDocumentCommand::new(
        command.publish_operation_id(),
        command.document_id(),
        command.target_document_version_id(),
        command.expected_document_revision(),
        PrincipalRef::new("test-idp", "actor-2").unwrap(),
    )
    .unwrap();
    assert!(!stored.matches_identity(&PublishCommandIdentity::from_command(
        &different_actor
    )));
}

#[test]
fn publish_result_round_trips_persisted_fields_and_event_names_are_stable() {
    let operation_id = valid_operation_id(3);
    let document_id = DocumentId::from_uuid(Uuid::from_u128(30));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(31));
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_000_020).unwrap();
    let result =
        PublishDocumentResult::from_persisted(operation_id, document_id, version_id, 1, published_at);

    assert_eq!(result.publish_operation_id(), operation_id);
    assert_eq!(result.document_id(), document_id);
    assert_eq!(result.document_version_id(), version_id);
    assert_eq!(result.resulting_document_revision(), 1);
    assert_eq!(result.published_at(), published_at);
    assert_eq!(DOCUMENT_VERSION_PUBLISHED, "DocumentVersionPublished");
    assert_eq!(
        AUDIT_DOCUMENT_VERSION_PUBLISHED,
        "document.version.published"
    );
}
