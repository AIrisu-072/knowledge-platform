use document_application::{
    IdentityContextResolver, IdentityResolutionError, InvocationKind, VerifiedActorContext,
    authorize_scheduled_publish,
};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use time::{Duration, OffsetDateTime};

struct Resolver {
    returned_principal: PrincipalRef,
}

impl IdentityContextResolver for Resolver {
    async fn resolve(
        &self,
        _principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        let subject = PolicySubject::new(
            PolicySubjectKind::Principal,
            self.returned_principal.identity_provider(),
            self.returned_principal.principal_id(),
        )
        .unwrap();
        VerifiedActorContext::from_trusted_adapter(
            self.returned_principal.clone(),
            vec![subject],
            OffsetDateTime::now_utc() + Duration::hours(1),
            InvocationKind::HumanInteractive,
            None,
        )
        .map_err(|_| IdentityResolutionError::InvalidIdentity)
    }
}

#[tokio::test]
async fn scheduler_records_executor_without_adding_executor_to_policy_subjects() {
    let requester = PrincipalRef::new("issuer", "requester").unwrap();
    let executor = PrincipalRef::new("service", "scheduler").unwrap();
    let ctx = authorize_scheduled_publish(
        &Resolver {
            returned_principal: requester.clone(),
        },
        &requester,
        &executor,
    )
    .await
    .unwrap();
    assert_eq!(ctx.principal(), &requester);
    assert_eq!(ctx.invocation_kind(), InvocationKind::Service);
    assert_eq!(ctx.service_executor(), Some(&executor));
    assert_eq!(ctx.subjects().len(), 1);
    assert_eq!(ctx.subjects()[0].subject_id(), "requester");
    assert_eq!(
        authorize_scheduled_publish(
            &Resolver {
                returned_principal: executor.clone(),
            },
            &requester,
            &executor,
        )
        .await
        .err(),
        Some(IdentityResolutionError::InvalidIdentity)
    );
}
