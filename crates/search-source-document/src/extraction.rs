//! P1-S02: Source-owned raw read, trusted extraction profile registry and host
//! verification of untrusted worker output before any `KnowledgeUnit` exists.
//!
//! The worker never receives Source, actor, item or storage identities. The host
//! re-reads raw bytes before and after the worker, re-validates the report,
//! proves every locator by a second native resolution and only then derives
//! Source-owned Unit identity and provenance.

use std::io::Cursor;

use document_application::{ContentReader, FileStorage, StorageError};
use search_core::id::SourceId;
use search_core::knowledge_unit::{
    ArchiveProfilePlan, ArchiveReaderNode, BudgetKey, ExtractionProfileDefinitionV1,
    ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator, UnitAuthorityBinding, UnitId,
    UnitProvenance, text_sha256, validate_archive_member, validate_part_units,
};
use search_extraction_core::{
    BodyCoverage, ContentExtractor, CoverageReason, ExtractionError, ItemOperationState,
    PermanentFailureCode, RegisteredProfile, RetryableFailureCode, WorkerOperation, WorkerReport,
    WorkerRequest, validate_worker_report,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::body_manifest::version_ref;
use crate::model::AuthoritativeItemBinding;
use crate::postgres::VersionSnapshotRecord;

/// Publication-safe outcome for one authoritative item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedItemResult {
    pub operation: ItemOperationState,
    pub coverage: Option<BodyCoverage>,
    pub units: Vec<KnowledgeUnit>,
    /// `None` when the declared media type is outside every registered format.
    pub detected_format: Option<FormatId>,
    pub profile: Option<ExtractionProfileId>,
    /// The host-built composite plan for a ZIP item.
    pub archive_plan: Option<ArchiveProfilePlan>,
}

impl ExtractedItemResult {
    fn unsupported(
        format: Option<FormatId>,
        profile: Option<ExtractionProfileId>,
        reason: CoverageReason,
    ) -> Self {
        Self {
            operation: ItemOperationState::Completed,
            coverage: Some(BodyCoverage::Unsupported { reason }),
            units: Vec::new(),
            detected_format: format,
            profile,
            archive_plan: None,
        }
    }

    fn failed(
        format: Option<FormatId>,
        profile: Option<ExtractionProfileId>,
        code: PermanentFailureCode,
    ) -> Self {
        Self {
            operation: ItemOperationState::FailedPermanent { code },
            coverage: None,
            units: Vec::new(),
            detected_format: format,
            profile,
            archive_plan: None,
        }
    }
}

/// Generation build failures. None of these may be staged as an item outcome.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BodyBuildError {
    #[error("retryable body build failure: {0:?}")]
    Retryable(RetryableFailureCode),
    #[error("body integrity incident: {0}")]
    Integrity(&'static str),
    #[error("body configuration incident: {0}")]
    Configuration(&'static str),
}

const OLE_MAGIC: &[u8] = &[0xd0, 0xcf, 0x11, 0xe0];

enum Planned {
    Profile(Box<RegisteredProfile>),
    Unsupported(CoverageReason),
    Failed(PermanentFailureCode),
}

/// Trusted registry: one qualified definition per format and the worker build ID.
pub struct BodyProfileRegistry {
    parser_build_id: String,
    definitions: Vec<(FormatId, ExtractionProfileDefinitionV1)>,
    profiles: Vec<(FormatId, RegisteredProfile)>,
}

/// The pre-admission decision for one item.
enum Admission {
    Read,
    Refused(Option<FormatId>, CoverageReason),
}

/// The format a declared lowercase MIME essence names.
fn declared_format(media_type: &str) -> Option<FormatId> {
    Some(match media_type {
        "text/plain" => FormatId::Text,
        "text/csv" => FormatId::Csv,
        "text/html" => FormatId::Html,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => FormatId::Docx,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => FormatId::Xlsx,
        "application/vnd.ms-excel.sheet.macroenabled.12" => FormatId::Xlsm,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            FormatId::Pptx
        }
        "application/pdf" => FormatId::Pdf,
        "application/zip" | "application/x-zip-compressed" => FormatId::Zip,
        _ => return None,
    })
}

impl BodyProfileRegistry {
    pub fn new(
        parser_build_id: impl Into<String>,
        definitions: Vec<ExtractionProfileDefinitionV1>,
    ) -> Result<Self, BodyBuildError> {
        let parser_build_id = parser_build_id.into();
        if parser_build_id.is_empty()
            || !parser_build_id.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(BodyBuildError::Configuration("parser build ID"));
        }
        let mut registered = Vec::new();
        let mut profiles = Vec::new();
        for definition in definitions {
            if registered
                .iter()
                .any(|(format, _): &(FormatId, _)| *format == definition.format)
            {
                return Err(BodyBuildError::Configuration("duplicate format definition"));
            }
            if definition.format == FormatId::Zip {
                // ZIP has no single-format profile to validate it; the host
                // itself reads these budgets before any worker runs.
                if [BudgetKey::InputBytes, BudgetKey::ZipEntries]
                    .iter()
                    .any(|key| !definition.limits.contains_key(key))
                {
                    return Err(BodyBuildError::Configuration("ZIP budgets"));
                }
            } else {
                profiles.push((
                    definition.format,
                    RegisteredProfile::register_definition(definition.clone())
                        .map_err(|_| BodyBuildError::Configuration("format profile"))?,
                ));
            }
            registered.push((definition.format, definition));
        }
        Ok(Self {
            parser_build_id,
            definitions: registered,
            profiles,
        })
    }

    pub fn parser_build_id(&self) -> &str {
        &self.parser_build_id
    }

    fn definition(&self, format: FormatId) -> Option<&ExtractionProfileDefinitionV1> {
        self.definitions
            .iter()
            .find(|(candidate, _)| *candidate == format)
            .map(|(_, definition)| definition)
    }

    /// Pre-admission from the raw binding alone, before any byte is read: an
    /// unknown or unregistered format is unsupported, and an object larger
    /// than its format's input budget is resource-limited. A registered
    /// format without an input budget is a configuration error, never an
    /// unbounded read.
    fn admit(&self, media_type: &str, size_bytes: u64) -> Result<Admission, BodyBuildError> {
        let Some((format, definition)) = declared_format(media_type).and_then(|format| {
            self.definition(format)
                .map(|definition| (format, definition))
        }) else {
            return Ok(Admission::Refused(None, CoverageReason::UnsupportedFormat));
        };
        let limit =
            *definition
                .limits
                .get(&BudgetKey::InputBytes)
                .ok_or(BodyBuildError::Configuration(
                    "format without an input budget",
                ))?;
        Ok(if size_bytes > limit {
            Admission::Refused(Some(format), CoverageReason::ResourceLimit)
        } else {
            Admission::Read
        })
    }

    /// Declared lowercase MIME essence plus a magic check. A mismatch is an
    /// unsupported item, never a guess at another format.
    pub fn detect(&self, media_type: &str, raw: &[u8]) -> Result<FormatId, CoverageReason> {
        let format = declared_format(media_type).ok_or(CoverageReason::UnsupportedFormat)?;
        let zip_magic = raw.starts_with(b"PK\x03\x04") || raw.starts_with(b"PK\x05\x06");
        let consistent = match format {
            FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Zip => {
                zip_magic
            }
            FormatId::Pdf => raw.starts_with(b"%PDF-"),
            FormatId::Text | FormatId::Csv | FormatId::Html => {
                !zip_magic && !raw.starts_with(b"%PDF-") && !raw.starts_with(OLE_MAGIC)
            }
        };
        if !consistent || self.definition(format).is_none() {
            return Err(CoverageReason::UnsupportedFormat);
        }
        Ok(format)
    }

    fn plan(&self, format: FormatId, raw: &[u8]) -> Result<Planned, BodyBuildError> {
        if format != FormatId::Zip {
            let profile = self
                .profiles
                .iter()
                .find(|(candidate, _)| *candidate == format)
                .map(|(_, profile)| profile.clone())
                .ok_or(BodyBuildError::Configuration("unregistered format"))?;
            return Ok(Planned::Profile(Box::new(profile)));
        }
        self.archive_plan(raw)
    }

    /// Flat explicit ZIP plan from the central directory. Nested containers are
    /// not planned by the host and therefore stay unsupported.
    fn archive_plan(&self, raw: &[u8]) -> Result<Planned, BodyBuildError> {
        let root = self
            .definition(FormatId::Zip)
            .ok_or(BodyBuildError::Configuration("ZIP definition"))?
            .clone();
        let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(raw)) else {
            return Ok(Planned::Failed(PermanentFailureCode::MalformedArchive));
        };
        if archive.len() as u64 > root.limits[&BudgetKey::ZipEntries] {
            return Ok(Planned::Unsupported(CoverageReason::ResourceLimit));
        }
        let mut names = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let Ok(entry) = archive.by_index_raw(index) else {
                return Ok(Planned::Failed(PermanentFailureCode::MalformedArchive));
            };
            let name = entry.name().to_owned();
            // OS archivers write explicit directory entries (`docs/`); they
            // carry no content and are not planned as leaves.
            if let Some(directory) = name.strip_suffix('/') {
                if entry.size() != 0 || validate_archive_member(directory).is_err() {
                    return Ok(Planned::Unsupported(CoverageReason::UnsupportedStructure));
                }
                continue;
            }
            names.push(name);
        }
        names.sort();
        let mut nodes = vec![ArchiveReaderNode {
            members: Vec::new(),
            parser_build_id: self.parser_build_id.clone(),
            definition: root,
        }];
        let mut leaves = Vec::new();
        for name in names {
            if validate_archive_member(&name).is_err() {
                return Ok(Planned::Unsupported(CoverageReason::UnsupportedStructure));
            }
            let lower = name.to_ascii_lowercase();
            let format = match lower.rsplit_once('.').map(|(_, extension)| extension) {
                Some("txt") => FormatId::Text,
                Some("csv") => FormatId::Csv,
                Some("html" | "htm") => FormatId::Html,
                Some("docx") => FormatId::Docx,
                Some("xlsx") => FormatId::Xlsx,
                Some("xlsm") => FormatId::Xlsm,
                Some("pptx") => FormatId::Pptx,
                Some("pdf") => FormatId::Pdf,
                Some("zip") => {
                    return Ok(Planned::Unsupported(CoverageReason::UnsupportedStructure));
                }
                _ => return Ok(Planned::Unsupported(CoverageReason::UnsupportedFormat)),
            };
            let definition = self
                .definition(format)
                .ok_or(BodyBuildError::Configuration("leaf definition"))?
                .clone();
            nodes.push(ArchiveReaderNode {
                members: vec![name.clone()],
                parser_build_id: self.parser_build_id.clone(),
                definition,
            });
            leaves.push(vec![name]);
        }
        if leaves.is_empty() {
            return Ok(Planned::Unsupported(CoverageReason::UnsupportedStructure));
        }
        let plan = ArchiveProfilePlan {
            nodes,
            used_leaf_chains: leaves,
        };
        RegisteredProfile::register_archive(plan)
            .map(|profile| Planned::Profile(Box::new(profile)))
            .map_err(|_| BodyBuildError::Configuration("archive profile"))
    }
}

/// Host-side body extraction for one Document Source.
pub struct DocumentBodyExtractor<F, E> {
    source_id: SourceId,
    storage: F,
    extractor: E,
    registry: BodyProfileRegistry,
}

impl<F: FileStorage, E: ContentExtractor> DocumentBodyExtractor<F, E> {
    pub fn new(
        source_id: SourceId,
        storage: F,
        extractor: E,
        registry: BodyProfileRegistry,
    ) -> Self {
        Self {
            source_id,
            storage,
            extractor,
            registry,
        }
    }

    pub fn registry(&self) -> &BodyProfileRegistry {
        &self.registry
    }

    pub fn extractor(&self) -> &E {
        &self.extractor
    }

    pub async fn extract_item(
        &self,
        snapshot: &VersionSnapshotRecord,
        item: &AuthoritativeItemBinding,
    ) -> Result<ExtractedItemResult, BodyBuildError> {
        // Nothing is buffered or parsed by the host before admission.
        if let Admission::Refused(format, reason) = self
            .registry
            .admit(&item.raw.media_type, item.raw.size_bytes)?
        {
            return Ok(ExtractedItemResult::unsupported(format, None, reason));
        }
        let raw = self.read_raw(item).await?;
        let format = match self.registry.detect(&item.raw.media_type, &raw) {
            Ok(format) => format,
            Err(reason) => return Ok(ExtractedItemResult::unsupported(None, None, reason)),
        };
        let profile = match self.registry.plan(format, &raw)? {
            Planned::Profile(profile) => *profile,
            Planned::Unsupported(reason) => {
                return Ok(ExtractedItemResult::unsupported(Some(format), None, reason));
            }
            Planned::Failed(code) => {
                return Ok(ExtractedItemResult::failed(Some(format), None, code));
            }
        };
        let profile_id = profile.id().clone();
        if item.raw.size_bytes > profile.budgets().get(BudgetKey::InputBytes) {
            return Ok(ExtractedItemResult::unsupported(
                Some(format),
                Some(profile_id),
                CoverageReason::ResourceLimit,
            ));
        }
        let request = WorkerRequest {
            operation: WorkerOperation::Extract,
            format,
            profile: profile_id.clone(),
            profile_bytes: profile.profile_bytes().to_vec(),
            expected_raw: item.raw.clone(),
            budgets: profile.budgets().clone(),
        };
        let report = match self.extractor.extract(&raw, request.clone(), &profile) {
            Ok(report) => report,
            Err(ExtractionError::Permanent(code)) => {
                self.verify_unchanged(item).await?;
                return Ok(ExtractedItemResult::failed(
                    Some(format),
                    Some(profile_id),
                    code,
                ));
            }
            Err(error) => return Err(build_error(error)),
        };
        validate_worker_report(&report, &profile)
            .map_err(|_| BodyBuildError::Integrity("worker report"))?;
        self.verify_unchanged(item).await?;
        let coverage = report.coverage.clone();
        if let BodyCoverage::Unsupported { reason } = coverage {
            return Ok(ExtractedItemResult::unsupported(
                Some(format),
                Some(profile_id),
                reason,
            ));
        }
        if !report.fragments.is_empty() {
            let locators = report
                .fragments
                .iter()
                .map(|fragment| fragment.locator.clone())
                .collect();
            let resolve = WorkerRequest {
                operation: WorkerOperation::ResolveLocators(locators),
                ..request
            };
            let resolved = self
                .extractor
                .resolve_locators(&raw, resolve, &profile)
                .map_err(build_error)?;
            let same = resolved.len() == report.fragments.len()
                && resolved
                    .iter()
                    .zip(&report.fragments)
                    .all(|(again, first)| {
                        again.locator == first.locator
                            && again.text == first.text
                            && again.kind == first.kind
                    });
            if !same {
                return Err(BodyBuildError::Integrity("native locator round trip"));
            }
            self.verify_unchanged(item).await?;
        }
        let units = self.units(snapshot, item, format, &profile, &report)?;
        Ok(ExtractedItemResult {
            operation: ItemOperationState::Completed,
            coverage: Some(coverage),
            units,
            detected_format: Some(format),
            profile: Some(profile_id),
            archive_plan: profile.archive_plan().cloned(),
        })
    }

    fn units(
        &self,
        snapshot: &VersionSnapshotRecord,
        item: &AuthoritativeItemBinding,
        format: FormatId,
        profile: &RegisteredProfile,
        report: &WorkerReport,
    ) -> Result<Vec<KnowledgeUnit>, BodyBuildError> {
        let version = version_ref(self.source_id, snapshot);
        let binding = UnitAuthorityBinding {
            version: version.clone(),
            part: item.part.clone(),
            source_snapshot: snapshot.snapshot.source_snapshot.clone(),
            authoritative_representation_ref: item.representation_id.to_string(),
            raw: item.raw.clone(),
            detected_format: format,
            archive_inner_format: None,
            profile: profile.id().clone(),
            parser_build_id: self.registry.parser_build_id.clone(),
            archive_plan: profile.archive_plan().cloned(),
        };
        let mut units: Vec<KnowledgeUnit> = Vec::with_capacity(report.fragments.len());
        for fragment in &report.fragments {
            let archive_inner_format = match (&fragment.locator, profile.archive_plan()) {
                (NativeLocator::Archive { members, .. }, Some(plan)) => Some(
                    plan.nodes
                        .iter()
                        .find(|node| &node.members == members)
                        .map(|node| node.definition.format)
                        .ok_or(BodyBuildError::Integrity("archive leaf"))?,
                ),
                _ => None,
            };
            let parent_unit_id = match fragment.parent_ordinal {
                Some(parent) => Some(
                    units
                        .get(parent as usize)
                        .map(|unit| unit.unit_id)
                        .ok_or(BodyBuildError::Integrity("parent ordinal"))?,
                ),
                None => None,
            };
            let unit_id = UnitId::derive(
                &version,
                &item.part,
                profile.id(),
                &fragment.locator,
                fragment.ordinal,
            )
            .map_err(|_| BodyBuildError::Integrity("unit identity"))?;
            units.push(KnowledgeUnit {
                unit_id,
                version: version.clone(),
                part: item.part.clone(),
                parent_unit_id,
                ordinal: fragment.ordinal,
                kind: fragment.kind,
                text: fragment.text.clone(),
                locator: fragment.locator.clone(),
                text_sha256: text_sha256(&fragment.text),
                provenance: UnitProvenance {
                    source_snapshot: binding.source_snapshot.clone(),
                    authoritative_representation_ref: binding
                        .authoritative_representation_ref
                        .clone(),
                    raw: item.raw.clone(),
                    detected_format: format,
                    archive_inner_format,
                    profile: profile.id().clone(),
                    parser_build_id: self.registry.parser_build_id.clone(),
                },
            });
        }
        validate_part_units(&binding, &units)
            .map_err(|_| BodyBuildError::Integrity("unit authority"))?;
        Ok(units)
    }

    async fn read_raw(&self, item: &AuthoritativeItemBinding) -> Result<Vec<u8>, BodyBuildError> {
        let reader = self
            .storage
            .open(&item.storage_key)
            .await
            .map_err(storage_error)?;
        let bytes = read_bounded(reader, item.raw.size_bytes).await?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if bytes.len() as u64 != item.raw.size_bytes || digest != item.raw.sha256 {
            return Err(BodyBuildError::Integrity("raw binding"));
        }
        Ok(bytes)
    }

    async fn verify_unchanged(
        &self,
        item: &AuthoritativeItemBinding,
    ) -> Result<(), BodyBuildError> {
        self.read_raw(item).await.map(|_| ())
    }
}

async fn read_bounded(mut reader: ContentReader, expected: u64) -> Result<Vec<u8>, BodyBuildError> {
    let mut bytes = Vec::new();
    (&mut reader)
        .take(expected.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| BodyBuildError::Retryable(RetryableFailureCode::SourceIo))?;
    Ok(bytes)
}

fn storage_error(error: StorageError) -> BodyBuildError {
    match error {
        StorageError::NotFound => BodyBuildError::Integrity("raw object missing"),
        _ => BodyBuildError::Retryable(RetryableFailureCode::SourceIo),
    }
}

fn build_error(error: ExtractionError) -> BodyBuildError {
    match error {
        ExtractionError::Retryable(code) => BodyBuildError::Retryable(code),
        ExtractionError::Configuration(reason) => BodyBuildError::Configuration(reason),
        ExtractionError::Integrity(reason) => BodyBuildError::Integrity(reason),
        ExtractionError::Wire(_) => BodyBuildError::Integrity("worker wire"),
        // A permanent outcome during locator resolution contradicts the first pass.
        ExtractionError::Permanent(_) => BodyBuildError::Integrity("worker outcome changed"),
    }
}

/// Object-safe body extraction used by the outbox indexer.
pub trait BodyItemExtractor: Send + Sync {
    fn parser_build_id(&self) -> &str;

    fn extract<'a>(
        &'a self,
        record: &'a VersionSnapshotRecord,
        item: &'a AuthoritativeItemBinding,
    ) -> search_application::ports::BoxFuture<'a, ExtractedItemResult>;
}

impl<F, E> BodyItemExtractor for DocumentBodyExtractor<F, E>
where
    F: FileStorage + 'static,
    E: ContentExtractor + 'static,
{
    fn parser_build_id(&self) -> &str {
        self.registry.parser_build_id()
    }

    fn extract<'a>(
        &'a self,
        record: &'a VersionSnapshotRecord,
        item: &'a AuthoritativeItemBinding,
    ) -> search_application::ports::BoxFuture<'a, ExtractedItemResult> {
        Box::pin(async move {
            self.extract_item(record, item)
                .await
                .map_err(|error| match error {
                    BodyBuildError::Retryable(code) => {
                        search_application::SearchError::SourceUnavailable(format!(
                            "Document body extraction is retryable: {code:?}"
                        ))
                    }
                    other => search_application::SearchError::OperationFailed(format!(
                        "Document body extraction stopped: {other}"
                    )),
                })
        })
    }
}
