use uuid::Uuid;

use crate::{RepositoryError, VerifiedActorContext};

use super::{DiffPairSnapshot, DiffRequest, DiffResult};

#[allow(async_fn_in_trait)]
pub trait DocumentDiffRepository: Send + Sync {
    async fn capture_pair(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError>;

    async fn authorize_and_audit_result(
        &self,
        actor: &VerifiedActorContext,
        pair: &DiffPairSnapshot,
        result: &DiffResult,
        cache_hit: bool,
        correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError>;
}
