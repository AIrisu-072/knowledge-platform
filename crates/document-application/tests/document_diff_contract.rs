use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use document_application::{
    ApplicationError, AuditedFileGrant, ContentReader, FileStorage, InspectionExecutionError,
    RepositoryError, SemanticInspectionRecord, StorageError, StorageObjectInfo, StoreFileRequest,
    StoredFile, VerifiedActorContext, VersionFileAccessRepository, VersionFileRequest,
    document_diff::{
        DiffCache, DiffCacheKey, DiffExecutionError, DiffExecutor, DiffInspectionEvidence,
        DiffPairSnapshot, DiffRequest, DocumentDiffRepository, DocumentDiffService, SnapshotItem,
        VersionSnapshot,
    },
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, SourceLocator, UnverifiedReason,
    WorkerDiffRequest, WorkerDiffResponse, WorkerProtocolVersion, WorkerUnverifiedRegion,
};
use document_domain::{DocumentId, DocumentVersionId, FileId, MediaType, StorageKey};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use uuid::Uuid;

fn pair() -> DiffPairSnapshot {
    let document_id = DocumentId::from_uuid(Uuid::from_u128(1));
    let item = |number: u128, raw: u8| SnapshotItem {
        content_item_id: Uuid::from_u128(number + 10),
        logical_path: "primary".into(),
        ordinal: 0,
        authoritative_representation_id: Uuid::from_u128(number + 20),
        file_id: FileId::from_uuid(Uuid::from_u128(number + 30)),
        format: Some(FormatId::Txt),
        inspection_profile: InspectionProfileVersion::DsiV0,
        semantic_fingerprint: Some([8; 32]),
        inspection_binding_digest: Some([9; 32]),
        raw_sha256: [raw; 32],
        size_bytes: 4,
    };
    let version = |number: u128, raw: u8| VersionSnapshot {
        document_id,
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(number)),
        reference_purpose: document_application::VersionPurpose::History,
        document_revision: 1,
        title: "Title".into(),
        items: vec![item(number, raw)],
        manifest_fingerprint: [8; 32],
        version_metadata_digest: [4; 32],
    };
    DiffPairSnapshot {
        document_id,
        base: version(2, 1),
        target: version(3, 2),
    }
}

struct FakeRepository {
    pair: DiffPairSnapshot,
    audits: AtomicUsize,
    deny_final: bool,
    cache: Mutex<
        Option<(
            DiffCacheKey,
            document_application::document_diff::DiffResult,
        )>,
    >,
    cache_hits: Mutex<Vec<bool>>,
}

impl DiffCache for FakeRepository {
    async fn get(
        &self,
        key: &DiffCacheKey,
    ) -> Result<Option<document_application::document_diff::DiffResult>, RepositoryError> {
        Ok(self
            .cache
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(saved, result)| (saved == key).then(|| result.clone())))
    }
    async fn put(
        &self,
        key: DiffCacheKey,
        result: document_application::document_diff::DiffResult,
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
        Ok(self.pair.clone())
    }
    async fn authorize_and_audit_result(
        &self,
        _actor: &VerifiedActorContext,
        _pair: &DiffPairSnapshot,
        _result: &document_application::document_diff::DiffResult,
        cache_hit: bool,
        _correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError> {
        if self.deny_final {
            return Err(RepositoryError::Forbidden);
        }
        self.audits.fetch_add(1, Ordering::SeqCst);
        self.cache_hits.lock().unwrap().push(cache_hit);
        Ok(Uuid::from_u128(55))
    }
}

impl VersionFileAccessRepository for FakeRepository {
    async fn authorize_and_audit_file(
        &self,
        _actor: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        let key = StorageKey::new(
            if request.version.document_version_id == self.pair.base.version_id {
                "base"
            } else {
                "target"
            },
        )
        .unwrap();
        Ok(AuditedFileGrant::new(
            key,
            MediaType::new("text/plain").unwrap(),
            4,
            "source.txt".into(),
            Uuid::from_u128(56),
        ))
    }
}

struct FakeStorage;
impl FileStorage for FakeStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }
    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(std::io::Cursor::new(b"text".to_vec())))
    }
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}

struct FakeExecutor {
    unsupported: bool,
}
impl DiffExecutor for FakeExecutor {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        _base: ContentReader,
        _target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        Ok(WorkerDiffResponse {
            protocol_version: WorkerProtocolVersion::V0,
            diff_profile_version: request.diff_profile_version,
            resource_profile_version: request.resource_profile_version,
            base_raw_sha256: request.base_raw_sha256,
            base_size_bytes: request.base_size_bytes,
            target_raw_sha256: request.target_raw_sha256,
            target_size_bytes: request.target_size_bytes,
            format: request.format,
            coverage: if self.unsupported {
                DiffCoverage::None
            } else {
                DiffCoverage::Full
            },
            changes: vec![],
            unverified_regions: if self.unsupported {
                vec![WorkerUnverifiedRegion {
                    base: Some(SourceLocator::ContentItem),
                    target: Some(SourceLocator::ContentItem),
                    reason: UnverifiedReason::UnsupportedSemanticConstruct,
                    navigation_hint: Some("原本を確認".into()),
                }]
            } else {
                vec![]
            },
            parser_provenance: "fake-qualified-txt".into(),
        })
    }
}

struct MissingEvidence;
impl DiffInspectionEvidence for MissingEvidence {
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

fn context() -> VerifiedActorContext {
    use document_application::InvocationKind;
    use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
    let principal = PrincipalRef::new("test", "reader").unwrap();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test", "reader").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        time::OffsetDateTime::now_utc() + time::Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn request(pair: &DiffPairSnapshot) -> DiffRequest {
    DiffRequest {
        document_id: pair.document_id,
        base_version_id: pair.base.version_id,
        target_version_id: pair.target.version_id,
        profile: DiffProfileVersion::V0,
    }
}

#[tokio::test]
async fn equal_semantics_with_different_raw_bytes_stays_same_and_is_audited() {
    let repository = Arc::new(FakeRepository {
        pair: pair(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service
        .compare(&context(), request(&repository.pair))
        .await
        .unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Same);
    assert_eq!(output.result.coverage, DiffCoverage::Full);
    assert_eq!(repository.audits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_inspection_and_unqualified_worker_never_claim_same() {
    let mut source = pair();
    source.target.items[0].semantic_fingerprint = None;
    source.target.items[0].inspection_binding_digest = None;
    let repository = Arc::new(FakeRepository {
        pair: source,
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: true }),
        Arc::new(MissingEvidence),
    );
    let output = service
        .compare(&context(), request(&repository.pair))
        .await
        .unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Unknown);
    assert!(!output.result.unverified_regions.is_empty());
}

#[tokio::test]
async fn cross_format_has_confirmed_format_change_and_unverified_detail() {
    let mut source = pair();
    source.target.items[0].format = Some(FormatId::Html);
    let repository = Arc::new(FakeRepository {
        pair: source,
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service
        .compare(&context(), request(&repository.pair))
        .await
        .unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Different);
    assert_eq!(output.result.coverage, DiffCoverage::Partial);
    assert!(!output.result.changes.is_empty());
    assert!(!output.result.unverified_regions.is_empty());
}

#[tokio::test]
async fn metadata_only_change_does_not_change_content_verdict_and_final_denial_returns_no_result() {
    let mut source = pair();
    source.target.version_metadata_digest = [5; 32];
    let repository = Arc::new(FakeRepository {
        pair: source.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service.compare(&context(), request(&source)).await.unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Same);
    assert_eq!(output.result.ancillary_changes.len(), 1);
    let denied = Arc::new(FakeRepository {
        pair: source.clone(),
        audits: AtomicUsize::new(0),
        deny_final: true,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        denied.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    assert!(matches!(
        service.compare(&context(), request(&source)).await,
        Err(ApplicationError::Forbidden)
    ));
    assert_eq!(denied.audits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_fingerprint_without_worker_locator_stays_different_with_unverified_detail() {
    let mut source = pair();
    source.target.items[0].semantic_fingerprint = Some([10; 32]);
    let repository = Arc::new(FakeRepository {
        pair: source.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository,
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service.compare(&context(), request(&source)).await.unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Different);
    assert_eq!(output.result.coverage, DiffCoverage::Partial);
    assert_eq!(output.result.changes[0].facet, "content_item");
    assert!(!output.result.unverified_regions.is_empty());
}

#[tokio::test]
async fn ambiguous_item_correspondence_is_not_invented_as_add_remove_or_move() {
    let mut source = pair();
    let mut old_extra = source.base.items[0].clone();
    old_extra.content_item_id = Uuid::from_u128(101);
    old_extra.logical_path = "old/second".into();
    old_extra.ordinal = 1;
    source.base.items[0].logical_path = "old/first".into();
    source.base.items.push(old_extra);
    source.target.items[0].logical_path = "new/first".into();
    let mut new_extra = source.target.items[0].clone();
    new_extra.content_item_id = Uuid::from_u128(102);
    new_extra.logical_path = "new/second".into();
    new_extra.ordinal = 1;
    new_extra.semantic_fingerprint = Some([11; 32]);
    source.target.items.push(new_extra);
    let repository = Arc::new(FakeRepository {
        pair: source.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository,
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service.compare(&context(), request(&source)).await.unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Unknown);
    assert!(output.result.changes.is_empty());
    assert!(
        output
            .result
            .unverified_regions
            .iter()
            .all(|region| region.reason == UnverifiedReason::AmbiguousAlignment)
    );
}

#[tokio::test]
async fn unique_reorder_and_one_sided_manifest_addition_are_confirmed() {
    let mut reordered = pair();
    reordered.target.items[0].ordinal = 2;
    let repository = Arc::new(FakeRepository {
        pair: reordered.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository,
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service
        .compare(&context(), request(&reordered))
        .await
        .unwrap();
    assert_eq!(output.result.verdict, ContentVerdict::Different);
    assert_eq!(output.result.coverage, DiffCoverage::Full);
    assert!(
        output
            .result
            .changes
            .iter()
            .any(|change| change.relocation == Some(document_diff_core::RelocationKind::Reordered))
    );

    let mut added = pair();
    let mut extra = added.target.items[0].clone();
    extra.content_item_id = Uuid::from_u128(300);
    extra.authoritative_representation_id = Uuid::from_u128(301);
    extra.file_id = FileId::from_uuid(Uuid::from_u128(302));
    extra.logical_path = "appendix".into();
    extra.ordinal = 1;
    added.target.items.push(extra);
    let repository = Arc::new(FakeRepository {
        pair: added.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository,
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    let output = service.compare(&context(), request(&added)).await.unwrap();
    assert!(output.result.changes.iter().any(|change| change.operation
        == Some(document_diff_core::ChangeOperation::Added)
        && change.base.is_none()
        && change.target.is_some()));
}

#[tokio::test]
async fn cache_hit_and_table_retrieval_each_require_fresh_final_audit() {
    let source = pair();
    let repository = Arc::new(FakeRepository {
        pair: source.clone(),
        audits: AtomicUsize::new(0),
        deny_final: false,
        cache: Mutex::new(None),
        cache_hits: Mutex::new(vec![]),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(FakeStorage),
        Arc::new(FakeExecutor { unsupported: false }),
        Arc::new(MissingEvidence),
    );
    service.compare(&context(), request(&source)).await.unwrap();
    service.compare(&context(), request(&source)).await.unwrap();
    let table = service
        .comparison_table(&context(), request(&source))
        .await
        .unwrap();
    assert_eq!(table.verdict, ContentVerdict::Same);
    assert_eq!(repository.audits.load(Ordering::SeqCst), 3);
    assert_eq!(
        &*repository.cache_hits.lock().unwrap(),
        &[false, true, true]
    );
}
