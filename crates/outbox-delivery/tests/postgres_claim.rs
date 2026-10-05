use std::{collections::HashSet, sync::Arc, time::Duration};

use document_repository_postgres::migrate;
use outbox_delivery::{DeliveryError, DeliveryPolicy, OutboxStore, postgres::PostgresOutboxStore};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::Barrier;
use uuid::Uuid;

struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    url: String,
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
        .with_env_var("POSTGRES_DB", "outbox_claim_test")
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/outbox_claim_test");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    migrate(&pool)
        .await
        .expect("public Domain migration should pass");
    Fixture {
        _container: container,
        url,
        pool,
    }
}

async fn insert_event(
    pool: &PgPool,
    event_id: Uuid,
    attempt_count: i32,
    attempt_limit: Option<i32>,
    delivered: bool,
    payload: Value,
) {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
          attempt_count,attempt_limit,delivered_at) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3,'2000-01-01T00:00:00Z', \
                 '2000-01-01T00:00:00Z',$4,$5, \
                 CASE WHEN $6 THEN '2000-01-02T00:00:00Z'::timestamptz ELSE NULL END)",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(50))
    .bind(payload)
    .bind(attempt_count)
    .bind(attempt_limit)
    .bind(delivered)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn policy_mismatch_and_legacy_exhausted_refuse_claim() {
    // Removing any exact-field comparison would allow a mismatched worker to claim.
    let f = fixture().await;
    let store = PostgresOutboxStore::new(f.pool.clone(), DeliveryPolicy::default());
    store.verify_policy().await.unwrap();
    for invalid in [
        DeliveryPolicy {
            max_attempts: 33,
            ..DeliveryPolicy::default()
        },
        DeliveryPolicy {
            lease_min_ms: 999,
            ..DeliveryPolicy::default()
        },
        DeliveryPolicy {
            lease_max_ms: 120_001,
            ..DeliveryPolicy::default()
        },
        DeliveryPolicy {
            backoff_min_ms: 300_001,
            ..DeliveryPolicy::default()
        },
        DeliveryPolicy {
            backoff_max_ms: 999,
            ..DeliveryPolicy::default()
        },
    ] {
        let invalid_store = PostgresOutboxStore::new(f.pool.clone(), invalid);
        assert_eq!(
            invalid_store.verify_policy().await,
            Err(DeliveryError::InvalidConfig)
        );
        assert_eq!(
            invalid_store
                .claim(Uuid::from_u128(100), 1, Duration::from_secs(1))
                .await,
            Err(DeliveryError::InvalidConfig),
        );
    }
    let variations = [
        "UPDATE outbox_delivery_policy SET revision=2",
        "UPDATE outbox_delivery_policy SET max_attempts=9",
        "UPDATE outbox_delivery_policy SET lease_min_ms=2000",
        "UPDATE outbox_delivery_policy SET lease_max_ms=119000",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=2000",
        "UPDATE outbox_delivery_policy SET backoff_max_ms=299000",
    ];
    for changed_policy in variations {
        sqlx::query(changed_policy).execute(&f.pool).await.unwrap();
        assert_eq!(
            store.verify_policy().await,
            Err(DeliveryError::PolicyMismatch)
        );
        assert_eq!(
            store
                .claim(Uuid::from_u128(100), 1, Duration::from_secs(1))
                .await,
            Err(DeliveryError::PolicyMismatch),
        );
        sqlx::query(
            "UPDATE outbox_delivery_policy SET revision=1,max_attempts=8, \
             lease_min_ms=1000,lease_max_ms=120000,backoff_min_ms=1000,backoff_max_ms=300000",
        )
        .execute(&f.pool)
        .await
        .unwrap();
    }

    let ids: Vec<_> = (1..=33).map(Uuid::from_u128).collect();
    for &id in &ids {
        insert_event(&f.pool, id, 8, None, false, json!({"legacy": true})).await;
    }
    insert_event(
        &f.pool,
        Uuid::from_u128(34),
        8,
        None,
        true,
        json!({"already_delivered": true}),
    )
    .await;
    let expected = DeliveryError::LegacyExhausted {
        count: 33,
        first_ids: ids[..32].to_vec(),
    };
    assert_eq!(store.verify_policy().await.unwrap_err(), expected);
    assert_eq!(
        store
            .claim(Uuid::from_u128(100), 1, Duration::from_secs(1))
            .await,
        Err(expected),
    );
    let untouched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE delivered_at IS NULL \
         AND attempt_count=8 AND attempt_limit IS NULL AND lease_token IS NULL",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(untouched, 33);
}

#[tokio::test]
async fn claim_caps_batch_and_pins_limit_once() {
    // A 33rd claim, altered payload, or policy rollout rewrite must fail this test.
    let f = fixture().await;
    for n in 1..=35_u128 {
        insert_event(
            &f.pool,
            Uuid::from_u128(1_000 + n),
            0,
            None,
            false,
            json!({"n": n}),
        )
        .await;
    }
    let store = PostgresOutboxStore::new(f.pool.clone(), DeliveryPolicy::default());
    let owner = Uuid::from_u128(500);
    assert_eq!(
        store.claim(owner, 0, Duration::from_secs(1)).await,
        Err(DeliveryError::InvalidConfig)
    );
    assert_eq!(
        store.claim(owner, 33, Duration::from_secs(1)).await,
        Err(DeliveryError::InvalidConfig)
    );
    assert_eq!(
        store.claim(owner, 1, Duration::from_millis(999)).await,
        Err(DeliveryError::InvalidConfig)
    );
    assert_eq!(
        store.claim(owner, 1, Duration::from_secs(121)).await,
        Err(DeliveryError::InvalidConfig)
    );

    let claimed = store
        .claim(owner, 32, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(claimed.len(), 32);
    let ids: HashSet<_> = claimed.iter().map(|c| c.envelope.event_id).collect();
    let tokens: HashSet<_> = claimed.iter().map(|c| c.lease_token).collect();
    assert_eq!(ids.len(), 32);
    assert_eq!(tokens.len(), 32);
    for event in &claimed {
        assert_eq!(event.attempt, 1);
        assert_eq!(event.attempt_limit, 8);
        assert_eq!(event.lease_owner, owner);
        assert_eq!(event.envelope.event_type, "DocumentRegistered");
        assert_eq!(event.envelope.aggregate_type, "Document");
        let stored: Value =
            sqlx::query_scalar("SELECT payload FROM outbox_events WHERE event_id=$1")
                .bind(event.envelope.event_id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(event.envelope.payload, stored);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE attempt_count=1")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 32);

    // Existing row limits remain pinned when the DB policy changes for new rows.
    let old = Uuid::from_u128(1);
    insert_event(
        &f.pool,
        old,
        1,
        Some(3),
        false,
        json!({"unchanged": [1, null]}),
    )
    .await;
    sqlx::query("UPDATE outbox_delivery_policy SET revision=2,max_attempts=4")
        .execute(&f.pool)
        .await
        .unwrap();
    let rolled = PostgresOutboxStore::new(
        f.pool.clone(),
        DeliveryPolicy {
            revision: 2,
            max_attempts: 4,
            ..DeliveryPolicy::default()
        },
    );
    rolled.verify_policy().await.unwrap();
    let next = rolled
        .claim(owner, 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].envelope.event_id, old);
    assert_eq!(next[0].attempt, 2);
    assert_eq!(next[0].attempt_limit, 3);
    assert_eq!(next[0].envelope.payload, json!({"unchanged": [1, null]}));
}

#[tokio::test]
async fn concurrent_claims_are_disjoint() {
    // Removing row arbitration or using a fixed token would duplicate a live claim.
    let f = fixture().await;
    for workers in [2_usize, 4, 8] {
        for n in 0..workers * 4 {
            insert_event(
                &f.pool,
                Uuid::from_u128((workers * 100 + n) as u128),
                0,
                None,
                false,
                json!({"worker_wave": workers}),
            )
            .await;
        }
        let barrier = Arc::new(Barrier::new(workers));
        let mut tasks = Vec::new();
        for n in 0..workers {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&f.url)
                .await
                .unwrap();
            let store = PostgresOutboxStore::new(pool, DeliveryPolicy::default());
            let gate = barrier.clone();
            tasks.push(tokio::spawn(async move {
                gate.wait().await;
                store
                    .claim(
                        Uuid::from_u128((workers * 10 + n) as u128),
                        4,
                        Duration::from_secs(120),
                    )
                    .await
                    .unwrap()
            }));
        }
        let mut ids = HashSet::new();
        let mut tokens = HashSet::new();
        for task in tasks {
            for event in task.await.unwrap() {
                assert!(ids.insert(event.envelope.event_id));
                assert!(tokens.insert(event.lease_token));
            }
        }
        assert_eq!(ids.len(), workers * 4);
        assert_eq!(tokens.len(), workers * 4);
    }

    let retry_id = Uuid::from_u128(1);
    insert_event(
        &f.pool,
        retry_id,
        0,
        None,
        false,
        json!({"same": "payload"}),
    )
    .await;
    let first_store = PostgresOutboxStore::new(f.pool.clone(), DeliveryPolicy::default());
    let first = first_store
        .claim(Uuid::from_u128(700), 1, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].envelope.event_id, retry_id);
    let other_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&f.url)
        .await
        .unwrap();
    let other_store = PostgresOutboxStore::new(other_pool, DeliveryPolicy::default());
    assert!(
        other_store
            .claim(Uuid::from_u128(701), 1, Duration::from_secs(1))
            .await
            .unwrap()
            .is_empty()
    );
    tokio::time::sleep(Duration::from_millis(1_150)).await;
    let reclaimed = other_store
        .claim(Uuid::from_u128(701), 1, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].envelope.event_id, retry_id);
    assert_eq!(reclaimed[0].attempt, 2);
    assert_ne!(reclaimed[0].lease_token, first[0].lease_token);
    assert_eq!(reclaimed[0].envelope.payload, first[0].envelope.payload);
}
