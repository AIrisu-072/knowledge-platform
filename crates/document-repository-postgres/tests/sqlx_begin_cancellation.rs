//! A dropped `begin` future must roll back exactly what it started.
//!
//! sqlx-postgres 0.9.0 counted a BEGIN only after its reply. A future dropped
//! while waiting (a cancelled request) left the transaction open on a
//! connection returned to the pool at depth 0, and the next REPEATABLE READ
//! snapshot on it failed with 25001 (matching the intermittent Organization
//! Finding 503). A dropped SAVEPOINT rolled back the enclosing transaction.
//! The patched copy is `third_party/database/sqlx-postgres-0.9.0`.

#[path = "support/semantic_inspection.rs"]
mod support;

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

use sqlx::Connection;
use sqlx::postgres::{PgConnection, PgPoolOptions};

/// Polls `future` once and drops it; true when it was still waiting.
fn dropped_while_pending<F: Future>(future: F) -> bool {
    let mut future = pin!(future);
    future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending()
}

/// Sends a slow statement and drops its future, so the reply to whatever is
/// sent next cannot arrive within one poll: the connection must first wait
/// out the sleep. Without it a fast server can answer BEGIN inside the poll.
fn stall(conn: &mut PgConnection) {
    assert!(dropped_while_pending(
        sqlx::raw_sql("SELECT pg_sleep(0.1)").execute(&mut *conn)
    ));
}

/// SAVEPOINT succeeds only inside an open transaction block.
async fn transaction_open(conn: &mut PgConnection) -> bool {
    sqlx::raw_sql("SAVEPOINT kp_probe")
        .execute(&mut *conn)
        .await
        .is_ok()
}

#[tokio::test]
async fn dropped_begin_leaves_no_transaction_open() {
    let (_container, pool) = support::postgres().await;
    let mut conn = pool.acquire().await.unwrap();
    for _ in 0..3 {
        stall(&mut conn);
        assert!(dropped_while_pending(Connection::begin(&mut *conn)));
        assert!(!conn.is_in_transaction());
        assert!(
            !transaction_open(&mut conn).await,
            "a dropped BEGIN stayed open"
        );
    }
}

#[tokio::test]
async fn dropped_begin_keeps_the_next_pooled_snapshot_usable() {
    let (_container, pool) = support::postgres().await;
    // One connection, so every step reuses it; no acquire ping, so the next
    // checkout gets it exactly as the dropped `begin` left it.
    let single = PgPoolOptions::new()
        .max_connections(1)
        .test_before_acquire(false)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    for _ in 0..3 {
        let mut conn = single.acquire().await.unwrap();
        stall(&mut conn);
        assert!(dropped_while_pending(Connection::begin(&mut *conn)));
        drop(conn);
        let one: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&single)
            .await
            .unwrap();
        assert_eq!(one, 1);
        // The snapshot every Document read starts with.
        let mut tx = single.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .expect("the snapshot starts on a reused connection");
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn dropped_savepoint_keeps_the_enclosing_transaction() {
    let (_container, pool) = support::postgres().await;
    sqlx::query("CREATE TABLE begin_cancellation (i integer)")
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let mut outer = Connection::begin(&mut *conn).await.unwrap();
    sqlx::query("INSERT INTO begin_cancellation VALUES (1)")
        .execute(&mut *outer)
        .await
        .unwrap();
    stall(&mut outer);
    assert!(dropped_while_pending(Connection::begin(&mut *outer)));
    assert!(
        outer.is_in_transaction(),
        "the enclosing transaction was lost"
    );
    sqlx::query("INSERT INTO begin_cancellation VALUES (2)")
        .execute(&mut *outer)
        .await
        .unwrap();
    outer.commit().await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM begin_cancellation")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 2);
    assert!(!transaction_open(&mut conn).await);
}
