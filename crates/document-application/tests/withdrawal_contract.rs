use document_application::{VersionOperationId, WithdrawVersionCommand};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use uuid::Uuid;

#[test]
fn withdrawal_command_requires_reason_and_revision() {
    let operation_id = VersionOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap(),
    )
    .unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(1));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(2));
    let actor = PrincipalRef::new("test", "actor").unwrap();
    assert!(
        WithdrawVersionCommand::new(operation_id, document_id, version_id, 1, actor.clone(), " ")
            .is_err()
    );
    assert!(
        WithdrawVersionCommand::new(operation_id, document_id, version_id, -1, actor, "retired")
            .is_err()
    );
}
