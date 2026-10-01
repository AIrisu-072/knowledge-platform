use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use document_repository_postgres::migrate;
use outbox_delivery::{
    DeliveryError, DeliveryPolicy, ErrorCode, FenceResult, OutboxStore,
    postgres::PostgresOutboxStore,
};
use serde_json::json;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    port: u16,
    pool: PgPool,
}

async fn fixture() -> Fixture {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "outbox_settle_test")
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/outbox_settle_test");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    Fixture {
        _container: container,
        port,
        pool,
    }
}

async fn insert_event(pool: &PgPool, event_id: Uuid, attempt_count: i32, limit: Option<i32>) {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
          attempt_count,attempt_limit) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3, \
                 '2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',$4,$5)",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(50))
    .bind(json!({"preserve": [1, null]}))
    .bind(attempt_count)
    .bind(limit)
    .execute(pool)
    .await
    .unwrap();
}

#[derive(Debug, PartialEq)]
struct RowState {
    lease_token: Option<Uuid>,
    lease_owner: Option<Uuid>,
    lease_expires_at: Option<OffsetDateTime>,
    available_at: OffsetDateTime,
    delivered_at: Option<OffsetDateTime>,
    dead_lettered_at: Option<OffsetDateTime>,
    last_error_code: Option<String>,
    attempt_count: i32,
    attempt_limit: Option<i32>,
}

async fn state(pool: &PgPool, id: Uuid) -> RowState {
    let row = sqlx::query(
        "SELECT lease_token,lease_owner,lease_expires_at,available_at,delivered_at, \
         dead_lettered_at,last_error_code,attempt_count,attempt_limit \
         FROM outbox_events WHERE event_id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    RowState {
        lease_token: row.get("lease_token"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: row.get("lease_expires_at"),
        available_at: row.get("available_at"),
        delivered_at: row.get("delivered_at"),
        dead_lettered_at: row.get("dead_lettered_at"),
        last_error_code: row.get("last_error_code"),
        attempt_count: row.get("attempt_count"),
        attempt_limit: row.get("attempt_limit"),
    }
}

fn store(pool: &PgPool) -> PostgresOutboxStore {
    PostgresOutboxStore::new(pool.clone(), DeliveryPolicy::default())
}

#[tokio::test]
async fn expired_or_old_token_cannot_renew_ack_or_fail() {
    let f = fixture().await;
    let store = store(&f.pool);
    let owner = Uuid::from_u128(200);
    let old_id = Uuid::from_u128(1);
    insert_event(&f.pool, old_id, 0, None).await;
    let old = store.claim(owner, 1, Duration::from_secs(1)).await.unwrap();
    let old_token = old[0].lease_token;
    sqlx::query("UPDATE outbox_events SET lease_expires_at=clock_timestamp() WHERE event_id=$1")
        .bind(old_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let current = store
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(current[0].envelope.event_id, old_id);
    assert_ne!(current[0].lease_token, old_token);
    let before = state(&f.pool, old_id).await;
    assert_eq!(
        store.renew(old_id, old_token, Duration::from_secs(2)).await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(
        store.settle_success(old_id, old_token).await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(
        store
            .settle_failure(
                old_id,
                old_token,
                ErrorCode::IndexingFailed,
                false,
                Duration::from_secs(1)
            )
            .await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(state(&f.pool, old_id).await, before);
    assert_eq!(
        store.settle_success(old_id, current[0].lease_token).await,
        Ok(FenceResult::Updated)
    );
    let acked = state(&f.pool, old_id).await;
    assert!(acked.delivered_at.is_some());
    assert!(acked.lease_token.is_none());
    assert!(acked.lease_owner.is_none());
    assert!(acked.lease_expires_at.is_none());
    assert!(acked.dead_lettered_at.is_none());
    assert_eq!(acked.available_at, before.available_at);
    assert_eq!(acked.last_error_code, before.last_error_code);

    let expired_id = Uuid::from_u128(2);
    insert_event(&f.pool, expired_id, 0, None).await;
    let expired = store
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    let token = expired[0].lease_token;
    // Equality with the DB clock is already expired: all fences use strict >.
    sqlx::query("UPDATE outbox_events SET lease_expires_at=clock_timestamp() WHERE event_id=$1")
        .bind(expired_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let at_boundary = state(&f.pool, expired_id).await;
    let valid_at_db_clock: bool = sqlx::query_scalar(
        "SELECT lease_expires_at > clock_timestamp() FROM outbox_events WHERE event_id=$1",
    )
    .bind(expired_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(!valid_at_db_clock);
    assert_eq!(
        store.renew(expired_id, token, Duration::from_secs(2)).await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(
        store.settle_success(expired_id, token).await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(
        store
            .settle_failure(
                expired_id,
                token,
                ErrorCode::HandlerTimeout,
                true,
                Duration::from_secs(1)
            )
            .await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(state(&f.pool, expired_id).await, at_boundary);

    let live = store
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(live[0].envelope.event_id, expired_id);
    let token = live[0].lease_token;
    let db_before: OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        store.renew(expired_id, token, Duration::from_secs(2)).await,
        Ok(FenceResult::Updated)
    );
    let db_after: OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let renewed = state(&f.pool, expired_id).await;
    assert!(renewed.lease_expires_at.unwrap() >= db_before + time::Duration::seconds(2));
    assert!(renewed.lease_expires_at.unwrap() <= db_after + time::Duration::seconds(2));
}

#[tokio::test]
async fn retry_uses_db_clock_and_row_limit() {
    let f = fixture().await;
    let store = store(&f.pool);
    let owner = Uuid::from_u128(201);
    let retry_id = Uuid::from_u128(10);
    insert_event(&f.pool, retry_id, 0, Some(3)).await;
    let claim = store
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(claim[0].attempt_limit, 3);
    let token = claim[0].lease_token;
    let before = state(&f.pool, retry_id).await;
    for bad in [
        Duration::from_millis(999),
        Duration::from_millis(300_001),
        Duration::from_nanos(1_000_000_001),
    ] {
        assert_eq!(
            store
                .settle_failure(retry_id, token, ErrorCode::IndexingFailed, false, bad)
                .await,
            Err(DeliveryError::InvalidConfig)
        );
        assert_eq!(state(&f.pool, retry_id).await, before);
    }
    let db_before: OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        store
            .settle_failure(
                retry_id,
                token,
                ErrorCode::IndexingFailed,
                false,
                Duration::from_millis(1_500)
            )
            .await,
        Ok(FenceResult::Updated)
    );
    let db_after: OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let retried = state(&f.pool, retry_id).await;
    assert!(retried.available_at >= db_before + time::Duration::milliseconds(1_500));
    assert!(retried.available_at <= db_after + time::Duration::milliseconds(1_500));
    assert_eq!(retried.attempt_count, 1);
    assert_eq!(retried.attempt_limit, Some(3));
    assert_eq!(retried.last_error_code.as_deref(), Some("indexing_failed"));
    assert!(retried.lease_token.is_none());
    assert!(retried.lease_owner.is_none());
    assert!(retried.lease_expires_at.is_none());
    assert!(retried.dead_lettered_at.is_none());
    assert_eq!(
        store.settle_success(retry_id, token).await,
        Ok(FenceResult::Lost)
    );

    sqlx::query("UPDATE outbox_events SET available_at='2000-01-01T00:00:00Z' WHERE event_id=$1")
        .bind(retry_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let next = store
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(next[0].attempt, 2);
    assert_eq!(
        store
            .settle_failure(
                retry_id,
                next[0].lease_token,
                ErrorCode::UnsupportedEvent,
                true,
                Duration::from_secs(1)
            )
            .await,
        Ok(FenceResult::Updated)
    );
    let terminal = state(&f.pool, retry_id).await;
    assert!(terminal.dead_lettered_at.is_some());
    assert!(terminal.lease_token.is_none());
    assert_eq!(
        terminal.last_error_code.as_deref(),
        Some("unsupported_event")
    );

    for (index, limit) in [1, 8, 32].into_iter().enumerate() {
        let id = Uuid::from_u128(20 + index as u128);
        insert_event(&f.pool, id, limit - 1, Some(limit)).await;
        let current = store
            .claim(owner, 1, Duration::from_secs(120))
            .await
            .unwrap();
        assert_eq!(current[0].envelope.event_id, id);
        assert_eq!(current[0].attempt, limit);
        assert_eq!(current[0].attempt_limit, limit);
        assert_eq!(
            store
                .settle_failure(
                    id,
                    current[0].lease_token,
                    ErrorCode::DeliveryUnknownAtLimit,
                    false,
                    Duration::from_secs(1)
                )
                .await,
            Ok(FenceResult::Updated)
        );
        let at_limit = state(&f.pool, id).await;
        assert!(at_limit.dead_lettered_at.is_some(), "row limit {limit}");
        assert_eq!(at_limit.attempt_count, limit);
        assert_eq!(at_limit.attempt_limit, Some(limit));
        assert_eq!(
            at_limit.last_error_code.as_deref(),
            Some("delivery_unknown_at_limit")
        );
        assert!(at_limit.lease_token.is_none());
    }

    for (index, code) in ErrorCode::ALL.into_iter().enumerate() {
        let id = Uuid::from_u128(100 + index as u128);
        insert_event(&f.pool, id, 0, Some(1)).await;
        let current = store
            .claim(owner, 1, Duration::from_secs(120))
            .await
            .unwrap();
        assert_eq!(current[0].envelope.event_id, id);
        assert_eq!(
            store
                .settle_failure(
                    id,
                    current[0].lease_token,
                    code,
                    true,
                    Duration::from_secs(1)
                )
                .await,
            Ok(FenceResult::Updated)
        );
        assert_eq!(
            state(&f.pool, id).await.last_error_code.as_deref(),
            Some(code.as_str())
        );
    }
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM outbox_events WHERE event_id=$1")
            .bind(retry_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(payload, json!({"preserve": [1, null]}));
}

// One PostgreSQL protocol connection is forwarded unchanged until COMMIT.
// Its ReadyForQuery is consumed after the server has committed, then the
// client socket is closed before the commit response reaches SQLx.
fn drop_commit_response_proxy(
    postgres_port: u16,
) -> (u16, mpsc::Receiver<bool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let (mut client_reader, _) = listener.accept().unwrap();
        let mut client_writer = client_reader.try_clone().unwrap();
        let mut server_writer = TcpStream::connect(("127.0.0.1", postgres_port)).unwrap();
        let mut server_reader = server_writer.try_clone().unwrap();
        client_reader
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        server_reader
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let commit_sent = Arc::new(AtomicBool::new(false));
        let c2s_commit = commit_sent.clone();
        let forwarding = thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            let mut tail = Vec::new();
            while let Ok(n) = client_reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.windows(6).any(|w| w == b"COMMIT") {
                    c2s_commit.store(true, Ordering::Release);
                }
                if tail.len() > 16 {
                    tail.drain(..tail.len() - 16);
                }
                if server_writer.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
        let mut buf = [0_u8; 8192];
        let mut withheld = Vec::new();
        let mut committed = false;
        while let Ok(n) = server_reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            if commit_sent.load(Ordering::Acquire) {
                withheld.extend_from_slice(&buf[..n]);
                if withheld.windows(6).any(|w| w == [b'Z', 0, 0, 0, 5, b'I']) {
                    committed = true;
                    break;
                }
            } else if client_writer.write_all(&buf[..n]).is_err() {
                break;
            }
        }
        let _ = done_tx.send(committed);
        let _ = client_writer.shutdown(Shutdown::Both);
        let _ = server_reader.shutdown(Shutdown::Both);
        let _ = forwarding.join();
    });
    (proxy_port, done_rx, worker)
}

#[tokio::test]
async fn unknown_ack_commit_is_not_success() {
    let f = fixture().await;
    let id = Uuid::from_u128(500);
    insert_event(&f.pool, id, 0, None).await;
    let claim = store(&f.pool)
        .claim(Uuid::from_u128(250), 1, Duration::from_secs(120))
        .await
        .unwrap();
    let token = claim[0].lease_token;
    assert!(state(&f.pool, id).await.delivered_at.is_none());
    let (proxy_port, commit_seen, proxy_thread) = drop_commit_response_proxy(f.port);
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{proxy_port}/outbox_settle_test?sslmode=disable"
    );
    let proxy_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let via_proxy = PostgresOutboxStore::new(proxy_pool.clone(), DeliveryPolicy::default());
    assert_eq!(
        via_proxy.settle_success(id, token).await,
        Err(DeliveryError::StoreUnknown)
    );
    assert!(
        commit_seen.recv_timeout(Duration::from_secs(5)).unwrap(),
        "PostgreSQL must reach committed ReadyForQuery"
    );
    proxy_pool.close().await;
    proxy_thread.join().unwrap();

    // A separate direct connection reads durable state; the ambiguous caller
    // result alone cannot establish whether ack committed.
    let durable = state(&f.pool, id).await;
    assert!(durable.delivered_at.is_some());
    assert!(durable.lease_token.is_none());
    assert_eq!(
        store(&f.pool).settle_success(id, token).await,
        Ok(FenceResult::Lost)
    );
}

#[tokio::test]
async fn store_access_errors_are_unknown_for_every_fenced_operation() {
    let f = fixture().await;
    let id = Uuid::from_u128(600);
    insert_event(&f.pool, id, 0, None).await;
    let claimed = store(&f.pool)
        .claim(Uuid::from_u128(260), 1, Duration::from_secs(120))
        .await
        .unwrap();
    let token = claimed[0].lease_token;
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/outbox_settle_test",
        f.port
    );
    let unavailable = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    unavailable.close().await;
    let failed_store = PostgresOutboxStore::new(unavailable, DeliveryPolicy::default());
    assert_eq!(
        failed_store.renew(id, token, Duration::from_secs(1)).await,
        Err(DeliveryError::StoreUnknown)
    );
    assert_eq!(
        failed_store.settle_success(id, token).await,
        Err(DeliveryError::StoreUnknown)
    );
    assert_eq!(
        failed_store
            .settle_failure(
                id,
                token,
                ErrorCode::IndexingFailed,
                false,
                Duration::from_secs(1)
            )
            .await,
        Err(DeliveryError::StoreUnknown)
    );
    assert_eq!(state(&f.pool, id).await.lease_token, Some(token));
}
