//! Explicit initialization uses Document's existing policy/read ports only.
use crate::{OrganizationProfile, SyntheticIdentityAdapter};
use document_application::{
    AccessPolicyReadRepository, BootstrapRootPolicy, DocumentDetailPurpose,
    DocumentDetailReadService, PolicyBindingMode,
};
use document_domain::{
    Action, DocumentId, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind, PolicyTarget,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// The original two-principal fixture. An existing database initialized with
/// exactly these grants stays accepted; its added principals simply have no
/// Document access (an honest provider denial, never a widened grant).
pub fn legacy_organization_root_grants() -> Vec<PolicyGrant> {
    let subject = |name| {
        PolicySubject::new(PolicySubjectKind::Principal, "organization-synthetic", name)
            .expect("fixed synthetic principal")
    };
    vec![
        PolicyGrant::new(subject("office-01"), [Action::Read, Action::ReadHistory])
            .expect("fixed read actions"),
        PolicyGrant::new(
            subject("sales-01"),
            [
                Action::Read,
                Action::ReadHistory,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ],
        )
        .expect("fixed fixture-author actions"),
        // Explicit provider grant belongs only to a new disposable fixture.
        // The Organization executor is distinct and receives no Document grant.
        PolicyGrant::new(
            PolicySubject::new(PolicySubjectKind::Principal, "poc", "poc-agent")
                .expect("fixed Document provider principal"),
            [Action::Read, Action::ReadHistory],
        )
        .expect("fixed read-only provider actions"),
    ]
}
/// New disposable fixtures additionally let every added synthetic Human
/// principal read shared inputs. Work assignment never implies this grant.
pub fn organization_root_grants() -> Vec<PolicyGrant> {
    let mut grants = legacy_organization_root_grants();
    for name in ["review-01", "approver-01", "multi-role-01", "delegate-01"] {
        grants.push(
            PolicyGrant::new(
                PolicySubject::new(PolicySubjectKind::Principal, "organization-synthetic", name)
                    .expect("fixed synthetic principal"),
                [Action::Read, Action::ReadHistory],
            )
            .expect("fixed read actions"),
        );
    }
    grants.sort_by(|a, b| a.subject().cmp(b.subject()));
    grants
}

pub async fn bootstrap_document_policy(
    pool: &PgPool,
    profile: OrganizationProfile,
) -> Result<(), &'static str> {
    if profile != OrganizationProfile::Sales {
        return Err("bootstrap-poc requires sales-01");
    }
    let actor = SyntheticIdentityAdapter::new(profile)
        .current_context()
        .map_err(|_| "synthetic identity unavailable")?;
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(
        pool.clone(),
        actor.principal().clone(),
    );
    let expected = organization_root_grants();
    match repository
        .initialize_root_policy(&actor, expected.clone())
        .await
    {
        Ok(_) => Ok(()),
        Err(document_application::RepositoryError::BusinessRule) => {
            let target = PolicyTarget::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID));
            let policy = repository
                .read_access_policy(&actor, target)
                .await
                .map_err(|_| "existing Document policy cannot be read")?;
            let mut actual = policy.effective_grants;
            actual.sort_by(|a, b| a.subject().cmp(b.subject()));
            if policy.target == target
                && policy.binding_mode == PolicyBindingMode::Explicit
                && policy.policy_id == Some(policy.effective_policy_id)
                && policy.effective_source == target
                && (actual == expected || actual == sorted(legacy_organization_root_grants()))
            {
                Ok(())
            } else {
                Err("existing Document policy differs; use a new disposable PoC database")
            }
        }
        Err(_) => Err("Document bootstrap failed"),
    }
}

fn sorted(mut grants: Vec<PolicyGrant>) -> Vec<PolicyGrant> {
    grants.sort_by(|a, b| a.subject().cmp(b.subject()));
    grants
}
/// Both original fixed users must be able to read the already-published shared input.
/// A Work reference never publishes a document or expands its current ACL.
pub async fn verify_shared_document(pool: &PgPool, id: Uuid) -> Result<(), &'static str> {
    let service =
        DocumentDetailReadService::new(Arc::new(PostgresDocumentRepository::new(pool.clone())));
    for profile in [OrganizationProfile::Sales, OrganizationProfile::Office] {
        let actor = SyntheticIdentityAdapter::new(profile)
            .current_context()
            .map_err(|_| "synthetic identity unavailable")?;
        service.read(&actor, DocumentId::from_uuid(id), DocumentDetailPurpose::Published).await
            .map_err(|_| "shared input must be published and currently readable by both synthetic profiles")?;
    }
    Ok(())
}
