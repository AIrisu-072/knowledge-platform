use document_domain::{Action, PolicySubjectKind};
use organization_server::{
    OrganizationProfile, bootstrap_document_policy, organization_root_grants,
};

#[test]
fn fixture_policy_grants_office_read_only_and_keeps_provider_explicit() {
    let grants = organization_root_grants();
    assert_eq!(grants.len(), 3);
    let office = grants
        .iter()
        .find(|g| g.subject().subject_id() == "office-01")
        .unwrap();
    assert_eq!(
        office.subject().identity_provider(),
        "organization-synthetic"
    );
    assert_eq!(office.subject().kind(), PolicySubjectKind::Principal);
    assert_eq!(office.actions().len(), 2);
    assert!(office.actions().contains(&Action::Read));
    assert!(office.actions().contains(&Action::ReadHistory));
    let provider = grants
        .iter()
        .find(|g| g.subject().subject_id() == "poc-agent")
        .expect("the independent Document provider needs its own fixture grant");
    assert_eq!(provider.subject().identity_provider(), "poc");
    assert_eq!(provider.subject().kind(), PolicySubjectKind::Principal);
    assert_eq!(provider.actions().len(), 2);
    assert!(provider.actions().contains(&Action::Read));
    assert!(provider.actions().contains(&Action::ReadHistory));
}
#[tokio::test]
async fn office_cannot_bootstrap_before_any_database_access() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/synthetic")
        .unwrap();
    let error = bootstrap_document_policy(&pool, OrganizationProfile::Office)
        .await
        .unwrap_err();
    assert_eq!(error, "bootstrap-poc requires sales-01");
}
