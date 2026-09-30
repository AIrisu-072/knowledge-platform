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
    ApplicationError, AuditedFileGrant, ContentReader, FileStorage, IdentityResolutionError,
    InspectionExecutionError, InvocationKind, RepositoryError, SemanticInspectionRecord,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile, VerifiedActorContext,
    VersionFileAccessRepository, VersionFileRequest, VersionPurpose,
};
use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest, WorkerDiffResponse,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
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
    capture_error: Option<RepositoryError>,
    deny_final: bool,
    audit_cache_hits: Mutex<Vec<bool>>,
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
        _correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError> {
        if self.deny_final {
            return Err(RepositoryError::Forbidden);
        }
        self.audit_cache_hits.lock().unwrap().push(cache_hit);
        Ok(id(0xa010, 1))
    }
}

impl VersionFileAccessRepository for FakeRepository {
    async fn authorize_and_audit_file(
        &self,
        _ctx: &VerifiedActorContext,
        _request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        unreachable!("cache-backed HTTP contract must not open source files")
    }
}

struct UnusedStorage;

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
    let version = |lane: u16, raw: u8| VersionSnapshot {
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
            raw_sha256: [raw; 32],
            size_bytes: 12,
        }],
        manifest_fingerprint: [7; 32],
        version_metadata_digest: [6; 32],
    };
    DiffPairSnapshot {
        document_id,
        base: version(0xa110, 1),
        target: version(0xa120, 2),
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
    let pair = pair();
    let key = DiffCacheKey::from_pair(&pair, DiffProfileVersion::V0, ResourceProfileVersion::V0);
    let repository = Arc::new(FakeRepository {
        pair,
        cache: Mutex::new(Some((key, result))),
        capture_error,
        deny_final,
        audit_cache_hits: Mutex::new(Vec::new()),
    });
    let router = diff_router(
        repository.clone(),
        Arc::new(UnusedStorage),
        Arc::new(UnusedExecutor),
        Arc::new(UnusedInspection),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    (repository, router)
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
                        "projection": projection
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

fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
