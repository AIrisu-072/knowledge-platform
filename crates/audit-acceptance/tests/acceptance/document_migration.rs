//! Design §14.4 (last item) and §13: a Document migration that runs after
//! the relay migration. Adding a nullable column to the staging table (as
//! the Document owner) changes nothing for the producers, the registration
//! or the source digest: rows staged before and after are delivered
//! exactly once. Dropping or retyping a digested column fails (the relay's
//! `BEGIN ATOMIC` digest depends on it), so such a migration cannot
//! silently break the producers' staging at runtime.

use serde_json::json;

use crate::document::{Platform, small_lifecycle};
use crate::support::*;

#[test]
fn a_later_nullable_staging_column_keeps_producers_and_delivery_working() {
    run_scenario(later_document_migration);
}

async fn later_document_migration() {
    let env = Env::start().await;
    let platform = Platform::new(env.document.clone());
    platform.bootstrap().await;
    small_lifecycle(&platform, "before the migration").await;
    let before = staged_rows(&env).await.len();

    // A later Document migration, run by the Document owner.
    exec(
        &env.document,
        "ALTER TABLE public.audit_outbox_events ADD COLUMN synthetic_future_note text NULL",
    )
    .await;
    for refused in [
        "ALTER TABLE public.audit_outbox_events DROP COLUMN subject",
        "ALTER TABLE public.audit_outbox_events ALTER COLUMN result TYPE varchar(64)",
    ] {
        let error = sqlx::raw_sql(sqlx::AssertSqlSafe(refused.to_owned()))
            .execute(&env.document)
            .await
            .expect_err("a digested column cannot be dropped or retyped");
        let code = match &error {
            sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
            other => panic!("{other:?}"),
        };
        assert!(
            ["2BP01", "0A000"].contains(&code.as_str()),
            "{refused}: {error}"
        );
    }
    small_lifecycle(&platform, "after the migration").await;
    let staged = staged_rows(&env).await;
    assert_eq!(
        staged.len(),
        before + 5,
        "the producers stage and register as before"
    );

    assert_relay_posture_clean(&env).await;
    let relay = RunningRelay::start(&env);
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;
    let store = env.store_client().await;
    assert_delivered_exactly_once(&env, &env.store_admin, &store).await;
    let report = env.health(true).await;
    assert_eq!(
        report["delivered"]["quarantined_total"],
        json!(0),
        "{report}"
    );
    assert_eq!(report["installation"]["posture_violations"], json!([]));
    assert_eq!(report["alarms"], json!([]), "{report}");
}
