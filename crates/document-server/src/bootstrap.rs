use document_application::{
    AccessPolicyReadRepository, BootstrapRootPolicy, PolicyBindingMode, RepositoryError,
};
use document_domain::{
    Action, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind, PolicyTarget,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use sqlx::PgPool;
use thiserror::Error;

use crate::identity::{PoCIdentityProfile, StaticPoCIdentityAdapter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Initialized,
    AlreadyInitialized,
}

/// Bootstrap diagnostics never carry database errors, credentials or policy data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BootstrapError {
    #[error("PoC root bootstrap requires the fixed human profile")]
    HumanProfileRequired,
    #[error("PoC bootstrap identity is unavailable")]
    IdentityUnavailable,
    #[error("PoC root policy initialization failed")]
    InitializationFailed,
    #[error("existing PoC root policy could not be verified")]
    PolicyReadFailed,
    #[error("existing root policy differs from the fixed PoC policy")]
    UnexpectedPolicy,
}

/// Explicitly initialize the disposable PoC root through the existing port.
///
/// A retry is successful only when the fixed human can read the exact expected
/// policy through the ordinary authorized repository contract.
pub async fn bootstrap_poc(
    pool: &PgPool,
    profile: PoCIdentityProfile,
) -> Result<BootstrapOutcome, BootstrapError> {
    if profile != PoCIdentityProfile::Human {
        return Err(BootstrapError::HumanProfileRequired);
    }
    let context = StaticPoCIdentityAdapter::new(profile)
        .current_context()
        .map_err(|_| BootstrapError::IdentityUnavailable)?;
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(
        pool.clone(),
        context.principal().clone(),
    );
    let expected = fixed_root_grants();
    match repository
        .initialize_root_policy(&context, expected.clone())
        .await
    {
        Ok(_) => Ok(BootstrapOutcome::Initialized),
        Err(RepositoryError::BusinessRule) => {
            let root = PolicyTarget::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID));
            let policy = repository
                .read_access_policy(&context, root)
                .await
                .map_err(|_| BootstrapError::PolicyReadFailed)?;
            let mut grants = policy.effective_grants;
            grants.sort_by(|left, right| left.subject().cmp(right.subject()));
            if policy.target != root
                || policy.binding_mode != PolicyBindingMode::Explicit
                || policy.policy_id != Some(policy.effective_policy_id)
                || policy.effective_source != root
                || grants != expected
            {
                return Err(BootstrapError::UnexpectedPolicy);
            }
            Ok(BootstrapOutcome::AlreadyInitialized)
        }
        Err(_) => Err(BootstrapError::InitializationFailed),
    }
}

fn fixed_root_grants() -> Vec<PolicyGrant> {
    let group = |name| {
        PolicySubject::new(PolicySubjectKind::Group, "poc", name)
            .expect("the fixed PoC group subject is valid")
    };
    vec![
        PolicyGrant::new(group("poc-agents"), [Action::Read, Action::ReadHistory])
            .expect("fixed agent actions are nonempty and unique"),
        PolicyGrant::new(
            group("poc-users"),
            [
                Action::Read,
                Action::ReadHistory,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ],
        )
        .expect("fixed human actions are nonempty and unique"),
    ]
}
