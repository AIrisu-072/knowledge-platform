use document_application::{
    ApplicationError, CreateOutcomeProbe, CreateOutcomeRecoveryService, CreateOutcomeRepository,
    InvocationKind, RepositoryError, VerifiedActorContext,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
struct MustNotRead;
impl CreateOutcomeRepository for MustNotRead {
    async fn recover_initial_create(
        &self,
        _: &VerifiedActorContext,
        _: CreateOutcomeProbe,
    ) -> Result<bool, RepositoryError> {
        panic!("invalid recovery identity must be rejected before repository access")
    }
}
#[tokio::test]
async fn malformed_multiple_receipts_are_rejected_before_read() {
    let ctx = VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test", "author").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test", "author").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    let first = FileId::from_uuid(Uuid::now_v7());
    let other = FileId::from_uuid(Uuid::now_v7());
    for ids in [
        vec![],
        vec![first, first],
        vec![other],
        (0..64).map(|_| FileId::from_uuid(Uuid::now_v7())).collect(),
    ] {
        let probe = CreateOutcomeProbe {
            document_id: DocumentId::from_uuid(Uuid::now_v7()),
            document_version_id: DocumentVersionId::from_uuid(Uuid::now_v7()),
            file_id: first,
            file_ids: Some(ids),
        };
        assert!(matches!(
            CreateOutcomeRecoveryService::new(Arc::new(MustNotRead))
                .recover(&ctx, probe)
                .await,
            Err(ApplicationError::Validation(_))
        ));
    }
}
