use document_application::{ApplicationError, CreateVersionCommand, VersionOperationId};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use uuid::Uuid;

fn operation_id() -> VersionOperationId {
    VersionOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap(),
    )
    .unwrap()
}

#[test]
fn version_operation_id_requires_uuid_v7() {
    assert!(VersionOperationId::try_from_uuid(Uuid::from_u128(1)).is_err());
    assert_eq!(operation_id().as_uuid().get_version_num(), 7);
}

#[test]
fn create_version_command_rejects_negative_revision() {
    let result = CreateVersionCommand::new(
        operation_id(),
        DocumentId::from_uuid(Uuid::from_u128(1)),
        DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        -1,
        PrincipalRef::new("test", "actor").unwrap(),
    );
    assert!(matches!(result, Err(ApplicationError::Validation(_))));
}
