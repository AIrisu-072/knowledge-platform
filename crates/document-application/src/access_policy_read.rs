use std::future::Future;
use std::sync::Arc;

use document_domain::{PolicyGrant, PolicyId, PolicyTarget};

use crate::{ApplicationError, RepositoryError, VerifiedActorContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyBindingMode {
    Inherit,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPolicyRead {
    pub target: PolicyTarget,
    pub binding_mode: PolicyBindingMode,
    pub policy_id: Option<PolicyId>,
    pub policy_revision: i64,
    pub effective_policy_id: PolicyId,
    pub effective_source: PolicyTarget,
    pub effective_grants: Vec<PolicyGrant>,
}

pub trait AccessPolicyReadRepository: Send + Sync {
    fn read_access_policy(
        &self,
        ctx: &VerifiedActorContext,
        target: PolicyTarget,
    ) -> impl Future<Output = Result<AccessPolicyRead, RepositoryError>> + Send;
}

pub struct AccessPolicyReadService<R> {
    repository: Arc<R>,
}

impl<R: AccessPolicyReadRepository> AccessPolicyReadService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn read(
        &self,
        ctx: &VerifiedActorContext,
        target: PolicyTarget,
    ) -> Result<AccessPolicyRead, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .read_access_policy(ctx, target)
            .await
            .map_err(Into::into)
    }
}
