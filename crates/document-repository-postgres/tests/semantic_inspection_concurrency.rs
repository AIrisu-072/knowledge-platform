//! Task 11 RED: concurrent attempts converge or report deterministic disagreement.

#[path = "support/semantic_inspection.rs"]
mod support;

use std::sync::Arc;

use document_application::{RepositoryError, SemanticInspectionRepository};
use document_repository_postgres::{PostgresDocumentRepository, migrate};
use tokio::sync::Barrier;
use uuid::Uuid;

#[tokio::test]
async fn concurrent_identical_results_return_the_one_persisted_record() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(31);
    support::seed_file(&pool, id).await;
    let repo = PostgresDocumentRepository::new(pool.clone());
    let (first, second) = run_pair(&repo, id, 1, 1).await;
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(row_count(&pool, id).await, 1);
}

#[tokio::test]
async fn concurrent_different_results_leave_one_row_and_reject_loser() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(32);
    support::seed_file(&pool, id).await;
    let repo = PostgresDocumentRepository::new(pool.clone());
    let (first, second) = run_pair(&repo, id, 1, 2).await;
    let results = [first, second];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RepositoryError::SemanticInspectionDeterminismViolation)
            ))
            .count(),
        1
    );
    assert_eq!(row_count(&pool, id).await, 1);
}

async fn run_pair(
    repo: &PostgresDocumentRepository,
    id: Uuid,
    first_fingerprint: u8,
    second_fingerprint: u8,
) -> (
    Result<document_application::SemanticInspectionRecord, RepositoryError>,
    Result<document_application::SemanticInspectionRecord, RepositoryError>,
) {
    let barrier = Arc::new(Barrier::new(2));
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first_barrier = barrier.clone();
    let second_barrier = barrier.clone();
    tokio::join!(
        async move {
            first_barrier.wait().await;
            first_repo
                .insert_or_converge_semantic_inspection(support::record(id, first_fingerprint))
                .await
        },
        async move {
            second_barrier.wait().await;
            second_repo
                .insert_or_converge_semantic_inspection(support::record(id, second_fingerprint))
                .await
        }
    )
}

async fn row_count(pool: &sqlx::PgPool, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM document_semantic_inspections WHERE file_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}
