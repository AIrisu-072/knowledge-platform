use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use document_application::document_diff::{AuthorizedDiff, DiffRequest};
use document_application::{
    ApplicationError, DocumentRevisionDetail, DocumentRevisionDetailQuery,
    DocumentRevisionPageQuery, DocumentRevisionReadRepository, DocumentRevisionSummary,
    InvocationKind, MetadataComparisonStatus, Page, RevisionComparisonAuditRequest,
    RevisionComparisonService, RevisionContentComparator, RevisionContentComparison,
    VerifiedActorContext, compare_revision_metadata,
};
use document_domain::{
    DocumentId, DocumentVersionId, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

struct RevisionRepository {
    revisions: HashMap<Uuid, DocumentRevisionDetail>,
}

impl DocumentRevisionReadRepository for RevisionRepository {
    async fn list_document_revisions(
        &self,
        _ctx: &VerifiedActorContext,
        _query: DocumentRevisionPageQuery,
    ) -> Result<Page<DocumentRevisionSummary>, document_application::RepositoryError> {
        Ok(Page {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_document_revision(
        &self,
        _ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> Result<DocumentRevisionDetail, document_application::RepositoryError> {
        self.revisions
            .get(&query.revision_id)
            .cloned()
            .ok_or(document_application::RepositoryError::DocumentRevisionNotFound)
    }

    async fn authorize_and_audit_revision_comparison(
        &self,
        _ctx: &VerifiedActorContext,
        _request: RevisionComparisonAuditRequest,
    ) -> Result<Uuid, document_application::RepositoryError> {
        Ok(Uuid::now_v7())
    }
}

struct RecordingContentComparator {
    calls: AtomicUsize,
}

impl RevisionContentComparator for RecordingContentComparator {
    fn compare<'a>(
        &'a self,
        _ctx: &'a VerifiedActorContext,
        _request: DiffRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizedDiff, ApplicationError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(ApplicationError::RepositoryUnavailable) })
    }
}

fn revision(revision_id: Uuid, version_id: Uuid, metadata: Value) -> DocumentRevisionDetail {
    DocumentRevisionDetail {
        summary: DocumentRevisionSummary {
            revision_id,
            document_version_id: DocumentVersionId::from_uuid(version_id),
            major_no: 1,
            minor_no: 0,
            metadata_snapshot_status: "complete".into(),
            source_kind: "initialPublication".into(),
            created_at: OffsetDateTime::now_utc(),
        },
        metadata_snapshot: Some(metadata),
        actor: None,
        reason: None,
    }
}

fn context() -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", "reviewer").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "reviewer").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

#[test]
fn metadata_comparison_reports_structural_paths_without_inventing_legacy_values() {
    let base = json!({"category": "policy", "extensions": {"reviewers/team": ["a", "b"]}});
    let target = json!({"category": "guideline", "extensions": {"reviewers/team": ["a", "c"], "owner": "ops"}});

    let different =
        compare_revision_metadata("complete", Some(&base), "complete", Some(&target)).unwrap();

    assert_eq!(different.status, MetadataComparisonStatus::Different);
    assert_eq!(
        different
            .changes
            .iter()
            .map(|change| change.json_pointer.as_str())
            .collect::<Vec<_>>(),
        [
            "/category",
            "/extensions/owner",
            "/extensions/reviewers~1team"
        ]
    );

    let unavailable =
        compare_revision_metadata("unavailable_legacy", None, "complete", Some(&target)).unwrap();
    assert_eq!(
        unavailable.status,
        MetadataComparisonStatus::UnavailableLegacy
    );
    assert!(unavailable.changes.is_empty());
    assert!(unavailable.base_snapshot_digest.is_none());
    assert!(unavailable.target_snapshot_digest.is_some());
}

#[tokio::test]
async fn same_authoritative_version_revision_pair_never_calls_content_diff() {
    let document_id = DocumentId::from_uuid(Uuid::now_v7());
    let base_id = Uuid::now_v7();
    let target_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let repository = Arc::new(RevisionRepository {
        revisions: HashMap::from([
            (
                base_id,
                revision(base_id, version_id, json!({"category": "policy"})),
            ),
            (
                target_id,
                revision(target_id, version_id, json!({"category": "guideline"})),
            ),
        ]),
    });
    let content = Arc::new(RecordingContentComparator {
        calls: AtomicUsize::new(0),
    });
    let service = RevisionComparisonService::new(repository, content.clone());

    let comparison = service
        .compare(&context(), document_id, base_id, target_id)
        .await
        .unwrap();

    assert!(matches!(
        comparison.content,
        RevisionContentComparison::SameAuthoritativeVersion
    ));
    assert_eq!(
        comparison.metadata.status,
        MetadataComparisonStatus::Different
    );
    assert_eq!(content.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn different_authoritative_versions_reuse_the_content_diff_comparator() {
    let document_id = DocumentId::from_uuid(Uuid::now_v7());
    let base_id = Uuid::now_v7();
    let target_id = Uuid::now_v7();
    let repository = Arc::new(RevisionRepository {
        revisions: HashMap::from([
            (base_id, revision(base_id, Uuid::now_v7(), json!({}))),
            (target_id, revision(target_id, Uuid::now_v7(), json!({}))),
        ]),
    });
    let content = Arc::new(RecordingContentComparator {
        calls: AtomicUsize::new(0),
    });
    let service = RevisionComparisonService::new(repository, content.clone());

    let result = service
        .compare(&context(), document_id, base_id, target_id)
        .await;

    assert!(matches!(
        result,
        Err(ApplicationError::RepositoryUnavailable)
    ));
    assert_eq!(content.calls.load(Ordering::SeqCst), 1);
}
