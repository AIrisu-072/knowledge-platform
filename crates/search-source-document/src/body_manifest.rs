//! P1-B01: canonical body Unit manifest, body coverage artifact, profile set and
//! the composite generation bundle receipt.
//!
//! Every digest and count is recomputed here from staged data. Source snapshot
//! and generation ID are matching fields, not part of the rebuild-equivalence
//! digests, so a full and an incremental rebuild of the same items agree.

use std::collections::BTreeSet;

use search_core::id::SourceId;
use search_core::knowledge_unit::{
    ArchiveProfilePlan, ContentPartRef, ExtractionProfileId, FormatId, KnowledgeUnit, RawBinding,
    ResourceVersionRef, UnitAuthorityBinding, UnitKind, validate_part_units,
};
use search_core::projection::ProjectionGenerationKey;
use search_extraction_core::{
    BodyCoverage, CoverageReason, ItemOperationState, PermanentFailureCode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::extraction::{BodyBuildError, ExtractedItemResult};
use crate::model::AuthoritativeItemBinding;
use crate::postgres::{DocumentOutboxSnapshot, VersionSnapshotRecord};

/// P1 lexical schema version carried by every body-ready bundle.
pub const LEXICAL_SCHEMA_VERSION: &str = "schema-2";

/// One Live authoritative item and its publication-safe extraction outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyItemEntry {
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    /// `None` only for a declared media type outside every registered format.
    pub detected_format: Option<FormatId>,
    pub profile: Option<ExtractionProfileId>,
    pub parser_build_id: String,
    pub archive_plan: Option<ArchiveProfilePlan>,
    pub operation: ItemOperationState,
    pub coverage: Option<BodyCoverage>,
    pub units: Vec<KnowledgeUnit>,
}

impl BodyItemEntry {
    /// Bind one extraction result to the Source-owned item it was read from.
    pub fn from_extracted(
        source_id: SourceId,
        record: &VersionSnapshotRecord,
        item: &AuthoritativeItemBinding,
        parser_build_id: &str,
        result: ExtractedItemResult,
    ) -> Self {
        Self {
            version: version_ref(source_id, record),
            part: item.part.clone(),
            authoritative_representation_ref: item.representation_id.to_string(),
            raw: item.raw.clone(),
            detected_format: result.detected_format,
            profile: result.profile,
            parser_build_id: parser_build_id.to_owned(),
            archive_plan: result.archive_plan,
            operation: result.operation,
            coverage: result.coverage,
            units: result.units,
        }
    }

    fn order_key(&self) -> (search_core::id::ResourceId, u32, &str, &str) {
        (
            self.version.resource_id,
            self.part.ordinal,
            self.part.logical_path.as_str(),
            self.part.source_native_part_id.as_str(),
        )
    }
}

/// Units belong to the Live Knowledge Resource of the exact Document version,
/// the same Resource identity the projection translator publishes.
pub(crate) fn version_ref(
    source_id: SourceId,
    record: &VersionSnapshotRecord,
) -> ResourceVersionRef {
    let version = record.snapshot.document_version_id.as_uuid();
    ResourceVersionRef {
        source_id,
        resource_id: search_core::id::ResourceId::from_uuid(version),
        source_native_version: version.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyUnitManifest {
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub entries: Vec<BodyItemEntry>,
}

impl search_tantivy::UnitSource for BodyUnitManifest {
    fn units(&self) -> Vec<&search_core::knowledge_unit::KnowledgeUnit> {
        self.entries.iter().flat_map(|entry| &entry.units).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCoverageItem {
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub operation: ItemOperationState,
    pub coverage: Option<BodyCoverage>,
    pub raw: RawBinding,
    pub unit_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCoverageArtifact {
    pub key: ProjectionGenerationKey,
    pub items: Vec<BodyCoverageItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReceipt {
    pub key: ProjectionGenerationKey,
    pub digest: [u8; 32],
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationBundleReceipt {
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub projection_digest: [u8; 32],
    pub unit_manifest: ArtifactReceipt,
    pub body_coverage: ArtifactReceipt,
    pub lexical: ArtifactReceipt,
    pub graph: ArtifactReceipt,
    pub profile_set_digest: [u8; 32],
    pub lexical_schema_version: String,
    pub composite_digest: [u8; 32],
}

fn integrity(reason: &'static str) -> BodyBuildError {
    BodyBuildError::Integrity(reason)
}

/// Every current Live authoritative item exactly once, in canonical order, with a
/// publication-safe state. Returns the coverage artifact derived from the same items.
pub fn validate_manifest(
    manifest: &BodyUnitManifest,
    snapshot: &DocumentOutboxSnapshot,
) -> Result<BodyCoverageArtifact, BodyBuildError> {
    if manifest.source_snapshot != snapshot.source_snapshot {
        return Err(integrity("manifest source snapshot"));
    }
    let source_id = manifest.key.source_id;
    let mut expected: Vec<(ResourceVersionRef, &AuthoritativeItemBinding)> = snapshot
        .live
        .iter()
        .flat_map(|record| {
            let version = version_ref(source_id, record);
            record
                .authoritative_items
                .iter()
                .map(move |item| (version.clone(), item))
        })
        .collect();
    expected.sort_by(|left, right| {
        (
            left.0.resource_id,
            left.1.part.ordinal,
            &left.1.part.logical_path,
            &left.1.part.source_native_part_id,
        )
            .cmp(&(
                right.0.resource_id,
                right.1.part.ordinal,
                &right.1.part.logical_path,
                &right.1.part.source_native_part_id,
            ))
    });
    if expected.len() != manifest.entries.len() {
        return Err(integrity("manifest item set"));
    }
    if manifest
        .entries
        .windows(2)
        .any(|pair| pair[0].order_key() >= pair[1].order_key())
    {
        return Err(integrity("manifest order"));
    }
    let mut items = Vec::with_capacity(manifest.entries.len());
    for (entry, (version, item)) in manifest.entries.iter().zip(&expected) {
        if &entry.version != version
            || entry.part != item.part
            || entry.authoritative_representation_ref != item.representation_id.to_string()
            || entry.raw != item.raw
        {
            return Err(integrity("manifest item binding"));
        }
        validate_entry(entry, &manifest.source_snapshot)?;
        items.push(BodyCoverageItem {
            version: entry.version.clone(),
            part: entry.part.clone(),
            operation: entry.operation,
            coverage: entry.coverage.clone(),
            raw: entry.raw.clone(),
            unit_count: u32::try_from(entry.units.len()).map_err(|_| integrity("unit count"))?,
        });
    }
    Ok(BodyCoverageArtifact {
        key: manifest.key,
        items,
    })
}

/// Structural check of a manifest restored from durable storage, without the
/// Source snapshot: canonical order, one Source, publication-safe outcomes and
/// full Unit authority (binding, ordinals, normalized text and its digest).
/// Returns the coverage artifact derived from the same items.
pub fn validate_restored_manifest(
    manifest: &BodyUnitManifest,
) -> Result<BodyCoverageArtifact, BodyBuildError> {
    validate_restored_manifest_skipping(manifest, |_| false)
}

/// `validate_restored_manifest` that skips the per-item checks of entries for
/// which `verified(index)` is true: items this process already validated in
/// the same content (by segment digest). Order and Source binding are always
/// checked and the coverage artifact is always derived from every entry.
pub fn validate_restored_manifest_skipping(
    manifest: &BodyUnitManifest,
    verified: impl Fn(usize) -> bool,
) -> Result<BodyCoverageArtifact, BodyBuildError> {
    if manifest
        .entries
        .windows(2)
        .any(|pair| pair[0].order_key() >= pair[1].order_key())
    {
        return Err(integrity("manifest order"));
    }
    let mut items = Vec::with_capacity(manifest.entries.len());
    for (index, entry) in manifest.entries.iter().enumerate() {
        if entry.version.source_id != manifest.key.source_id {
            return Err(integrity("manifest source"));
        }
        if !verified(index) {
            validate_entry(entry, &manifest.source_snapshot)?;
        }
        items.push(BodyCoverageItem {
            version: entry.version.clone(),
            part: entry.part.clone(),
            operation: entry.operation,
            coverage: entry.coverage.clone(),
            raw: entry.raw.clone(),
            unit_count: u32::try_from(entry.units.len()).map_err(|_| integrity("unit count"))?,
        });
    }
    Ok(BodyCoverageArtifact {
        key: manifest.key,
        items,
    })
}

fn validate_entry(entry: &BodyItemEntry, source_snapshot: &str) -> Result<(), BodyBuildError> {
    match (entry.operation, &entry.coverage) {
        (ItemOperationState::Retryable { .. }, _) => {
            return Err(integrity("retryable item is not publishable"));
        }
        (ItemOperationState::FailedPermanent { .. }, None) if entry.units.is_empty() => {}
        (ItemOperationState::FailedPermanent { .. }, _) => {
            return Err(integrity("failed item output"));
        }
        (ItemOperationState::Completed, None) => return Err(integrity("completed coverage")),
        (ItemOperationState::Completed, Some(BodyCoverage::Unsupported { .. })) => {
            if !entry.units.is_empty() {
                return Err(integrity("unsupported item units"));
            }
        }
        (ItemOperationState::Completed, Some(BodyCoverage::Partial { reasons })) => {
            if reasons.is_empty() || entry.units.is_empty() {
                return Err(integrity("partial witness"));
            }
        }
        (ItemOperationState::Completed, Some(BodyCoverage::Supported)) => {}
    }
    if entry.detected_format.is_none()
        && (entry.profile.is_some()
            || entry.coverage
                != Some(BodyCoverage::Unsupported {
                    reason: CoverageReason::UnsupportedFormat,
                }))
    {
        return Err(integrity("undetected format outcome"));
    }
    if entry.units.is_empty() {
        return Ok(());
    }
    let (Some(format), Some(profile)) = (entry.detected_format, entry.profile.clone()) else {
        return Err(integrity("units without profile"));
    };
    let binding = UnitAuthorityBinding {
        version: entry.version.clone(),
        part: entry.part.clone(),
        source_snapshot: source_snapshot.to_owned(),
        authoritative_representation_ref: entry.authoritative_representation_ref.clone(),
        raw: entry.raw.clone(),
        detected_format: format,
        archive_inner_format: None,
        profile,
        parser_build_id: entry.parser_build_id.clone(),
        archive_plan: entry.archive_plan.clone(),
    };
    validate_part_units(&binding, &entry.units).map_err(|_| integrity("unit authority"))
}

/// Length-framed canonical writer: UUID 16 bytes, integers big-endian,
/// collection counts as u32, and fixed tags for every option and enum.
#[derive(Default)]
struct Canonical(Vec<u8>);

impl Canonical {
    fn frame(&mut self, bytes: &[u8]) -> Result<(), BodyBuildError> {
        let length = u32::try_from(bytes.len()).map_err(|_| integrity("frame length"))?;
        self.0.extend_from_slice(&length.to_be_bytes());
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    fn text(&mut self, value: &str) -> Result<(), BodyBuildError> {
        self.frame(value.as_bytes())
    }

    fn count(&mut self, value: usize) -> Result<(), BodyBuildError> {
        let value = u32::try_from(value).map_err(|_| integrity("collection count"))?;
        self.0.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn tag(&mut self, value: u8) {
        self.0.push(value);
    }

    fn uuid(&mut self, value: uuid::Uuid) {
        self.0.extend_from_slice(value.as_bytes());
    }

    fn version(&mut self, version: &ResourceVersionRef) -> Result<(), BodyBuildError> {
        self.uuid(version.source_id.as_uuid());
        self.uuid(version.resource_id.as_uuid());
        self.text(&version.source_native_version)
    }

    fn part(&mut self, part: &ContentPartRef) -> Result<(), BodyBuildError> {
        self.text(&part.source_native_part_id)?;
        self.text(&part.logical_path)?;
        self.u32(part.ordinal);
        Ok(())
    }

    fn raw(&mut self, raw: &RawBinding) -> Result<(), BodyBuildError> {
        self.0.extend_from_slice(&raw.sha256);
        self.u64(raw.size_bytes);
        self.text(&raw.media_type)
    }

    fn operation(&mut self, operation: ItemOperationState) -> Result<(), BodyBuildError> {
        match operation {
            ItemOperationState::Completed => self.tag(1),
            ItemOperationState::FailedPermanent { code } => {
                self.tag(2);
                self.tag(permanent_tag(code));
            }
            ItemOperationState::Retryable { .. } => {
                return Err(integrity("retryable item is not publishable"));
            }
        }
        Ok(())
    }

    fn coverage(&mut self, coverage: &Option<BodyCoverage>) -> Result<(), BodyBuildError> {
        match coverage {
            None => self.tag(0),
            Some(BodyCoverage::Supported) => self.tag(1),
            Some(BodyCoverage::Partial { reasons }) => {
                self.tag(2);
                self.count(reasons.len())?;
                for reason in reasons {
                    self.tag(reason_tag(*reason));
                }
            }
            Some(BodyCoverage::Unsupported { reason }) => {
                self.tag(3);
                self.tag(reason_tag(*reason));
            }
        }
        Ok(())
    }

    fn digest(self, domain: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(&self.0);
        hasher.finalize().into()
    }
}

const fn format_tag(format: FormatId) -> u8 {
    match format {
        FormatId::Docx => 1,
        FormatId::Xlsx => 2,
        FormatId::Xlsm => 3,
        FormatId::Pptx => 4,
        FormatId::Pdf => 5,
        FormatId::Text => 6,
        FormatId::Csv => 7,
        FormatId::Html => 8,
        FormatId::Zip => 9,
    }
}

const fn kind_tag(kind: UnitKind) -> u8 {
    match kind {
        UnitKind::Heading => 1,
        UnitKind::Paragraph => 2,
        UnitKind::TableCell => 3,
        UnitKind::SpreadsheetCell => 4,
        UnitKind::SlideText => 5,
        UnitKind::PdfText => 6,
        UnitKind::PlainText => 7,
        UnitKind::CsvField => 8,
        UnitKind::HtmlText => 9,
    }
}

const fn reason_tag(reason: CoverageReason) -> u8 {
    match reason {
        CoverageReason::RequiresOcr => 1,
        CoverageReason::UnsupportedFormat => 2,
        CoverageReason::Encrypted => 3,
        CoverageReason::UnsupportedStructure => 4,
        CoverageReason::UnsupportedEncoding => 5,
        CoverageReason::UnsupportedDialect => 6,
        CoverageReason::UnsupportedCodec => 7,
        CoverageReason::MissingFormulaCache => 8,
        CoverageReason::AmbiguousReadingOrder => 9,
        CoverageReason::DynamicVisibility => 10,
        CoverageReason::ResourceLimit => 11,
    }
}

const fn permanent_tag(code: PermanentFailureCode) -> u8 {
    match code {
        PermanentFailureCode::CorruptDocument => 1,
        PermanentFailureCode::MalformedArchive => 2,
        PermanentFailureCode::TextExtractionFailed => 3,
        PermanentFailureCode::WorkerOutputLimit => 4,
    }
}

/// `body-unit-segment:v1`: one item's identity, raw, profile/format, parser
/// build, outcome and each Unit's ID, parent, kind, locator and text digest.
/// Text bodies and the Source snapshot are excluded, so an unchanged item has
/// the same digest in every generation.
pub fn segment_digest(entry: &BodyItemEntry) -> Result<[u8; 32], BodyBuildError> {
    let mut out = Canonical::default();
    out.text(&entry.parser_build_id)?;
    out.version(&entry.version)?;
    out.part(&entry.part)?;
    out.text(&entry.authoritative_representation_ref)?;
    out.raw(&entry.raw)?;
    match &entry.profile {
        None => out.tag(0),
        Some(profile) => {
            out.tag(1);
            out.text(profile.as_str())?;
        }
    }
    match entry.detected_format {
        None => out.tag(0),
        Some(format) => {
            out.tag(1);
            out.tag(format_tag(format));
        }
    }
    out.operation(entry.operation)?;
    out.coverage(&entry.coverage)?;
    out.count(entry.units.len())?;
    for unit in &entry.units {
        out.text(&unit.unit_id.to_string())?;
        match unit.parent_unit_id {
            None => out.tag(0),
            Some(parent) => {
                out.tag(1);
                out.text(&parent.to_string())?;
            }
        }
        out.u32(unit.ordinal);
        out.tag(kind_tag(unit.kind));
        out.frame(
            &unit
                .locator
                .encode()
                .map_err(|_| integrity("unit locator"))?,
        )?;
        out.frame(&unit.text_sha256)?;
    }
    Ok(out.digest(b"body-unit-segment:v1\0"))
}

/// `body-unit-manifest:v2`: the item count and each item's segment digest in
/// manifest order; `count` is the number of Units. A generation restored from
/// stored segments recomputes the same receipt from their digests alone.
pub fn unit_manifest_receipt(
    manifest: &BodyUnitManifest,
) -> Result<ArtifactReceipt, BodyBuildError> {
    let segments = manifest
        .entries
        .iter()
        .map(|entry| Ok((segment_digest(entry)?, entry.units.len() as u64)))
        .collect::<Result<Vec<_>, BodyBuildError>>()?;
    unit_manifest_receipt_from_segments(manifest.key, &segments)
}

/// The `body-unit-manifest:v2` receipt of ordered `(segment digest, Unit count)`.
pub fn unit_manifest_receipt_from_segments(
    key: ProjectionGenerationKey,
    segments: &[([u8; 32], u64)],
) -> Result<ArtifactReceipt, BodyBuildError> {
    let mut out = Canonical::default();
    out.count(segments.len())?;
    let mut units = 0u64;
    for (digest, count) in segments {
        out.frame(digest)?;
        units = units.checked_add(*count).ok_or(integrity("unit count"))?;
    }
    Ok(ArtifactReceipt {
        key,
        digest: out.digest(b"body-unit-manifest:v2\0"),
        count: units,
    })
}

/// `body-coverage:v1`: item identity, raw, operation, coverage and Unit count.
pub fn coverage_receipt(
    artifact: &BodyCoverageArtifact,
) -> Result<ArtifactReceipt, BodyBuildError> {
    let mut out = Canonical::default();
    out.count(artifact.items.len())?;
    for item in &artifact.items {
        out.version(&item.version)?;
        out.part(&item.part)?;
        out.raw(&item.raw)?;
        out.operation(item.operation)?;
        out.coverage(&item.coverage)?;
        out.u32(item.unit_count);
    }
    Ok(ArtifactReceipt {
        key: artifact.key,
        digest: out.digest(b"body-coverage:v1\0"),
        count: artifact.items.len() as u64,
    })
}

/// `body-profile-set:v1`: distinct `(profile ID, parser build)` pairs, ascending.
pub fn profile_set_digest(manifest: &BodyUnitManifest) -> Result<[u8; 32], BodyBuildError> {
    profile_set_digest_from(manifest.entries.iter().filter_map(|entry| {
        entry
            .profile
            .as_ref()
            .map(|profile| (profile.as_str(), entry.parser_build_id.as_str()))
    }))
}

/// [`profile_set_digest`] from each profiled item's profile and parser build.
pub fn profile_set_digest_from<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<[u8; 32], BodyBuildError> {
    let set: BTreeSet<(&str, &str)> = pairs.into_iter().collect();
    let mut out = Canonical::default();
    out.count(set.len())?;
    for (profile, build) in set {
        out.text(profile)?;
        out.text(build)?;
    }
    Ok(out.digest(b"body-profile-set:v1\0"))
}

/// Decode the projection-only `sha256:<hex>` manifest digest.
pub fn projection_digest(manifest_digest: &str) -> Result<[u8; 32], BodyBuildError> {
    let hex = manifest_digest
        .strip_prefix("sha256:")
        .filter(|hex| hex.len() == 64)
        .ok_or(integrity("projection digest"))?;
    let mut out = [0u8; 32];
    for (index, pair) in hex.as_bytes().chunks(2).enumerate() {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(integrity("projection digest")),
        };
        out[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(out)
}

/// Recompute every artifact receipt and the composite digest from staged data.
#[allow(clippy::too_many_arguments)]
pub fn compute_bundle_receipt(
    key: ProjectionGenerationKey,
    source_snapshot: &str,
    projection_manifest_digest: &str,
    manifest: &BodyUnitManifest,
    coverage: &BodyCoverageArtifact,
    lexical: ArtifactReceipt,
    graph: ArtifactReceipt,
) -> Result<GenerationBundleReceipt, BodyBuildError> {
    if manifest.key != key
        || coverage.key != key
        || lexical.key != key
        || graph.key != key
        || manifest.source_snapshot != source_snapshot
    {
        return Err(integrity("bundle key"));
    }
    compute_bundle_receipt_from(
        key,
        source_snapshot,
        projection_manifest_digest,
        unit_manifest_receipt(manifest)?,
        manifest.entries.len(),
        profile_set_digest(manifest)?,
        coverage,
        lexical,
        graph,
    )
}

/// `compute_bundle_receipt` from an already computed Unit manifest receipt
/// (e.g. from stored segment digests), its item count and profile set digest.
#[allow(clippy::too_many_arguments)]
pub fn compute_bundle_receipt_from(
    key: ProjectionGenerationKey,
    source_snapshot: &str,
    projection_manifest_digest: &str,
    unit_manifest: ArtifactReceipt,
    items: usize,
    profile_set_digest: [u8; 32],
    coverage: &BodyCoverageArtifact,
    lexical: ArtifactReceipt,
    graph: ArtifactReceipt,
) -> Result<GenerationBundleReceipt, BodyBuildError> {
    if unit_manifest.key != key || coverage.key != key || lexical.key != key || graph.key != key {
        return Err(integrity("bundle key"));
    }
    let body_coverage = coverage_receipt(coverage)?;
    if body_coverage.count != items as u64 {
        return Err(integrity("coverage item count"));
    }
    let projection_digest = projection_digest(projection_manifest_digest)?;
    let mut hasher = Sha256::new();
    hasher.update(b"document-generation-bundle:v2\0");
    hasher.update(key.source_id.as_uuid().as_bytes());
    hasher.update(projection_digest);
    hasher.update(unit_manifest.digest);
    hasher.update(body_coverage.digest);
    hasher.update(lexical.digest);
    hasher.update(graph.digest);
    hasher.update(profile_set_digest);
    hasher.update((LEXICAL_SCHEMA_VERSION.len() as u32).to_be_bytes());
    hasher.update(LEXICAL_SCHEMA_VERSION.as_bytes());
    hasher.update(unit_manifest.count.to_be_bytes());
    hasher.update(body_coverage.count.to_be_bytes());
    Ok(GenerationBundleReceipt {
        key,
        source_snapshot: source_snapshot.to_owned(),
        projection_digest,
        unit_manifest,
        body_coverage,
        lexical,
        graph,
        profile_set_digest,
        lexical_schema_version: LEXICAL_SCHEMA_VERSION.to_owned(),
        composite_digest: hasher.finalize().into(),
    })
}
