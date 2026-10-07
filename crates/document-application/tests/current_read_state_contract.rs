use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use document_application::{
    ApplicationError, CurrentReadProjection, CurrentReadState, CurrentReadStateRepository,
    CurrentReadStateService, InvocationKind, MAX_READ_STATE_REVISION, ReadStateMutation,
    ReadStateMutationKind, ReadStateMutationResult, ReadStateOperationId, RepositoryError,
    VerifiedActorContext, canonical_json_bytes, read_state_command_digest,
};
use document_domain::{DocumentId, DocumentVersionId, PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn context(provider: &str, kind: InvocationKind) -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new(provider, "alice").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, provider, "alice").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        kind,
        None,
    )
    .unwrap()
}

fn command() -> ReadStateMutation {
    ReadStateMutation {
        operation_id: ReadStateOperationId::try_from_uuid(
            Uuid::parse_str("0199a8ad-cf25-7f22-8fd5-5facbb735015").unwrap(),
        )
        .unwrap(),
        document_id: DocumentId::from_uuid(Uuid::from_u128(11)),
        document_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(12)),
        expected_read_state_revision: 0,
        kind: ReadStateMutationKind::View,
    }
}

#[derive(Default)]
struct FakeRepository {
    calls: AtomicUsize,
}

impl CurrentReadStateRepository for FakeRepository {
    async fn get_current_read_state(
        &self,
        _: &VerifiedActorContext,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    ) -> Result<CurrentReadState, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CurrentReadState {
            document_id,
            document_version_id,
            state: CurrentReadProjection::default(),
        })
    }

    async fn mutate_read_state(
        &self,
        _: &VerifiedActorContext,
        _: ReadStateMutation,
    ) -> Result<ReadStateMutationResult, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(RepositoryError::CommitOutcomeUnknown)
    }
}

#[test]
fn virtual_unread_and_legacy_row_preserve_exact_projection() {
    let unread = CurrentReadProjection::default();
    assert_eq!(
        unread,
        CurrentReadProjection {
            first_read_at: None,
            needs_recheck: false,
            read_state_revision: 0,
        }
    );
    assert!(!unread.is_read());
    let historical = CurrentReadProjection {
        first_read_at: Some(OffsetDateTime::UNIX_EPOCH),
        needs_recheck: false,
        read_state_revision: 1,
    };
    assert!(historical.is_read());
    assert!(!CurrentReadProjection { needs_recheck: true, ..historical }.is_read());
}

#[test]
fn operation_id_requires_uuid_v7_and_rfc_variant() {
    assert!(ReadStateOperationId::try_from_uuid(Uuid::nil()).is_err());
    assert!(ReadStateOperationId::try_from_uuid(
        Uuid::from_u128(0x00000000000070000000000000000001)
    ).is_err());
    assert_eq!(command().operation_id.as_uuid().get_version_num(), 7);
}

#[tokio::test]
async fn actor_and_invocation_only_come_from_current_trusted_context() {
    let repository = Arc::new(FakeRepository::default());
    let service = CurrentReadStateService::new(repository.clone());
    for kind in [InvocationKind::Agent, InvocationKind::Service] {
        let ctx = context("test-idp", kind);
        let cmd = command();
        assert_eq!(service.mutate_read_state(&ctx, cmd).await, Err(ApplicationError::Forbidden));
        assert_eq!(service.get_current_read_state(&ctx, cmd.document_id, cmd.document_version_id).await, Err(ApplicationError::Forbidden));
    }
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn revision_is_nonnegative_javascript_safe_integer_before_repository_use() {
    assert_eq!(MAX_READ_STATE_REVISION, 9_007_199_254_740_991);
    let repository = Arc::new(FakeRepository::default());
    let service = CurrentReadStateService::new(repository.clone());
    let ctx = context("test-idp", InvocationKind::HumanInteractive);
    for revision in [-1, 9_007_199_254_740_992, i64::MAX] {
        assert!(matches!(
            service.mutate_read_state(&ctx, ReadStateMutation { expected_read_state_revision: revision, ..command() }).await,
            Err(ApplicationError::Validation(_))
        ));
    }
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
    for revision in [0, MAX_READ_STATE_REVISION] {
        assert!(matches!(
            service.mutate_read_state(&ctx, ReadStateMutation { expected_read_state_revision: revision, ..command() }).await,
            Err(ApplicationError::CurrentReadStateCommitOutcomeUnknown { .. })
        ));
    }
}

#[test]
fn canonical_digest_binds_actor_operation_target_kind_and_revision() {
    let ctx = context("test-idp", InvocationKind::HumanInteractive);
    let cmd = command();
    let original = read_state_command_digest(&ctx, &cmd).unwrap();
    for modified in [
        ReadStateMutation { kind: ReadStateMutationKind::Reset, ..cmd },
        ReadStateMutation { expected_read_state_revision: 1, ..cmd },
        ReadStateMutation { document_id: DocumentId::from_uuid(Uuid::from_u128(13)), ..cmd },
        ReadStateMutation { document_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(14)), ..cmd },
        ReadStateMutation { operation_id: ReadStateOperationId::try_from_uuid(Uuid::now_v7()).unwrap(), ..cmd },
    ] {
        assert_ne!(original, read_state_command_digest(&ctx, &modified).unwrap());
    }
    assert_ne!(original, read_state_command_digest(&context("other-idp", InvocationKind::HumanInteractive), &cmd).unwrap());
    let canonical = canonical_json_bytes(&json!({
        "schemaVersion": 1,
        "identityProvider": "test-idp",
        "principalId": "alice",
        "invocationKind": "human_interactive",
        "operationId": cmd.operation_id.as_uuid().to_string(),
        "documentId": cmd.document_id.as_uuid().to_string(),
        "versionId": cmd.document_version_id.as_uuid().to_string(),
        "kind": "VIEW",
        "expectedReadStateRevision": 0
    })).unwrap();
    let expected: [u8; 32] = Sha256::digest(
        [b"document-current-read-state-v1\0".as_slice(), &canonical].concat()
    ).into();
    assert_eq!(original, expected);
}

#[tokio::test]
async fn unknown_outcome_keeps_operation_document_and_version_identity() {
    let cmd = command();
    let result = CurrentReadStateService::new(Arc::new(FakeRepository::default()))
        .mutate_read_state(&context("test-idp", InvocationKind::HumanInteractive), cmd)
        .await;
    assert_eq!(result, Err(ApplicationError::CurrentReadStateCommitOutcomeUnknown {
        operation_id: cmd.operation_id,
        document_id: cmd.document_id,
        document_version_id: cmd.document_version_id,
    }));
}
