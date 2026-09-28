use document_domain::{PolicyGrant, PrincipalRef};

use crate::{
    IdentityResolutionError, ManagementCommand, ManagementOperationId, ManagementResult,
    RepositoryError, VerifiedActorContext,
};

#[allow(async_fn_in_trait)]
pub trait IdentityContextResolver: Send + Sync {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError>;
}

#[allow(async_fn_in_trait)]
pub trait ManagementRepository: Send + Sync {
    async fn execute(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError>;

    async fn lookup(
        &self,
        ctx: &VerifiedActorContext,
        operation_id: ManagementOperationId,
    ) -> Result<Option<ManagementResult>, RepositoryError>;
}

#[allow(async_fn_in_trait)]
pub trait BootstrapRootPolicy: Send + Sync {
    async fn initialize_root_policy(
        &self,
        trusted_bootstrap_actor: &VerifiedActorContext,
        grants: Vec<PolicyGrant>,
    ) -> Result<ManagementResult, RepositoryError>;
}
