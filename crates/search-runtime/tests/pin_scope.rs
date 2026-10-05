//! P7-10: evaluation pins bound to the actor scope. A pin cannot move to
//! another actor, evaluation or tenant; registration, visibility or access
//! changes revoke it; expiry is on the DB clock; a pin on an older key
//! survives the pointer moving on; the reader role cannot write lease rows.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use bundle::*;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, AccessRevision, AuthorizedSourceScope,
    CurrentSourceVisibilityPort, PrincipalRef, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, TrustedDiscoveryBinding,
};
use search_application::search_core::id::{DiscoveryEvaluationId, SessionId};
use search_runtime::pin::{PgEvaluationPins, PinError, PinTtl};

fn ttl() -> PinTtl {
    PinTtl::new(Duration::from_secs(60)).unwrap()
}

fn identity(
    authority: &SyntheticAuthorityAdapter,
    tenant: &str,
    principal: &str,
) -> AccessContextHandle {
    authority
        .issue_verified_identity(
            TenantId::new(tenant).unwrap(),
            PrincipalRef::new(principal).unwrap(),
            Some(SessionId::from_uuid(Uuid::now_v7())),
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(600),
        )
        .unwrap()
}

fn grant(
    visibility: &SyntheticVisibilityAdapter<'_>,
    tenant: &str,
    principal: &str,
    revision: u64,
) {
    visibility
        .grant(
            TenantId::new(tenant).unwrap(),
            PrincipalRef::new(principal).unwrap(),
            source_id(),
            registration::revision(1),
            registration::visibility(revision),
        )
        .unwrap();
}

async fn bind(
    authority: &SyntheticAuthorityAdapter,
    visibility: &SyntheticVisibilityAdapter<'_>,
    handle: &AccessContextHandle,
    evaluation: u128,
) -> (TrustedDiscoveryBinding, AuthorizedSourceScope) {
    let actor = authority.resolve(handle).await.unwrap().unwrap();
    let binding = authority
        .bind_discovery(
            &actor,
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(evaluation)),
        )
        .await
        .unwrap()
        .unwrap();
    let scope = visibility
        .bind_source(&actor, source_id())
        .await
        .unwrap()
        .unwrap();
    (binding, scope)
}

async fn lease_rows(admin: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_evaluation_lease")
        .fetch_one(admin)
        .await
        .unwrap()
}

#[tokio::test]
async fn pin_cannot_transfer_between_actor_scopes() {
    let fixture = fixture().await;
    let key = fixture.publish_current(7_950).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    grant(&visibility, "tenant-a", "alice", 1);
    grant(&visibility, "tenant-a", "bob", 1);
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );
    let alice = identity(&authority, "tenant-a", "alice");
    let (binding, scope) = bind(&authority, &visibility, &alice, 1).await;
    let pin = pins.pin_current(&binding, &scope, ttl()).await.unwrap();
    assert_eq!(pin.lease.key(), key);
    pins.verify_pin_before_return(&pin.lease, &binding, &scope)
        .await
        .unwrap();

    // Bob holds his own current scope and even the same evaluation ID.
    let bob = identity(&authority, "tenant-a", "bob");
    let (bob_binding, bob_scope) = bind(&authority, &visibility, &bob, 1).await;
    assert_eq!(
        pins.verify_pin_before_return(&pin.lease, &bob_binding, &bob_scope)
            .await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.renew(&pin.lease, &bob_binding, &bob_scope, ttl())
            .await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.release(&pin.lease, &bob_binding, &bob_scope).await,
        Err(PinError::Denied)
    );
    // Alice's other evaluation and a mixed binding/scope pair are refused.
    let (other_evaluation, other_scope) = bind(&authority, &visibility, &alice, 2).await;
    assert_eq!(
        pins.verify_pin_before_return(&pin.lease, &other_evaluation, &other_scope)
            .await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.verify_pin_before_return(&pin.lease, &bob_binding, &scope)
            .await,
        Err(PinError::Denied)
    );

    pins.renew(&pin.lease, &binding, &scope, ttl())
        .await
        .unwrap();
    pins.release(&pin.lease, &binding, &scope).await.unwrap();
    assert_eq!(lease_rows(&fixture.admin).await, 0);
    assert_eq!(
        pins.verify_pin_before_return(&pin.lease, &binding, &scope)
            .await,
        Err(PinError::Denied)
    );
}

#[tokio::test]
async fn registration_or_visibility_change_revokes_old_pin_read() {
    let fixture = fixture().await;
    fixture.publish_current(7_951).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    grant(&visibility, "tenant-a", "alice", 1);
    grant(&visibility, "tenant-a", "carol", 1);
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );

    // A visibility revision change revokes the old scope; no new scope binds.
    let alice = identity(&authority, "tenant-a", "alice");
    let (binding, scope) = bind(&authority, &visibility, &alice, 1).await;
    let pin = pins.pin_current(&binding, &scope, ttl()).await.unwrap();
    grant(&visibility, "tenant-a", "alice", 2);
    assert_eq!(
        pins.verify_pin_before_return(&pin.lease, &binding, &scope)
            .await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.release(&pin.lease, &binding, &scope).await,
        Err(PinError::Denied)
    );
    let actor = authority.resolve(&alice).await.unwrap().unwrap();
    assert!(
        visibility
            .bind_source(&actor, source_id())
            .await
            .unwrap()
            .is_none()
    );

    // An access revision change: the re-resolved actor cannot carry the pin.
    let carol = identity(&authority, "tenant-a", "carol");
    let (carol_binding, carol_scope) = bind(&authority, &visibility, &carol, 3).await;
    let carol_pin = pins
        .pin_current(&carol_binding, &carol_scope, ttl())
        .await
        .unwrap();
    authority
        .replace_access_revision(&carol, AccessRevision::new(2).unwrap())
        .unwrap();
    assert_eq!(
        pins.verify_pin_before_return(&carol_pin.lease, &carol_binding, &carol_scope)
            .await,
        Err(PinError::Denied)
    );
    let (fresh_binding, fresh_scope) = bind(&authority, &visibility, &carol, 3).await;
    assert_eq!(
        pins.verify_pin_before_return(&carol_pin.lease, &fresh_binding, &fresh_scope)
            .await,
        Err(PinError::Denied)
    );

    // A registration revision change through the host ledger revokes the
    // remaining scope in both the catalog gate and the stored row match.
    let (other_binding, other_scope) = bind(&authority, &visibility, &carol, 4).await;
    let other_pin = pins
        .pin_current(&other_binding, &other_scope, ttl())
        .await
        .unwrap();
    let next = registration::publish(
        &fixture.host,
        RegistrationNamespace::Document,
        2,
        vec![registration::document_with_revision(source_id(), "tenant-a", 2).await],
    )
    .await;
    catalog.replace_checked(&next).await.unwrap();
    assert_eq!(
        pins.verify_pin_before_return(&other_pin.lease, &other_binding, &other_scope)
            .await,
        Err(PinError::Denied)
    );
    // Revoked actors cannot release; only GC removes the rows after expiry.
    assert_eq!(lease_rows(&fixture.admin).await, 3);
}

#[tokio::test]
async fn foreign_tenant_cannot_renew_same_source_pin() {
    let fixture = fixture().await;
    fixture.publish_current(7_952).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    grant(&visibility, "tenant-a", "alice", 1);
    grant(&visibility, "tenant-b", "mallory", 1);
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );
    let alice = identity(&authority, "tenant-a", "alice");
    let (binding, scope) = bind(&authority, &visibility, &alice, 1).await;
    let pin = pins.pin_current(&binding, &scope, ttl()).await.unwrap();

    let mallory = identity(&authority, "tenant-b", "mallory");
    let actor = authority.resolve(&mallory).await.unwrap().unwrap();
    // No Source scope is minted for another tenant's Source.
    assert!(
        visibility
            .bind_source(&actor, source_id())
            .await
            .unwrap()
            .is_none()
    );
    let foreign = authority
        .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::from_u128(1)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pins.renew(&pin.lease, &foreign, &scope, ttl()).await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.release(&pin.lease, &foreign, &scope).await,
        Err(PinError::Denied)
    );
    pins.renew(&pin.lease, &binding, &scope, ttl())
        .await
        .unwrap();
    pins.verify_pin_before_return(&pin.lease, &binding, &scope)
        .await
        .unwrap();
}

#[tokio::test]
async fn old_key_pin_survives_pointer_but_not_expiry_restart_or_reader_dml() {
    let fixture = fixture().await;
    let first = fixture.publish_current(7_953).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    grant(&visibility, "tenant-a", "alice", 1);
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );
    let alice = identity(&authority, "tenant-a", "alice");
    let (binding, scope) = bind(&authority, &visibility, &alice, 1).await;
    let old = pins.pin_current(&binding, &scope, ttl()).await.unwrap();

    // The pointer moves on; the old key's pin still verifies and renews.
    let second = fixture.publish_current(7_954).await;
    pins.verify_pin_before_return(&old.lease, &binding, &scope)
        .await
        .unwrap();
    pins.renew(&old.lease, &binding, &scope, ttl())
        .await
        .unwrap();
    let (next_binding, next_scope) = bind(&authority, &visibility, &alice, 2).await;
    let new = pins
        .pin_current(&next_binding, &next_scope, ttl())
        .await
        .unwrap();
    assert_eq!((old.lease.key(), new.lease.key()), (first, second));

    // After a restart the host cannot re-resolve the old actor: fail closed.
    let restarted = SyntheticAuthorityAdapter::new();
    let after_restart = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &restarted,
        &visibility,
    );
    assert_eq!(
        after_restart
            .verify_pin_before_return(&new.lease, &next_binding, &next_scope)
            .await,
        Err(PinError::Denied)
    );

    // Expiry is on the DB clock; an expired pin can neither verify nor release.
    sqlx::query(
        "UPDATE search_evaluation_lease SET expires_at = clock_timestamp() - interval '1 second' \
         WHERE lease_id=$1",
    )
    .bind(old.lease.lease_id())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        pins.verify_pin_before_return(&old.lease, &binding, &scope)
            .await,
        Err(PinError::Denied)
    );
    assert_eq!(
        pins.release(&old.lease, &binding, &scope).await,
        Err(PinError::Denied)
    );
    assert_eq!(lease_rows(&fixture.admin).await, 2);

    // The reader role sees pins but cannot write them.
    let reader = fixture.login("search_reader").await;
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM search_evaluation_lease")
        .fetch_one(&reader)
        .await
        .unwrap();
    assert_eq!(visible, 2);
    for statement in [
        "UPDATE search_evaluation_lease SET expires_at = clock_timestamp() + interval '1 hour'",
        "DELETE FROM search_evaluation_lease",
        "INSERT INTO search_evaluation_lease SELECT * FROM search_evaluation_lease",
    ] {
        let error = sqlx::query(statement).execute(&reader).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501"),
            "{statement}"
        );
    }
}
