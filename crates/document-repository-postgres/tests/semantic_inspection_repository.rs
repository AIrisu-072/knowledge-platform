//! Task 11 RED: authoritative lookup, immutable insert, full-result convergence.

#[path = "support/semantic_inspection.rs"]
mod support;

use document_application::{RepositoryError, SemanticInspectionRepository};
use document_domain::FileId;
use document_repository_postgres::{PostgresDocumentRepository, migrate};
use document_semantic_inspection_core::{
    DigitalSignatureEvidence, InspectionProfileVersion, SignatureValidity,
};
use uuid::Uuid;

#[tokio::test]
async fn repository_restores_file_and_round_trips_immutable_result() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(21);
    support::seed_file(&pool, id).await;
    let repo = PostgresDocumentRepository::new(pool.clone());
    let file = repo
        .get_file_object(FileId::from_uuid(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.content_hash().as_bytes(), &support::RAW_HASH);
    assert_eq!(file.size_bytes().get(), support::RAW_SIZE);
    assert_eq!(file.media_type().as_str(), "text/plain");
    assert_eq!(file.storage_key().as_str(), format!("objects/{id}/file"));
    assert!(
        repo.get_semantic_inspection(FileId::from_uuid(id), InspectionProfileVersion::DsiV0)
            .await
            .unwrap()
            .is_none()
    );

    let candidate = support::record(id, 1);
    let first = repo
        .insert_or_converge_semantic_inspection(candidate.clone())
        .await
        .unwrap();
    let replay = repo
        .insert_or_converge_semantic_inspection(candidate.clone())
        .await
        .unwrap();
    let loaded = repo
        .get_semantic_inspection(FileId::from_uuid(id), InspectionProfileVersion::DsiV0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, candidate);
    assert_eq!(replay, first);
    assert_eq!(loaded, first);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_semantic_inspections WHERE file_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn same_raw_binding_with_different_semantics_or_signature_is_rejected() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(22);
    support::seed_file(&pool, id).await;
    let repo = PostgresDocumentRepository::new(pool);
    repo.insert_or_converge_semantic_inspection(support::record(id, 1))
        .await
        .unwrap();

    let semantic_error = repo
        .insert_or_converge_semantic_inspection(support::record(id, 2))
        .await
        .unwrap_err();
    assert_eq!(
        semantic_error,
        RepositoryError::SemanticInspectionDeterminismViolation
    );

    let mut signature_only = support::response(1);
    signature_only
        .digital_signature_evidence
        .push(DigitalSignatureEvidence {
            signature_type: "cms".into(),
            signer_claim: None,
            certificate_subject: None,
            certificate_issuer: None,
            certificate_fingerprint: None,
            signed_at: None,
            cryptographic_validity: SignatureValidity::Unverifiable,
            covered_content: vec![],
            validation_diagnostics: vec![],
        });
    let signature_only = document_application::SemanticInspectionRecord::restore(
        FileId::from_uuid(id),
        signature_only,
        time::OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    let signature_error = repo
        .insert_or_converge_semantic_inspection(signature_only)
        .await
        .unwrap_err();
    assert_eq!(
        signature_error,
        RepositoryError::SemanticInspectionDeterminismViolation
    );
}

#[tokio::test]
async fn stale_cached_raw_binding_is_integrity_violation() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(23);
    support::seed_file(&pool, id).await;
    let repo = PostgresDocumentRepository::new(pool.clone());
    repo.insert_or_converge_semantic_inspection(support::record(id, 1))
        .await
        .unwrap();
    sqlx::query("UPDATE file_objects SET content_hash = $1 WHERE file_id = $2")
        .bind(vec![9_u8; 32])
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        repo.get_semantic_inspection(FileId::from_uuid(id), InspectionProfileVersion::DsiV0)
            .await
            .unwrap_err(),
        RepositoryError::IntegrityViolation,
    );
}
