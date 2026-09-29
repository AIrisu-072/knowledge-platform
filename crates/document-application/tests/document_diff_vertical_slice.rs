use std::io::Cursor;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use document_application::{
    ApplicationError, AuditedFileGrant, ContentReader, FileStorage, RepositoryError,
    SemanticInspectionRecord, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
    VerifiedActorContext, VersionFileAccessRepository, VersionFileRequest,
    document_diff::{
        ComparisonRowState, DiffCache, DiffCacheKey, DiffExecutionError, DiffExecutor,
        DiffInspectionEvidence, DiffPairSnapshot, DiffRequest, DiffResult, DocumentDiffRepository,
        DocumentDiffService, SnapshotItem, VersionSnapshot,
    },
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, SourceLocator, WorkerDiffRequest,
    WorkerDiffResponse,
};
use document_diff_worker::run_worker_shell;
use document_domain::{DocumentId, DocumentVersionId, FileId, MediaType, StorageKey};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const BASE: &[u8] = b"header\nold value\nfooter\n";
const TARGET: &[u8] = b"header\nnew value\nfooter\n";

fn pair() -> DiffPairSnapshot {
    let document_id = DocumentId::from_uuid(Uuid::from_u128(1));
    let version = |number: u128, body: &[u8], semantic: u8| VersionSnapshot {
        document_id,
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(number)),
        reference_purpose: document_application::VersionPurpose::History,
        document_revision: 1,
        title: "Title".into(),
        items: vec![SnapshotItem {
            content_item_id: Uuid::from_u128(number + 10),
            logical_path: "primary".into(),
            ordinal: 0,
            authoritative_representation_id: Uuid::from_u128(number + 20),
            file_id: FileId::from_uuid(Uuid::from_u128(number + 30)),
            format: Some(FormatId::Txt),
            inspection_profile: InspectionProfileVersion::DsiV0,
            semantic_fingerprint: Some([semantic; 32]),
            inspection_binding_digest: Some([semantic; 32]),
            raw_sha256: Sha256::digest(body).into(),
            size_bytes: body.len() as u64,
        }],
        manifest_fingerprint: [8; 32],
        version_metadata_digest: [4; 32],
    };
    DiffPairSnapshot {
        document_id,
        base: version(2, BASE, 1),
        target: version(3, TARGET, 2),
    }
}

struct Repository {
    pair: DiffPairSnapshot,
    audits: AtomicUsize,
}

impl DiffCache for Repository {
    async fn get(&self, _: &DiffCacheKey) -> Result<Option<DiffResult>, RepositoryError> {
        Ok(None)
    }
    async fn put(
        &self,
        _: DiffCacheKey,
        _: DiffResult,
        _: [u8; 32],
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
}

impl DocumentDiffRepository for Repository {
    async fn capture_pair(
        &self,
        _: &VerifiedActorContext,
        _: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError> {
        Ok(self.pair.clone())
    }
    async fn authorize_and_audit_result(
        &self,
        _: &VerifiedActorContext,
        _: &DiffPairSnapshot,
        _: &DiffResult,
        _: bool,
        _: Option<&str>,
    ) -> Result<Uuid, RepositoryError> {
        self.audits.fetch_add(1, Ordering::SeqCst);
        Ok(Uuid::from_u128(55))
    }
}

impl VersionFileAccessRepository for Repository {
    async fn authorize_and_audit_file(
        &self,
        _: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        let is_base = request.version.document_version_id == self.pair.base.version_id;
        let key = StorageKey::new(if is_base { "base" } else { "target" }).unwrap();
        let size = if is_base { BASE.len() } else { TARGET.len() };
        Ok(AuditedFileGrant::new(
            key,
            MediaType::new("text/plain").unwrap(),
            size as i64,
            "source.txt".into(),
            Uuid::from_u128(56),
        ))
    }
}

struct Storage;
impl FileStorage for Storage {
    async fn put_immutable(&self, _: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }
    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        let bytes = if key.as_str() == "base" { BASE } else { TARGET };
        Ok(Box::pin(Cursor::new(bytes.to_vec())))
    }
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}

struct WorkerExecutor;
impl DiffExecutor for WorkerExecutor {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        mut base: ContentReader,
        mut target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        let mut old = Vec::new();
        let mut new = Vec::new();
        base.read_to_end(&mut old)
            .await
            .map_err(|_| DiffExecutionError::Unavailable)?;
        target
            .read_to_end(&mut new)
            .await
            .map_err(|_| DiffExecutionError::Unavailable)?;
        run_worker_shell(
            &serde_json::to_vec(&request).unwrap(),
            Cursor::new(old),
            Cursor::new(new),
        )
        .map_err(|_| DiffExecutionError::InvalidWorkerResult)
    }
}

struct Inspection;
impl DiffInspectionEvidence for Inspection {
    async fn ensure(
        &self,
        _: FileId,
        _: InspectionProfileVersion,
    ) -> Result<SemanticInspectionRecord, ApplicationError> {
        unreachable!("both snapshots already carry qualified inspection evidence")
    }
}

fn actor() -> VerifiedActorContext {
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

#[tokio::test]
async fn real_txt_worker_change_reaches_authorized_table_with_both_original_spans() {
    let repository = Arc::new(Repository {
        pair: pair(),
        audits: AtomicUsize::new(0),
    });
    let service = DocumentDiffService::new(
        repository.clone(),
        Arc::new(Storage),
        Arc::new(WorkerExecutor),
        Arc::new(Inspection),
    );
    let request = DiffRequest {
        document_id: repository.pair.document_id,
        base_version_id: repository.pair.base.version_id,
        target_version_id: repository.pair.target.version_id,
        profile: DiffProfileVersion::V0,
    };
    let table = service.comparison_table(&actor(), request).await.unwrap();
    assert_eq!(table.verdict, ContentVerdict::Different);
    assert_eq!(table.coverage, DiffCoverage::Full);
    assert_eq!(repository.audits.load(Ordering::SeqCst), 1);
    let row = table
        .rows
        .iter()
        .find(|row| row.state == ComparisonRowState::Confirmed && row.facet == "text")
        .unwrap();
    let old = row.base.as_ref().unwrap();
    let new = row.target.as_ref().unwrap();
    assert_eq!(old.version_id, repository.pair.base.version_id);
    assert_eq!(new.version_id, repository.pair.target.version_id);
    assert_eq!(old.file_id, repository.pair.base.items[0].file_id);
    assert_eq!(new.file_id, repository.pair.target.items[0].file_id);
    assert_eq!(old.raw_sha256, <[u8; 32]>::from(Sha256::digest(BASE)));
    assert_eq!(new.raw_sha256, <[u8; 32]>::from(Sha256::digest(TARGET)));
    assert!(matches!(
        old.locator,
        SourceLocator::TextSpan { line: 2, .. }
    ));
    assert!(matches!(
        new.locator,
        SourceLocator::TextSpan { line: 2, .. }
    ));
}
