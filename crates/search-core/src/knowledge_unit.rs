//! Canonical, provider-neutral search unit identity and local validation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use unicode_normalization::UnicodeNormalization;

use crate::id::{ResourceId, SourceId};
use crate::projection::ProjectionGenerationKey;
use crate::source::RetentionMode;

mod identity;
mod locator;
mod profile;
pub use identity::UnitId;
pub use locator::{
    DocxStep, NativeLocator, PptxTextSlot, validate_archive_member, validate_logical_path,
};
pub use profile::{
    ArchiveProfilePlan, ArchiveReaderNode, BudgetKey, ExtractionProfileDefinitionV1,
    ExtractionProfileId, FormatSettings,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitCodecError {
    Invalid(&'static str),
}

impl std::fmt::Display for UnitCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid KnowledgeUnit codec: {reason}"),
        }
    }
}

impl std::error::Error for UnitCodecError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersionRef {
    pub source_id: SourceId,
    pub resource_id: ResourceId,
    pub source_native_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPartRef {
    pub source_native_part_id: String,
    pub logical_path: String,
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawBinding {
    pub sha256: [u8; 32],
    pub size_bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormatId {
    Docx,
    Xlsx,
    Xlsm,
    Pptx,
    Pdf,
    Text,
    Csv,
    Html,
    Zip,
}

impl FormatId {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::Docx => 1,
            Self::Xlsx => 2,
            Self::Xlsm => 3,
            Self::Pptx => 4,
            Self::Pdf => 5,
            Self::Text => 6,
            Self::Csv => 7,
            Self::Html => 8,
            Self::Zip => 9,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Result<Self, UnitCodecError> {
        match tag {
            1 => Ok(Self::Docx),
            2 => Ok(Self::Xlsx),
            3 => Ok(Self::Xlsm),
            4 => Ok(Self::Pptx),
            5 => Ok(Self::Pdf),
            6 => Ok(Self::Text),
            7 => Ok(Self::Csv),
            8 => Ok(Self::Html),
            9 => Ok(Self::Zip),
            _ => Err(UnitCodecError::Invalid("format tag")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Heading,
    Paragraph,
    TableCell,
    SpreadsheetCell,
    SlideText,
    PdfText,
    PlainText,
    CsvField,
    HtmlText,
}

pub fn normalize_unit_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect()
}

pub fn text_sha256(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub start_byte: u32,
    pub end_byte: u32,
}

impl TextSpan {
    pub fn new(text: &str, start_byte: u32, end_byte: u32) -> Result<Self, UnitCodecError> {
        let start = start_byte as usize;
        let end = end_byte as usize;
        if normalize_unit_text(text) != text
            || start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(UnitCodecError::Invalid("text span"));
        }
        Ok(Self {
            start_byte,
            end_byte,
        })
    }
}

pub(crate) fn is_nfc(value: &str) -> bool {
    value.nfc().eq(value.chars())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitProvenance {
    pub source_snapshot: String,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    pub detected_format: FormatId,
    /// ZIP Units always carry Some(leaf), checked against the archive reader plan.
    pub archive_inner_format: Option<FormatId>,
    pub profile: ExtractionProfileId,
    pub parser_build_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeUnit {
    pub unit_id: UnitId,
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub parent_unit_id: Option<UnitId>,
    pub ordinal: u32,
    pub kind: UnitKind,
    pub text: String,
    pub locator: NativeLocator,
    pub text_sha256: [u8; 32],
    pub provenance: UnitProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitAuthorityBinding {
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub source_snapshot: String,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    pub detected_format: FormatId,
    /// For ZIP, None permits different leaf formats within one Part.
    pub archive_inner_format: Option<FormatId>,
    pub profile: ExtractionProfileId,
    pub parser_build_id: String,
    pub archive_plan: Option<ArchiveProfilePlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitValidationError {
    Codec(UnitCodecError),
    Invalid(&'static str),
}

impl From<UnitCodecError> for UnitValidationError {
    fn from(value: UnitCodecError) -> Self {
        Self::Codec(value)
    }
}

impl std::fmt::Display for UnitValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(f),
            Self::Invalid(reason) => write!(f, "invalid KnowledgeUnit: {reason}"),
        }
    }
}

impl std::error::Error for UnitValidationError {}

fn valid_mime_essence(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    let valid_token = |token: &str| {
        !token.is_empty()
            && token.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || b"!#$%&'*+-.^_`|~".contains(&byte)
            })
    };
    valid_token(kind) && valid_token(subtype)
}

pub(crate) fn compatible_kind(format: FormatId, locator: &NativeLocator, kind: UnitKind) -> bool {
    if let (FormatId::Docx, NativeLocator::Docx { steps }) = (format, locator) {
        return match kind {
            UnitKind::Heading => true,
            UnitKind::Paragraph => steps.len() == 1,
            UnitKind::TableCell => steps.len() > 1,
            _ => false,
        };
    }
    matches!(
        (format, locator, kind),
        (
            FormatId::Xlsx | FormatId::Xlsm,
            NativeLocator::Spreadsheet { .. },
            UnitKind::SpreadsheetCell,
        ) | (
            FormatId::Pptx,
            NativeLocator::Pptx { .. },
            UnitKind::SlideText
        ) | (FormatId::Pdf, NativeLocator::Pdf { .. }, UnitKind::PdfText)
            | (
                FormatId::Text,
                NativeLocator::Text { .. },
                UnitKind::PlainText
            )
            | (FormatId::Csv, NativeLocator::Csv { .. }, UnitKind::CsvField)
            | (
                FormatId::Html,
                NativeLocator::Html { .. },
                UnitKind::HtmlText | UnitKind::Heading
            )
    )
}

/// Validates canonical local fields against trusted, already-pinned authority inputs.
/// The caller still proves Source Read, retention, registry, raw-byte and native round trips.
pub fn validate_part_units(
    binding: &UnitAuthorityBinding,
    units: &[KnowledgeUnit],
) -> Result<(), UnitValidationError> {
    if binding.version.source_native_version.is_empty()
        || !is_nfc(&binding.version.source_native_version)
        || binding.part.source_native_part_id.is_empty()
        || !is_nfc(&binding.part.source_native_part_id)
        || binding.source_snapshot.is_empty()
        || binding.authoritative_representation_ref.is_empty()
        || !profile::valid_ascii_id(&binding.parser_build_id)
        || !valid_mime_essence(&binding.raw.media_type)
    {
        return Err(UnitValidationError::Invalid("authority binding"));
    }
    validate_logical_path(&binding.part.logical_path)?;
    if binding.detected_format == FormatId::Zip {
        let Some(plan) = &binding.archive_plan else {
            return Err(UnitValidationError::Invalid("archive binding"));
        };
        if binding.archive_inner_format == Some(FormatId::Zip)
            || plan
                .nodes
                .first()
                .is_none_or(|root| root.parser_build_id != binding.parser_build_id)
            || ExtractionProfileId::for_archive(plan)? != binding.profile
        {
            return Err(UnitValidationError::Invalid("archive profile"));
        }
    } else if binding.archive_inner_format.is_some() || binding.archive_plan.is_some() {
        return Err(UnitValidationError::Invalid("nonarchive binding"));
    }

    let mut seen_ids = HashSet::new();
    let mut seen_locators = HashSet::new();
    for (index, unit) in units.iter().enumerate() {
        if unit.ordinal
            != u32::try_from(index).map_err(|_| UnitValidationError::Invalid("unit count"))?
            || unit.version != binding.version
            || unit.part != binding.part
            || unit.provenance.source_snapshot != binding.source_snapshot
            || unit.provenance.authoritative_representation_ref
                != binding.authoritative_representation_ref
            || unit.provenance.raw != binding.raw
            || unit.provenance.detected_format != binding.detected_format
            || (binding.detected_format != FormatId::Zip
                && unit.provenance.archive_inner_format != binding.archive_inner_format)
            || (binding.detected_format == FormatId::Zip
                && binding.archive_inner_format.is_some()
                && unit.provenance.archive_inner_format != binding.archive_inner_format)
            || unit.provenance.profile != binding.profile
            || unit.provenance.parser_build_id != binding.parser_build_id
        {
            return Err(UnitValidationError::Invalid("unit authority mismatch"));
        }
        if unit.text.is_empty()
            || unit.text.len() > u32::MAX as usize
            || normalize_unit_text(&unit.text) != unit.text
        {
            return Err(UnitValidationError::Invalid("unit text"));
        }
        if text_sha256(&unit.text) != unit.text_sha256 {
            return Err(UnitValidationError::Invalid("text digest"));
        }
        let locator_bytes = unit.locator.encode()?;
        if !seen_locators.insert(locator_bytes) {
            return Err(UnitValidationError::Invalid("duplicate locator"));
        }
        match (&unit.locator, binding.detected_format) {
            (NativeLocator::Archive { members, inner }, FormatId::Zip) => {
                let leaf_format = unit
                    .provenance
                    .archive_inner_format
                    .ok_or(UnitValidationError::Invalid("archive inner format"))?;
                let plan = binding
                    .archive_plan
                    .as_ref()
                    .ok_or(UnitValidationError::Invalid("archive plan"))?;
                if !plan.used_leaf_chains.contains(members)
                    || plan
                        .nodes
                        .iter()
                        .find(|node| &node.members == members)
                        .is_none_or(|node| node.definition.format != leaf_format)
                    || !compatible_kind(leaf_format, inner, unit.kind)
                {
                    return Err(UnitValidationError::Invalid("archive leaf locator"));
                }
            }
            (NativeLocator::Archive { .. }, _) | (_, FormatId::Zip) => {
                return Err(UnitValidationError::Invalid("archive locator"));
            }
            (locator, format) if !compatible_kind(format, locator, unit.kind) => {
                return Err(UnitValidationError::Invalid("kind locator"));
            }
            _ => {}
        }
        if UnitId::derive(
            &unit.version,
            &unit.part,
            &unit.provenance.profile,
            &unit.locator,
            unit.ordinal,
        )? != unit.unit_id
        {
            return Err(UnitValidationError::Invalid("unit ID"));
        }
        if unit
            .parent_unit_id
            .is_some_and(|parent| !seen_ids.contains(&parent))
        {
            return Err(UnitValidationError::Invalid("parent unit"));
        }
        if !seen_ids.insert(unit.unit_id) {
            return Err(UnitValidationError::Invalid("duplicate unit ID"));
        }
    }
    Ok(())
}

/// The key partitions body-derived embeddings by Source-owned authority and lifetime scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmbeddingCacheKey {
    pub embedding_model_id: String,
    pub unit_id: UnitId,
    pub text_sha256: [u8; 32],
    pub profile: ExtractionProfileId,
    pub source_id: SourceId,
    pub authority_scope_key: String,
    pub retention_lease_id: String,
    pub lifetime_scope_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorHitRef {
    pub generation: ProjectionGenerationKey,
    pub unit_id: UnitId,
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    pub profile: ExtractionProfileId,
    pub text_sha256: [u8; 32],
}

/// Inputs already pinned by trusted Source/P2 code; this record is not authority proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorAuthorityInput {
    pub generation: ProjectionGenerationKey,
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    pub profile: ExtractionProfileId,
    pub authority_scope_key: String,
    pub retention_lease_id: String,
    pub lifetime_scope_id: String,
    pub retention_mode: RetentionMode,
    pub lease_expires_at: Option<OffsetDateTime>,
}

/// Compares stored hit fields with one pinned Unit and manifest binding.
pub fn matches_pinned_unit(
    hit: &VectorHitRef,
    unit: &KnowledgeUnit,
    pinned: &VectorAuthorityInput,
) -> bool {
    pinned.generation.source_id == pinned.version.source_id
        && hit.generation == pinned.generation
        && hit.unit_id == unit.unit_id
        && hit.version == unit.version
        && hit.version == pinned.version
        && hit.part == unit.part
        && hit.part == pinned.part
        && hit.authoritative_representation_ref == unit.provenance.authoritative_representation_ref
        && hit.authoritative_representation_ref == pinned.authoritative_representation_ref
        && hit.raw == unit.provenance.raw
        && hit.raw == pinned.raw
        && hit.profile == unit.provenance.profile
        && hit.profile == pinned.profile
        && hit.text_sha256 == unit.text_sha256
}

/// Compares a cache partition with pinned fields, without asserting current Source permission.
pub fn cache_key_matches_authority(
    key: &EmbeddingCacheKey,
    unit: &KnowledgeUnit,
    pinned: &VectorAuthorityInput,
) -> bool {
    key.source_id == pinned.generation.source_id
        && key.source_id == pinned.version.source_id
        && key.source_id == unit.version.source_id
        && key.unit_id == unit.unit_id
        && key.text_sha256 == unit.text_sha256
        && key.profile == unit.provenance.profile
        && key.profile == pinned.profile
        && key.authority_scope_key == pinned.authority_scope_key
        && key.retention_lease_id == pinned.retention_lease_id
        && key.lifetime_scope_id == pinned.lifetime_scope_id
        && unit.version == pinned.version
        && unit.part == pinned.part
        && unit.provenance.authoritative_representation_ref
            == pinned.authoritative_representation_ref
        && unit.provenance.raw == pinned.raw
}

pub(crate) fn write_u32(output: &mut Vec<u8>, value: usize) -> Result<(), UnitCodecError> {
    let value = u32::try_from(value).map_err(|_| UnitCodecError::Invalid("length"))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

pub(crate) fn write_frame(output: &mut Vec<u8>, value: &[u8]) -> Result<(), UnitCodecError> {
    write_u32(output, value.len())?;
    output.extend_from_slice(value);
    Ok(())
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(crate) fn take(&mut self, size: usize) -> Result<&'a [u8], UnitCodecError> {
        if size > self.remaining() {
            return Err(UnitCodecError::Invalid("truncated bytes"));
        }
        let start = self.position;
        self.position += size;
        Ok(&self.bytes[start..self.position])
    }

    pub(crate) fn byte(&mut self) -> Result<u8, UnitCodecError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, UnitCodecError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().expect("four bytes")))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, UnitCodecError> {
        let bytes = self.take(8)?;
        Ok(u64::from_be_bytes(bytes.try_into().expect("eight bytes")))
    }

    pub(crate) fn frame(&mut self) -> Result<&'a [u8], UnitCodecError> {
        let size = self.u32()? as usize;
        self.take(size)
    }

    pub(crate) fn finish(self) -> Result<(), UnitCodecError> {
        if self.remaining() != 0 {
            return Err(UnitCodecError::Invalid("trailing bytes"));
        }
        Ok(())
    }
}
