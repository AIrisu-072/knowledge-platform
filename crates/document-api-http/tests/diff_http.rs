use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::diff::diff_router;
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_application::document_diff::{
    AncillaryChange, Change, DiffCache, DiffCacheKey, DiffExecutionError, DiffExecutor,
    DiffInspectionEvidence, DiffPairSnapshot, DiffRequest, DiffResult, DocumentDiffRepository,
    LocatorGranularity, SnapshotItem, SourceEvidence, UnverifiedRegion, VersionSnapshot,
};
use document_application::{
    ApplicationError, AuditedFileGrant, ContentReader, DocumentRevisionDetail,
    DocumentRevisionDetailQuery, DocumentRevisionPageQuery, DocumentRevisionReadRepository,
    DocumentRevisionSummary, FileStorage, IdentityResolutionError, InspectionExecutionError,
    InvocationKind, Page, RepositoryError, RevisionComparisonAuditRequest,
    SemanticInspectionRecord, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
    VerifiedActorContext, VersionFileAccessRepository, VersionFileRequest, VersionPurpose,
};
use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest, WorkerDiffResponse,
    WorkerDisplayRequest, WorkerDisplayResponse,
};
use document_diff_worker::WorkerError;
use document_domain::{
    DocumentId, DocumentVersionId, FileId, MediaType, PolicySubject, PolicySubjectKind,
    PrincipalRef, StorageKey,
};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tokio::io::AsyncReadExt;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct FixedIdentity(VerifiedActorContext);

impl IdentityAdapter for FixedIdentity {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

struct FakeRepository {
    pair: DiffPairSnapshot,
    cache: Mutex<Option<(DiffCacheKey, DiffResult)>>,
    revisions: HashMap<Uuid, DocumentRevisionDetail>,
    capture_error: Option<RepositoryError>,
    deny_final: bool,
    deny_display_disclosure: bool,
    audit_cache_hits: Mutex<Vec<bool>>,
    result_audit_correlations: Mutex<Vec<Option<String>>>,
    file_audit_correlations: Mutex<Vec<Option<Uuid>>>,
}

impl DiffCache for FakeRepository {
    async fn get(&self, key: &DiffCacheKey) -> Result<Option<DiffResult>, RepositoryError> {
        Ok(self
            .cache
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(saved_key, result)| (saved_key == key).then(|| result.clone())))
    }

    async fn put(
        &self,
        key: DiffCacheKey,
        result: DiffResult,
        _expected_digest: [u8; 32],
    ) -> Result<(), RepositoryError> {
        *self.cache.lock().unwrap() = Some((key, result));
        Ok(())
    }
}

impl DocumentDiffRepository for FakeRepository {
    async fn capture_pair(
        &self,
        _actor: &VerifiedActorContext,
        _request: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError> {
        if let Some(error) = &self.capture_error {
            return Err(error.clone());
        }
        Ok(self.pair.clone())
    }

    async fn authorize_and_audit_result(
        &self,
        _actor: &VerifiedActorContext,
        _pair: &DiffPairSnapshot,
        _result: &DiffResult,
        cache_hit: bool,
        correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError> {
        if self.deny_final || (self.deny_display_disclosure && correlation_id.is_some()) {
            return Err(RepositoryError::Forbidden);
        }
        self.audit_cache_hits.lock().unwrap().push(cache_hit);
        self.result_audit_correlations
            .lock()
            .unwrap()
            .push(correlation_id.map(str::to_owned));
        Ok(id(0xa010, 1))
    }
}

impl VersionFileAccessRepository for FakeRepository {
    async fn authorize_and_audit_file(
        &self,
        _ctx: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        self.file_audit_correlations
            .lock()
            .unwrap()
            .push(request.correlation_id);
        Ok(AuditedFileGrant::new(
            StorageKey::new("test-storage-key").unwrap(),
            MediaType::new("text/plain").unwrap(),
            DISPLAY_BYTES.len() as i64,
            "display.txt".into(),
            id(0xa020, 1),
        ))
    }
}

impl DocumentRevisionReadRepository for FakeRepository {
    async fn list_document_revisions(
        &self,
        _ctx: &VerifiedActorContext,
        _query: DocumentRevisionPageQuery,
    ) -> Result<Page<DocumentRevisionSummary>, RepositoryError> {
        Ok(Page {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_document_revision(
        &self,
        _ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> Result<DocumentRevisionDetail, RepositoryError> {
        self.revisions
            .get(&query.revision_id)
            .cloned()
            .ok_or(RepositoryError::DocumentRevisionNotFound)
    }

    async fn authorize_and_audit_revision_comparison(
        &self,
        _ctx: &VerifiedActorContext,
        _request: RevisionComparisonAuditRequest,
    ) -> Result<Uuid, RepositoryError> {
        if self.deny_final {
            return Err(RepositoryError::Forbidden);
        }
        Ok(id(0xa010, 3))
    }
}

const DISPLAY_BYTES: &[u8] = b"render me\n";

struct UnusedStorage;

struct DisplayStorage;

impl FileStorage for UnusedStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }

    async fn open(
        &self,
        _key: &document_domain::StorageKey,
    ) -> Result<ContentReader, StorageError> {
        unreachable!("cache-backed HTTP contract must not open storage")
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        unreachable!()
    }
}

impl FileStorage for DisplayStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(tokio::io::BufReader::new(std::io::Cursor::new(
            DISPLAY_BYTES.to_vec(),
        ))))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        unreachable!()
    }
}

struct UnusedExecutor;

impl DiffExecutor for UnusedExecutor {
    async fn compare(
        &self,
        _request: WorkerDiffRequest,
        _base: ContentReader,
        _target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        unreachable!("cache-backed HTTP contract must not invoke a worker")
    }
}

struct InlineDisplayExecutor;

impl DiffExecutor for InlineDisplayExecutor {
    async fn compare(
        &self,
        _request: WorkerDiffRequest,
        _base: ContentReader,
        _target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        unreachable!("cache-backed display contract must not invoke semantic Diff")
    }

    async fn extract_display(
        &self,
        request: WorkerDisplayRequest,
        mut source: ContentReader,
    ) -> Result<WorkerDisplayResponse, DiffExecutionError> {
        let mut bytes = Vec::new();
        source
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| DiffExecutionError::Unavailable)?;
        let request_bytes =
            serde_json::to_vec(&request).map_err(|_| DiffExecutionError::InvalidWorkerResult)?;
        document_diff_worker::run_display_worker_shell(&request_bytes, std::io::Cursor::new(bytes))
            .map_err(|error| match error {
                WorkerError::RawBindingMismatch => DiffExecutionError::RawBindingMismatch,
                WorkerError::ResourceLimit(_) => DiffExecutionError::ResourceLimit,
                _ => DiffExecutionError::InvalidWorkerResult,
            })
    }
}

struct UnusedInspection;

impl DiffInspectionEvidence for UnusedInspection {
    async fn ensure(
        &self,
        _file_id: FileId,
        _profile: InspectionProfileVersion,
    ) -> Result<SemanticInspectionRecord, ApplicationError> {
        Err(ApplicationError::InspectionFailed(
            InspectionExecutionError::UnsupportedDocumentFormat,
        ))
    }
}

fn id(lane: u16, value: u64) -> Uuid {
    Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-{lane:04x}-{value:012x}")).unwrap()
}

fn context() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test-idp", "reader").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "reader").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn pair() -> DiffPairSnapshot {
    let document_id = DocumentId::from_uuid(id(0xa100, 1));
    let version = |lane: u16| VersionSnapshot {
        document_id,
        version_id: DocumentVersionId::from_uuid(id(lane, 1)),
        reference_purpose: VersionPurpose::History,
        document_revision: 7,
        title: "Transport contract".into(),
        items: vec![SnapshotItem {
            content_item_id: id(lane, 2),
            logical_path: "primary".into(),
            ordinal: 0,
            authoritative_representation_id: id(lane, 3),
            file_id: FileId::from_uuid(id(lane, 4)),
            format: Some(FormatId::Txt),
            inspection_profile: InspectionProfileVersion::DsiV0,
            semantic_fingerprint: Some([9; 32]),
            inspection_binding_digest: Some([8; 32]),
            raw_sha256: Sha256::digest(DISPLAY_BYTES).into(),
            size_bytes: DISPLAY_BYTES.len() as u64,
        }],
        manifest_fingerprint: [7; 32],
        version_metadata_digest: [6; 32],
    };
    DiffPairSnapshot {
        document_id,
        base: version(0xa110),
        target: version(0xa120),
    }
}

fn source(version: &VersionSnapshot, locator: SourceLocator) -> SourceEvidence {
    let item = &version.items[0];
    SourceEvidence {
        document_id: version.document_id,
        version_id: version.version_id,
        content_item_id: item.content_item_id,
        authoritative_representation_id: item.authoritative_representation_id,
        file_id: item.file_id,
        raw_sha256: item.raw_sha256,
        inspection_profile: item.inspection_profile,
        locator,
        granularity: LocatorGranularity::Exact,
        parser_provenance: "qualified-test-parser".into(),
    }
}

fn result(pair: &DiffPairSnapshot, verdict: ContentVerdict, coverage: DiffCoverage) -> DiffResult {
    let has_change = matches!(verdict, ContentVerdict::Different);
    let has_unverified = !matches!(coverage, DiffCoverage::Full);
    DiffResult {
        document_id: pair.document_id,
        base_version_id: pair.base.version_id,
        target_version_id: pair.target.version_id,
        base_snapshot_digest: pair.base.snapshot_digest(),
        target_snapshot_digest: pair.target.snapshot_digest(),
        profile: DiffProfileVersion::V0,
        resource_profile: ResourceProfileVersion::V0,
        verdict,
        coverage,
        changes: has_change
            .then(|| Change {
                operation: Some(ChangeOperation::Modified),
                relocation: Some(RelocationKind::Moved),
                facet: "visible_text".into(),
                base: Some(source(
                    &pair.base,
                    SourceLocator::TextSpan {
                        line: 3,
                        byte_start: 8,
                        byte_end: 16,
                    },
                )),
                target: Some(source(
                    &pair.target,
                    SourceLocator::TextSpan {
                        line: 4,
                        byte_start: 9,
                        byte_end: 18,
                    },
                )),
                reason_code: "semantic_content_changed".into(),
            })
            .into_iter()
            .collect(),
        unverified_regions: has_unverified
            .then(|| UnverifiedRegion {
                base: Some(source(&pair.base, SourceLocator::ContentItem)),
                target: Some(source(&pair.target, SourceLocator::ContentItem)),
                reason: UnverifiedReason::UnsupportedSemanticConstruct,
                navigation_hint: Some("両方の原本を確認してください".into()),
            })
            .into_iter()
            .collect(),
        ancillary_changes: vec![AncillaryChange {
            kind: "editorial_metadata".into(),
            base_digest: Some([3; 32]),
            target_digest: Some([4; 32]),
        }],
    }
}

fn router_for(
    result: DiffResult,
    capture_error: Option<RepositoryError>,
    deny_final: bool,
) -> (Arc<FakeRepository>, Router) {
    router_for_with_io(
        result,
        capture_error,
        deny_final,
        false,
        Arc::new(UnusedStorage),
        Arc::new(UnusedExecutor),
    )
}

fn router_for_display(
    result: DiffResult,
    capture_error: Option<RepositoryError>,
    deny_final: bool,
) -> (Arc<FakeRepository>, Router) {
    router_for_with_io(
        result,
        capture_error,
        deny_final,
        false,
        Arc::new(DisplayStorage),
        Arc::new(InlineDisplayExecutor),
    )
}

fn router_for_display_denied_at_disclosure(result: DiffResult) -> (Arc<FakeRepository>, Router) {
    router_for_with_io(
        result,
        None,
        false,
        true,
        Arc::new(DisplayStorage),
        Arc::new(InlineDisplayExecutor),
    )
}

fn router_for_with_io<F, E>(
    result: DiffResult,
    capture_error: Option<RepositoryError>,
    deny_final: bool,
    deny_display_disclosure: bool,
    storage: Arc<F>,
    executor: Arc<E>,
) -> (Arc<FakeRepository>, Router)
where
    F: FileStorage + 'static,
    E: DiffExecutor + 'static,
{
    let pair = pair();
    let key = DiffCacheKey::from_pair(&pair, DiffProfileVersion::V0, ResourceProfileVersion::V0);
    let repository = Arc::new(FakeRepository {
        revisions: HashMap::from([
            (
                id(0xa130, 1),
                revision_detail(
                    id(0xa130, 1),
                    pair.base.version_id,
                    0,
                    json!({"title": "Base"}),
                ),
            ),
            (
                id(0xa130, 2),
                revision_detail(
                    id(0xa130, 2),
                    pair.target.version_id,
                    1,
                    json!({"title": "Target"}),
                ),
            ),
            (
                id(0xa130, 3),
                revision_detail(
                    id(0xa130, 3),
                    pair.base.version_id,
                    2,
                    json!({"title": "Before"}),
                ),
            ),
            (
                id(0xa130, 4),
                revision_detail(
                    id(0xa130, 4),
                    pair.base.version_id,
                    3,
                    json!({"title": "After"}),
                ),
            ),
        ]),
        pair,
        cache: Mutex::new(Some((key, result))),
        capture_error,
        deny_final,
        deny_display_disclosure,
        audit_cache_hits: Mutex::new(Vec::new()),
        result_audit_correlations: Mutex::new(Vec::new()),
        file_audit_correlations: Mutex::new(Vec::new()),
    });
    let router = diff_router(
        repository.clone(),
        storage,
        executor,
        Arc::new(UnusedInspection),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    (repository, router)
}

fn revision_detail(
    revision_id: Uuid,
    version_id: DocumentVersionId,
    minor_no: i64,
    metadata_snapshot: Value,
) -> DocumentRevisionDetail {
    DocumentRevisionDetail {
        summary: DocumentRevisionSummary {
            revision_id,
            document_version_id: version_id,
            major_no: 1,
            minor_no,
            metadata_snapshot_status: "complete".into(),
            source_kind: "initialPublication".into(),
            created_at: OffsetDateTime::now_utc(),
        },
        metadata_snapshot: Some(metadata_snapshot),
        actor: None,
        reason: None,
    }
}

async fn compare(router: Router, pair: &DiffPairSnapshot, projection: &str) -> (StatusCode, Value) {
    compare_versions(
        router,
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        projection,
    )
    .await
}

async fn compare_versions(
    router: Router,
    document_id: DocumentId,
    base_version_id: DocumentVersionId,
    target_version_id: DocumentVersionId,
    projection: &str,
) -> (StatusCode, Value) {
    compare_versions_page(
        router,
        document_id,
        base_version_id,
        target_version_id,
        projection,
        None,
        None,
    )
    .await
}

async fn compare_versions_page(
    router: Router,
    document_id: DocumentId,
    base_version_id: DocumentVersionId,
    target_version_id: DocumentVersionId,
    projection: &str,
    page_size: Option<u16>,
    cursor: Option<&str>,
) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/documents/{}/comparisons",
                    document_id.as_uuid()
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "baseVersionId": base_version_id.as_uuid(),
                        "targetVersionId": target_version_id.as_uuid(),
                        "profile": "document-diff-v0",
                        "projection": projection,
                        "pageSize": page_size,
                        "cursor": cursor
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

async fn compare_revisions(
    router: Router,
    document_id: DocumentId,
    base_revision_id: Uuid,
    target_revision_id: Uuid,
) -> (StatusCode, Value) {
    compare_revisions_page(
        router,
        document_id,
        base_revision_id,
        target_revision_id,
        "diff",
        None,
        None,
    )
    .await
}

async fn compare_revisions_page(
    router: Router,
    document_id: DocumentId,
    base_revision_id: Uuid,
    target_revision_id: Uuid,
    projection: &str,
    page_size: Option<u16>,
    cursor: Option<&str>,
) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/documents/{}/revision-comparisons",
                    document_id.as_uuid()
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "baseRevisionId": base_revision_id,
                        "targetRevisionId": target_revision_id,
                        "projection": projection,
                        "pageSize": page_size,
                        "cursor": cursor
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

#[tokio::test]
async fn diff_and_table_projections_are_lossless_and_cache_hits_are_freshly_audited() {
    let pair = pair();
    let expected = result(&pair, ContentVerdict::Different, DiffCoverage::Partial);
    let expected_digest = expected.canonical_digest();
    let (repository, router) = router_for(expected, None, false);

    let (status, diff) = compare(router.clone(), &pair, "diff").await;
    assert_eq!(status, StatusCode::OK, "{diff}");
    assert_eq!(diff["projection"], "diff");
    assert_eq!(diff["verdict"], "different");
    assert_eq!(diff["coverage"], "partial");
    assert_eq!(diff["resultDigest"], hex(expected_digest));
    assert_eq!(diff["auditEventId"], id(0xa010, 1).to_string());
    assert_eq!(diff["changes"][0]["operation"], "modified");
    assert_eq!(diff["changes"][0]["relocation"], "moved");
    assert_eq!(diff["changes"][0]["base"]["locator"]["kind"], "textSpan");
    assert_eq!(
        diff["changes"][0]["base"]["representationId"],
        id(0xa110, 3).to_string()
    );
    assert_eq!(
        diff["unverifiedRegions"][0]["reason"],
        "unsupportedSemanticConstruct"
    );
    assert_eq!(diff["ancillaryChanges"][0]["baseDigest"], "03".repeat(32));
    assert!(diff.to_string().find("storage").is_none());

    let (status, table) = compare(router, &pair, "comparisonTable").await;
    assert_eq!(status, StatusCode::OK, "{table}");
    assert_eq!(table["projection"], "comparisonTable");
    assert_eq!(table["rows"][0]["state"], "confirmed");
    assert_eq!(table["rows"][1]["state"], "unverified");
    assert_eq!(table["rows"][2]["state"], "ancillary");
    assert_eq!(table["unverifiedRegions"].as_array().unwrap().len(), 1);
    assert!(table.get("ancillaryChanges").is_none());
    assert_eq!(&*repository.audit_cache_hits.lock().unwrap(), &[true, true]);
}

#[tokio::test]
async fn all_valid_verdict_and_coverage_states_are_successful_http_results() {
    for (verdict, coverage, expected_verdict, expected_coverage) in [
        (ContentVerdict::Same, DiffCoverage::Full, "same", "full"),
        (
            ContentVerdict::Different,
            DiffCoverage::Full,
            "different",
            "full",
        ),
        (
            ContentVerdict::Different,
            DiffCoverage::Partial,
            "different",
            "partial",
        ),
        (
            ContentVerdict::Unknown,
            DiffCoverage::Partial,
            "unknown",
            "partial",
        ),
        (
            ContentVerdict::Unknown,
            DiffCoverage::None,
            "unknown",
            "none",
        ),
    ] {
        let pair = pair();
        let (_, router) = router_for(result(&pair, verdict, coverage), None, false);
        let (status, body) = compare(router, &pair, "diff").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["verdict"], expected_verdict);
        assert_eq!(body["coverage"], expected_coverage);
    }
}

#[tokio::test]
async fn display_projection_preserves_authoritative_verdict_and_coverage() {
    let pair = pair();
    let expected = result(&pair, ContentVerdict::Same, DiffCoverage::Full);
    let (repository, router) = router_for(expected, None, false);

    let (status, body) = compare(router, &pair, "display").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["projection"], "display");
    assert_eq!(body["verdict"], "same");
    assert_eq!(body["coverage"], "full");
    assert_eq!(body["pageSize"], 50);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
    assert_eq!(body["resultAuditEventId"], id(0xa010, 1).to_string());
    assert_eq!(&*repository.audit_cache_hits.lock().unwrap(), &[true, true]);
}

#[tokio::test]
async fn display_page_size_accepts_one_hundred_and_rejects_one_hundred_one() {
    let pair = pair();
    let expected = result(&pair, ContentVerdict::Same, DiffCoverage::Full);
    let (_, router) = router_for(expected, None, false);

    let (status, accepted) = compare_versions_page(
        router.clone(),
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(100),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["pageSize"], 100);

    let (status, rejected) = compare_versions_page(
        router,
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(101),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{rejected}");
    assert_eq!(rejected["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn display_projection_returns_authoritative_fragments_and_bound_pages() {
    let pair = pair();
    let mut expected = result(&pair, ContentVerdict::Different, DiffCoverage::Partial);
    let locator = SourceLocator::TextSpan {
        line: 1,
        byte_start: 0,
        byte_end: DISPLAY_BYTES.len() as u32,
    };
    expected.changes[0].base.as_mut().unwrap().locator = locator.clone();
    expected.changes[0].target.as_mut().unwrap().locator = locator;
    expected.changes.push(expected.changes[0].clone());
    let (repository, router) = router_for_display(expected, None, false);

    let (status, first) = compare_versions_page(
        router.clone(),
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(1),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["verdict"], "different");
    assert_eq!(first["coverage"], "partial");
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["items"][0]["changeIndex"], 0);
    assert_eq!(first["items"][0]["base"]["kind"], "text");
    assert_eq!(first["items"][0]["base"]["text"], "render me");
    assert_eq!(first["items"][0]["target"]["text"], "render me");
    assert_eq!(first["unverifiedRegions"].as_array().unwrap().len(), 0);
    assert_eq!(first["resultAuditEventId"], id(0xa010, 1).to_string());
    let cursor = first["nextCursor"].as_str().unwrap().to_owned();

    let (status, second) = compare_versions_page(
        router.clone(),
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(1),
        Some(&cursor),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["verdict"], "different");
    assert_eq!(second["coverage"], "partial");
    assert_eq!(second["items"][0]["changeIndex"], 1);
    assert_eq!(second["unverifiedRegions"].as_array().unwrap().len(), 0);
    let second_cursor = second["nextCursor"].as_str().unwrap().to_owned();

    let (status, third) = compare_versions_page(
        router,
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(1),
        Some(&second_cursor),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{third}");
    assert_eq!(third["verdict"], "different");
    assert_eq!(third["coverage"], "partial");
    assert_eq!(third["items"].as_array().unwrap().len(), 0);
    assert_eq!(third["unverifiedRegions"].as_array().unwrap().len(), 1);
    assert!(third["nextCursor"].is_null());
    let expected_result_correlation = Some(id(0xa010, 1).to_string());
    assert_eq!(
        &*repository.result_audit_correlations.lock().unwrap(),
        &[
            None,
            expected_result_correlation.clone(),
            None,
            expected_result_correlation.clone(),
            None,
            expected_result_correlation.clone(),
        ]
    );
    assert_eq!(
        &*repository.file_audit_correlations.lock().unwrap(),
        &[
            Some(id(0xa010, 1)),
            Some(id(0xa010, 1)),
            Some(id(0xa010, 1)),
            Some(id(0xa010, 1))
        ]
    );
}

#[tokio::test]
async fn display_disclosure_reauthorization_failure_returns_no_fragment() {
    let pair = pair();
    let mut expected = result(&pair, ContentVerdict::Different, DiffCoverage::Full);
    let locator = SourceLocator::TextSpan {
        line: 1,
        byte_start: 0,
        byte_end: DISPLAY_BYTES.len() as u32,
    };
    expected.changes[0].base.as_mut().unwrap().locator = locator.clone();
    expected.changes[0].target.as_mut().unwrap().locator = locator;
    let (repository, router) = router_for_display_denied_at_disclosure(expected);

    let (status, body) = compare(router, &pair, "display").await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "FORBIDDEN");
    assert!(!body.to_string().contains("render me"));
    assert_eq!(repository.file_audit_correlations.lock().unwrap().len(), 2);
    assert_eq!(
        &*repository.result_audit_correlations.lock().unwrap(),
        &[None]
    );
}

#[tokio::test]
async fn display_json_page_stays_within_one_mibibyte_with_many_unverified_regions() {
    let pair = pair();
    let mut expected = result(&pair, ContentVerdict::Unknown, DiffCoverage::None);
    let prototype = expected.unverified_regions[0].clone();
    expected.changes.clear();
    expected.ancillary_changes.clear();
    expected.unverified_regions = (0..100)
        .map(|index| UnverifiedRegion {
            navigation_hint: Some(format!("region {index} {}", "x".repeat(12 * 1024))),
            ..prototype.clone()
        })
        .collect();
    let (_, router) = router_for(expected, None, false);

    let (status, body) = compare_versions_page(
        router.clone(),
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(100),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(serde_json::to_vec(&body).unwrap().len() <= 1024 * 1024);
    assert!(body["unverifiedRegions"].as_array().unwrap().len() < 100);
    let cursor = body["nextCursor"].as_str().unwrap().to_owned();
    assert_eq!(body["verdict"], "unknown");
    assert_eq!(body["coverage"], "none");

    let (second_status, second) = compare_versions_page(
        router,
        pair.document_id,
        pair.base.version_id,
        pair.target.version_id,
        "display",
        Some(100),
        Some(&cursor),
    )
    .await;

    assert_eq!(second_status, StatusCode::OK, "{second}");
    assert!(serde_json::to_vec(&second).unwrap().len() <= 1024 * 1024);
    assert_eq!(
        body["unverifiedRegions"].as_array().unwrap().len()
            + second["unverifiedRegions"].as_array().unwrap().len(),
        100
    );
    assert!(second["nextCursor"].is_null());
    assert_eq!(second["verdict"], "unknown");
    assert_eq!(second["coverage"], "none");
}

#[tokio::test]
async fn revision_comparison_keeps_content_and_metadata_projections_separate() {
    let pair = pair();
    let (_, router) = router_for(
        result(&pair, ContentVerdict::Different, DiffCoverage::Partial),
        None,
        false,
    );
    let (status, body) =
        compare_revisions(router, pair.document_id, id(0xa130, 1), id(0xa130, 2)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["projection"], "diff");
    assert_eq!(
        body["contentComparisonStatus"],
        "differentAuthoritativeVersions"
    );
    assert_eq!(body["verdict"], "different");
    assert_eq!(body["coverage"], "partial");
    assert_eq!(body["metadataComparisonStatus"], "different");
    assert_eq!(body["metadataChanges"][0]["path"], "/title");
    assert_eq!(body["changes"][0]["facet"], "visible_text");
    assert_eq!(body["auditEventId"], id(0xa010, 3).to_string());
    assert_eq!(body["contentAuditEventId"], id(0xa010, 1).to_string());
}

#[tokio::test]
async fn same_version_revision_comparison_returns_metadata_only_with_audit() {
    let pair = pair();
    let (repository, router) = router_for(
        result(&pair, ContentVerdict::Different, DiffCoverage::Partial),
        None,
        false,
    );
    let (status, body) =
        compare_revisions(router, pair.document_id, id(0xa130, 3), id(0xa130, 4)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["contentComparisonStatus"], "sameAuthoritativeVersion");
    assert_eq!(body["metadataComparisonStatus"], "different");
    assert_eq!(body["metadataChanges"][0]["path"], "/title");
    assert!(body.get("verdict").is_none());
    assert!(body.get("resultDigest").is_none());
    assert!(body["changes"].as_array().unwrap().is_empty());
    assert_eq!(body["auditEventId"], id(0xa010, 3).to_string());
    assert!(body.get("contentAuditEventId").is_none());
    assert!(repository.audit_cache_hits.lock().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_same_version_stale_pair_and_policy_revocation_are_hard_errors() {
    let pair = pair();
    let cached = result(&pair, ContentVerdict::Same, DiffCoverage::Full);
    let (repository, router) = router_for(cached.clone(), None, false);
    let (status, invalid) = compare_versions(
        router,
        pair.document_id,
        pair.base.version_id,
        pair.base.version_id,
        "diff",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid}");
    assert_eq!(invalid["code"], "VALIDATION_FAILED");
    assert!(repository.audit_cache_hits.lock().unwrap().is_empty());

    let (_, router) = router_for(
        cached.clone(),
        Some(RepositoryError::StaleComparisonInput),
        false,
    );
    let (status, stale) = compare(router, &pair, "diff").await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["code"], "STALE_COMPARISON_INPUT");

    let (repository, router) = router_for(cached, None, true);
    let (status, forbidden) = compare(router, &pair, "diff").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{forbidden}");
    assert_eq!(forbidden["code"], "FORBIDDEN");
    assert!(repository.audit_cache_hits.lock().unwrap().is_empty());
}

#[tokio::test]
async fn same_version_revision_display_is_metadata_only_and_does_not_open_content() {
    let pair = pair();
    let (_, router) = router_for(
        result(&pair, ContentVerdict::Different, DiffCoverage::Partial),
        None,
        false,
    );
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/documents/{}/revision-comparisons",
                    pair.document_id.as_uuid()
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "baseRevisionId": id(0xa130, 3),
                        "targetRevisionId": id(0xa130, 4),
                        "projection": "display"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["projection"], "display");
    assert_eq!(body["contentComparisonStatus"], "sameAuthoritativeVersion");
    assert_eq!(body["metadataComparisonStatus"], "different");
    assert_eq!(body["metadataChanges"][0]["path"], "/title");
    assert_eq!(body["pageSize"], 50);
    assert_eq!(body["displayItems"].as_array().unwrap().len(), 0);
    assert!(body.get("displayAuditEventId").is_none());
    assert!(body.get("contentAuditEventId").is_none());
}

#[tokio::test]
async fn revision_display_composes_metadata_and_content_pages_with_bound_cursors() {
    let pair = pair();
    let mut expected = result(&pair, ContentVerdict::Different, DiffCoverage::Partial);
    let locator = SourceLocator::TextSpan {
        line: 1,
        byte_start: 0,
        byte_end: DISPLAY_BYTES.len() as u32,
    };
    expected.changes[0].base.as_mut().unwrap().locator = locator.clone();
    expected.changes[0].target.as_mut().unwrap().locator = locator;
    expected.changes.push(expected.changes[0].clone());
    let (repository, router) = router_for_display(expected, None, false);

    let (status, first) = compare_revisions_page(
        router.clone(),
        pair.document_id,
        id(0xa130, 1),
        id(0xa130, 2),
        "display",
        Some(1),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first["contentComparisonStatus"],
        "differentAuthoritativeVersions"
    );
    assert_eq!(first["verdict"], "different");
    assert_eq!(first["coverage"], "partial");
    assert_eq!(first["metadataComparisonStatus"], "different");
    assert_eq!(first["metadataChanges"][0]["path"], "/title");
    assert_eq!(first["displayItems"][0]["changeIndex"], 0);
    assert_eq!(first["displayItems"][0]["base"]["text"], "render me");
    assert_eq!(first["unverifiedRegions"].as_array().unwrap().len(), 0);
    assert_eq!(first["auditEventId"], id(0xa010, 3).to_string());
    assert_eq!(
        first["displayResultAuditEventId"],
        id(0xa010, 1).to_string()
    );
    let cursor = first["nextCursor"].as_str().unwrap().to_owned();

    let (stale_status, stale) = compare_revisions_page(
        router.clone(),
        pair.document_id,
        id(0xa130, 2),
        id(0xa130, 1),
        "display",
        Some(1),
        Some(&cursor),
    )
    .await;
    assert_eq!(stale_status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["code"], "CURSOR_STALE");

    let (status, second) = compare_revisions_page(
        router.clone(),
        pair.document_id,
        id(0xa130, 1),
        id(0xa130, 2),
        "display",
        Some(1),
        Some(&cursor),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["displayItems"][0]["changeIndex"], 1);
    assert_eq!(second["unverifiedRegions"].as_array().unwrap().len(), 0);
    let second_cursor = second["nextCursor"].as_str().unwrap().to_owned();

    let (status, third) = compare_revisions_page(
        router,
        pair.document_id,
        id(0xa130, 1),
        id(0xa130, 2),
        "display",
        Some(1),
        Some(&second_cursor),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{third}");
    assert_eq!(
        third["contentComparisonStatus"],
        "differentAuthoritativeVersions"
    );
    assert_eq!(third["verdict"], "different");
    assert_eq!(third["coverage"], "partial");
    assert_eq!(third["displayItems"].as_array().unwrap().len(), 0);
    assert_eq!(third["unverifiedRegions"].as_array().unwrap().len(), 1);
    assert!(third["nextCursor"].is_null());
    assert_eq!(repository.file_audit_correlations.lock().unwrap().len(), 4);
    assert_eq!(
        repository
            .result_audit_correlations
            .lock()
            .unwrap()
            .iter()
            .filter(|correlation| correlation.is_some())
            .count(),
        3
    );
}

fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
