use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, DocumentVersionId};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::{Uuid, Variant};

use crate::{
    ApplicationError, InvocationKind, RepositoryError, VerifiedActorContext, canonical_json_bytes,
};

pub const MAX_READ_STATE_REVISION: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CurrentReadProjection {
    pub first_read_at: Option<OffsetDateTime>,
    pub needs_recheck: bool,
    pub read_state_revision: i64,
}

impl CurrentReadProjection {
    pub fn is_read(&self) -> bool {
        self.first_read_at.is_some() && !self.needs_recheck
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentReadState {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub state: CurrentReadProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadStateOperationId(Uuid);

impl ReadStateOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() != 7 || value.get_variant() != Variant::RFC4122 {
            return Err(ApplicationError::Validation(
                "read-state operation id must be UUIDv7 with RFC variant".into(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStateMutationKind {
    View,
    Reset,
}

impl ReadStateMutationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "VIEW",
            Self::Reset => "RESET",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadStateMutation {
    pub operation_id: ReadStateOperationId,
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub expected_read_state_revision: i64,
    pub kind: ReadStateMutationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadStateMutationResult {
    pub operation_id: ReadStateOperationId,
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub kind: ReadStateMutationKind,
    pub expected_read_state_revision: i64,
    pub changed: bool,
    pub occurred_at: OffsetDateTime,
    pub resulting_read_state: CurrentReadProjection,
}

pub trait CurrentReadStateRepository: Send + Sync {
    fn get_current_read_state(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    ) -> impl Future<Output = Result<CurrentReadState, RepositoryError>> + Send;

    fn mutate_read_state(
        &self,
        ctx: &VerifiedActorContext,
        command: ReadStateMutation,
    ) -> impl Future<Output = Result<ReadStateMutationResult, RepositoryError>> + Send;
}

fn require_human(ctx: &VerifiedActorContext) -> Result<(), ApplicationError> {
    ctx.ensure_current()?;
    if ctx.invocation_kind() != InvocationKind::HumanInteractive {
        return Err(ApplicationError::Forbidden);
    }
    Ok(())
}

pub fn read_state_command_digest(
    ctx: &VerifiedActorContext,
    command: &ReadStateMutation,
) -> Result<[u8; 32], ApplicationError> {
    require_human(ctx)?;
    if !(0..=MAX_READ_STATE_REVISION).contains(&command.expected_read_state_revision) {
        return Err(ApplicationError::Validation(
            "expected read-state revision must be a safe nonnegative integer".into(),
        ));
    }
    let bytes = canonical_json_bytes(&json!({
        "schemaVersion": 1,
        "identityProvider": ctx.principal().identity_provider(),
        "principalId": ctx.principal().principal_id(),
        "invocationKind": ctx.invocation_kind().as_str(),
        "operationId": command.operation_id.as_uuid().to_string(),
        "documentId": command.document_id.as_uuid().to_string(),
        "versionId": command.document_version_id.as_uuid().to_string(),
        "kind": command.kind.as_str(),
        "expectedReadStateRevision": command.expected_read_state_revision
    }))?;
    let mut hash = Sha256::new();
    hash.update(b"document-current-read-state-v1\0");
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub struct CurrentReadStateService<R> {
    repository: Arc<R>,
}

impl<R: CurrentReadStateRepository> CurrentReadStateService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn get_current_read_state(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    ) -> Result<CurrentReadState, ApplicationError> {
        require_human(ctx)?;
        self.repository
            .get_current_read_state(ctx, document_id, document_version_id)
            .await
            .map_err(Into::into)
    }

    pub async fn mutate_read_state(
        &self,
        ctx: &VerifiedActorContext,
        command: ReadStateMutation,
    ) -> Result<ReadStateMutationResult, ApplicationError> {
        read_state_command_digest(ctx, &command)?;
        self.repository
            .mutate_read_state(ctx, command)
            .await
            .map_err(|error| match error {
                RepositoryError::CommitOutcomeUnknown => {
                    ApplicationError::CurrentReadStateCommitOutcomeUnknown {
                        operation_id: command.operation_id,
                        document_id: command.document_id,
                        document_version_id: command.document_version_id,
                    }
                }
                other => other.into(),
            })
    }
}
