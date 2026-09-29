use crate::{RepositoryError, VerifiedActorContext};

use super::{DiffPairSnapshot, DiffRequest};

#[allow(async_fn_in_trait)]
pub trait DocumentDiffRepository: Send + Sync {
    async fn capture_pair(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError>;
}
