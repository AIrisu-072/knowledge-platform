//! Shared fixtures for P1 body extraction and manifest tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use document_application::{
    ContentReader, FileStorage, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, FolderId, LifecycleState, StorageKey, Title,
};
use search_core::id::SourceId;
use search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, FormatId, FormatSettings, RawBinding,
};
use search_extraction_core::{
    BodyCoverage, ContentExtractor, ExtractionError, PermanentFailureCode, ReaderFailure,
    RegisteredProfile, RetryableFailureCode, WorkerFragment, WorkerOperation, WorkerReport,
    WorkerRequest, WorkerResponse, validate_worker_report,
};
use search_source_document::{
    AuthoritativeItemBinding, BodyProfileRegistry, DocumentAccessProjectionInput,
    DocumentBodyExtractor, DocumentSourceSnapshot, DsiReadState, PermittedDocumentMetadata,
    VersionSnapshotRecord,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "objects/synthetic-body";

pub fn limits(format: FormatId) -> BTreeMap<BudgetKey, u64> {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1_048_576);
    limits.insert(BudgetKey::Units, 10_000);
    limits.insert(BudgetKey::UnitUtf8Bytes, 65_536);
    limits.insert(BudgetKey::WorkerOutputBytes, 16_777_216);
    if matches!(
        format,
        FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Zip
    ) {
        limits.insert(BudgetKey::ZipEntries, 100);
        limits.insert(BudgetKey::ZipEntryBytes, 1_048_576);
        limits.insert(BudgetKey::ZipTotalBytes, 4_194_304);
        limits.insert(
            BudgetKey::ZipDepth,
            if format == FormatId::Zip { 3 } else { 1 },
        );
        limits.insert(BudgetKey::XmlDepth, 128);
        limits.insert(BudgetKey::XmlNodes, 100_000);
    }
    if matches!(format, FormatId::Pdf | FormatId::Zip) {
        limits.insert(BudgetKey::PdfPages, 16);
        limits.insert(BudgetKey::PdfOperations, 10_000);
    }
    if matches!(format, FormatId::Html | FormatId::Zip) {
        limits.insert(BudgetKey::HtmlNodes, 10_000);
    }
    if matches!(format, FormatId::Csv | FormatId::Zip) {
        limits.insert(BudgetKey::CsvRecords, 10_000);
        limits.insert(BudgetKey::CsvFieldBytes, 65_536);
    }
    limits
}

pub fn definition(format: FormatId) -> ExtractionProfileDefinitionV1 {
    ExtractionProfileDefinitionV1 {
        format,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [3; 32],
        native_binary_sha256: (format == FormatId::Pdf).then_some([4; 32]),
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: match format {
            FormatId::Text => FormatSettings::Text {
                charset: "utf-8".into(),
            },
            FormatId::Csv => FormatSettings::Csv {
                charset: "utf-8".into(),
                delimiter: b',',
                quote: b'"',
            },
            FormatId::Zip => FormatSettings::Archive {
                member_decoder: "utf-8".into(),
            },
            _ => FormatSettings::None,
        },
        limits: limits(format),
    }
}

pub fn registry() -> BodyProfileRegistry {
    BodyProfileRegistry::new(
        "search-extraction-worker-test",
        [
            FormatId::Docx,
            FormatId::Xlsx,
            FormatId::Xlsm,
            FormatId::Pptx,
            FormatId::Pdf,
            FormatId::Text,
            FormatId::Csv,
            FormatId::Html,
            FormatId::Zip,
        ]
        .into_iter()
        .map(definition)
        .collect(),
    )
    .unwrap()
}

/// Returns each queued byte image once per `open`, then repeats the last one.
#[derive(Default)]
pub struct SequencedStorage {
    images: Mutex<Vec<Vec<u8>>>,
    opens: AtomicUsize,
}

impl SequencedStorage {
    pub fn with(images: Vec<Vec<u8>>) -> Self {
        Self {
            images: Mutex::new(images),
            opens: AtomicUsize::new(0),
        }
    }
}

impl FileStorage for SequencedStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!("read-only fixture")
    }

    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        assert_eq!(key.as_str(), KEY);
        let index = self.opens.fetch_add(1, Ordering::SeqCst);
        let images = self.images.lock().unwrap();
        let bytes = images
            .get(index)
            .or_else(|| images.last())
            .cloned()
            .ok_or(StorageError::Unavailable)?;
        Ok(Box::pin(Cursor::new(bytes)))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Honest,
    TamperResolve,
    Permanent,
    Retryable,
}

/// Runs the real qualified readers in-process; the sealed process boundary is
/// covered by the runner isolation tests on hosted Linux.
pub struct InProcessExtractor {
    pub mode: Mode,
    pub requests: Mutex<Vec<WorkerRequest>>,
}

impl InProcessExtractor {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn run(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError> {
        self.requests.lock().unwrap().push(request.clone());
        match self.mode {
            Mode::Permanent => {
                return Err(ExtractionError::Permanent(
                    PermanentFailureCode::CorruptDocument,
                ));
            }
            Mode::Retryable => {
                return Err(ExtractionError::Retryable(RetryableFailureCode::Timeout));
            }
            Mode::Honest | Mode::TamperResolve => {}
        }
        match search_extraction_worker::readers::respond(&request, raw, profile.archive_plan()) {
            WorkerResponse::Report(report) => {
                validate_worker_report(&report, profile)?;
                Ok(report)
            }
            WorkerResponse::Failure(ReaderFailure::Unsupported(reason)) => Ok(WorkerReport {
                coverage: BodyCoverage::Unsupported { reason },
                fragments: Vec::new(),
                reader_use: Vec::new(),
                scope_items: 0,
                known_omissions: Vec::new(),
                traversal_complete: false,
            }),
            WorkerResponse::Failure(ReaderFailure::Permanent(code)) => {
                Err(ExtractionError::Permanent(code))
            }
            WorkerResponse::Failure(ReaderFailure::Retryable(code)) => {
                Err(ExtractionError::Retryable(code))
            }
        }
    }
}

impl ContentExtractor for InProcessExtractor {
    fn extract(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError> {
        assert!(matches!(request.operation, WorkerOperation::Extract));
        self.run(raw, request, profile)
    }

    fn resolve_locators(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<Vec<WorkerFragment>, ExtractionError> {
        assert!(matches!(
            request.operation,
            WorkerOperation::ResolveLocators(_)
        ));
        let mut fragments = self.run(raw, request, profile)?.fragments;
        if self.mode == Mode::TamperResolve
            && let Some(first) = fragments.first_mut()
        {
            first.text.push('!');
        }
        Ok(fragments)
    }
}

pub fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(500))
}

pub fn record() -> VersionSnapshotRecord {
    let at = |second| OffsetDateTime::from_unix_timestamp(second).unwrap();
    VersionSnapshotRecord {
        snapshot: DocumentSourceSnapshot {
            source_snapshot: "document-snapshot-body".into(),
            document_id: DocumentId::from_uuid(Uuid::from_u128(10)),
            document_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(20)),
            current_version_id: Some(DocumentVersionId::from_uuid(Uuid::from_u128(20))),
            publication_end: None,
            lifecycle_state: LifecycleState::Published,
            title: Title::new("本文試験").unwrap(),
            metadata: PermittedDocumentMetadata {
                document_type: None,
                category: None,
            },
            folder_id: FolderId::from_uuid(Uuid::from_u128(30)),
            created_at: at(100),
            published_at: Some(at(200)),
            withdrawn_at: None,
            effective_from: None,
            effective_to: None,
            access: DocumentAccessProjectionInput { access_scope: None },
            dsi: None,
        },
        document_revision: 1,
        access_revision: 1,
        dsi_state: DsiReadState::UnknownMissing,
        authoritative_items: Vec::new(),
    }
}

pub fn item(raw: &[u8], media_type: &str) -> AuthoritativeItemBinding {
    let content_item_id = Uuid::from_u128(40);
    AuthoritativeItemBinding {
        content_item_id,
        part: ContentPartRef {
            source_native_part_id: content_item_id.to_string(),
            logical_path: "本文/primary".into(),
            ordinal: 0,
        },
        representation_id: Uuid::from_u128(41),
        file_id: FileId::from_uuid(Uuid::from_u128(42)),
        raw: RawBinding {
            sha256: Sha256::digest(raw).into(),
            size_bytes: raw.len() as u64,
            media_type: media_type.into(),
        },
        storage_key: StorageKey::new(KEY).unwrap(),
    }
}

pub fn extractor(
    images: Vec<Vec<u8>>,
    mode: Mode,
) -> DocumentBodyExtractor<SequencedStorage, InProcessExtractor> {
    DocumentBodyExtractor::new(
        source_id(),
        SequencedStorage::with(images),
        InProcessExtractor::new(mode),
        registry(),
    )
}

pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        // A trailing `/` writes an explicit directory entry.
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
            continue;
        }
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub fn body_requests(
    body: &DocumentBodyExtractor<SequencedStorage, InProcessExtractor>,
) -> Vec<WorkerRequest> {
    body.extractor().requests.lock().unwrap().clone()
}
