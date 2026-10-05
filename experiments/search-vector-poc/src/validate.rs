//! Provider-neutral, frozen P1 identity codec. Synthetic Source bytes are checked here;
//! this is not a parser/native-locator round-trip qualification.

use std::collections::BTreeSet;

use search_core::id::{ResourceId, SourceId};
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator, RawBinding,
    ResourceVersionRef, UnitAuthorityBinding, UnitId, UnitKind, UnitProvenance,
    validate_part_units,
};
use search_core::source::RetentionMode;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::corpus::SourcePart;

pub const SYNTHETIC_PROFILE: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const UNIT_DOMAIN: &[u8] = b"knowledge-unit:v1\0";
const LOCATOR_DOMAIN: &[u8] = b"native-locator:v1\0";

#[derive(Clone, Debug)]
pub struct FrozenUnit {
    pub source_id: Uuid,
    pub resource_id: Uuid,
    pub generation_id: Uuid,
    pub source_snapshot: String,
    pub source_native_version: String,
    pub source_native_part_id: String,
    pub logical_path: String,
    pub part_ordinal: u32,
    pub profile: String,
    pub parser_build_id: String,
    pub authoritative_representation_ref: String,
    pub raw_bytes: Vec<u8>,
    pub raw_sha256: String,
    pub raw_size_bytes: u64,
    pub text: String,
    pub text_sha256: String,
    pub line_start: u32,
    pub line_end: u32,
    pub unit_ordinal: u32,
    pub unit_id: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn normalized_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect()
}

pub fn encode_text_locator(start: u32, end: u32) -> Result<Vec<u8>, String> {
    if end <= start {
        return Err("empty or reversed Text locator".into());
    }
    let mut bytes = LOCATOR_DOMAIN.to_vec();
    bytes.push(5);
    bytes.extend_from_slice(&start.to_be_bytes());
    bytes.extend_from_slice(&end.to_be_bytes());
    Ok(bytes)
}

pub fn decode_text_locator(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() != LOCATOR_DOMAIN.len() + 1 + 8
        || !bytes.starts_with(LOCATOR_DOMAIN)
        || bytes[LOCATOR_DOMAIN.len()] != 5
    {
        return Err("noncanonical Text locator".into());
    }
    let offset = LOCATOR_DOMAIN.len() + 1;
    let start = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let end = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
    if end <= start {
        return Err("empty or reversed Text locator".into());
    }
    Ok((start, end))
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) -> Result<(), String> {
    let len = u32::try_from(bytes.len()).map_err(|_| "field exceeds u32 frame")?;
    hasher.update(len.to_be_bytes());
    hasher.update(bytes);
    Ok(())
}

pub fn unit_id(unit: &FrozenUnit) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(UNIT_DOMAIN);
    frame(&mut hash, unit.source_id.as_bytes())?;
    frame(&mut hash, unit.resource_id.as_bytes())?;
    frame(&mut hash, unit.source_native_version.as_bytes())?;
    frame(&mut hash, unit.source_native_part_id.as_bytes())?;
    frame(&mut hash, unit.logical_path.as_bytes())?;
    frame(&mut hash, &unit.part_ordinal.to_be_bytes())?;
    frame(&mut hash, unit.profile.as_bytes())?;
    frame(
        &mut hash,
        &encode_text_locator(unit.line_start, unit.line_end)?,
    )?;
    frame(&mut hash, &unit.unit_ordinal.to_be_bytes())?;
    Ok(format!("ku1:{}", hex(&hash.finalize())))
}

pub fn validate_units(units: &[FrozenUnit], pinned_generation: Uuid) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut locations = BTreeSet::new();
    for unit in units {
        if unit.generation_id != pinned_generation
            || unit.source_snapshot != "synthetic-snapshot-v1"
        {
            return Err("generation or Source snapshot mismatch".into());
        }
        if Uuid::parse_str(&unit.source_native_version)
            .ok()
            .map(|id| id.to_string())
            != Some(unit.source_native_version.clone())
        {
            return Err("noncanonical Version ID".into());
        }
        if Uuid::parse_str(&unit.source_native_part_id)
            .ok()
            .map(|id| id.to_string())
            != Some(unit.source_native_part_id.clone())
            || unit.logical_path.is_empty()
            || unit.logical_path.starts_with('/')
            || unit
                .logical_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || unit.logical_path.contains('\\')
            || unit.logical_path != unit.logical_path.nfc().collect::<String>()
        {
            return Err("invalid Part binding".into());
        }
        if unit.profile != SYNTHETIC_PROFILE
            || unit.parser_build_id != "synthetic-trusted-projection-v1"
            || unit.authoritative_representation_ref
                != format!(
                    "synthetic-representation:{}:{}",
                    unit.resource_id, unit.logical_path
                )
        {
            return Err("profile or representation mismatch".into());
        }
        if unit.raw_size_bytes != unit.raw_bytes.len() as u64
            || unit.raw_sha256 != sha256_hex(&unit.raw_bytes)
            || unit.text != normalized_text(&unit.text)
            || unit.text_sha256 != sha256_hex(unit.text.as_bytes())
            || unit.raw_bytes != unit.text.as_bytes()
        {
            return Err("raw/text binding mismatch".into());
        }
        let locator = encode_text_locator(unit.line_start, unit.line_end)?;
        let expected_lines = if unit.logical_path == "attachment" {
            2
        } else {
            1
        };
        if decode_text_locator(&locator)? != (0, expected_lines)
            || unit.text.lines().count() != expected_lines as usize
            || unit.unit_ordinal != 0
        {
            return Err("synthetic native locator mismatch".into());
        }
        if unit.unit_id != unit_id(unit)? || !ids.insert(unit.unit_id.clone()) {
            return Err("UnitId mismatch or duplicate".into());
        }
        if !locations.insert((
            unit.source_id,
            unit.resource_id,
            unit.source_native_version.clone(),
            unit.source_native_part_id.clone(),
            unit.line_start,
            unit.line_end,
        )) {
            return Err("duplicate Version/Part/locator".into());
        }
    }
    Ok(())
}

/// The synthetic Source fixture supplies Part/raw/representation independently of the Unit.
/// Core validates the typed Unit against this binding; this does not qualify a real body parser.
pub fn validate_source_owned_unit(
    source_id: Uuid,
    resource_id: Uuid,
    generation_id: Uuid,
    source_native_version: &str,
    part: &SourcePart,
    unit: &FrozenUnit,
) -> Result<KnowledgeUnit, String> {
    if unit.source_id != source_id
        || unit.resource_id != resource_id
        || unit.generation_id != generation_id
        || unit.source_snapshot != "synthetic-snapshot-v1"
        || unit.source_native_version != source_native_version
        || unit.source_native_part_id != part.source_native_part_id
        || unit.logical_path != part.logical_path
        || unit.part_ordinal != part.ordinal
        || unit.authoritative_representation_ref != part.authoritative_representation_ref
        || unit.line_start != part.line_start
        || unit.line_end != part.line_end
        || unit.raw_bytes != part.raw_bytes
        || unit.raw_sha256 != sha256_hex(&part.raw_bytes)
        || unit.raw_size_bytes != part.raw_bytes.len() as u64
        || unit.text.as_bytes() != part.raw_bytes
    {
        return Err("Source-owned Version/Part/raw/representation/locator mismatch".into());
    }
    let version = ResourceVersionRef {
        source_id: SourceId::from_uuid(source_id),
        resource_id: ResourceId::from_uuid(resource_id),
        source_native_version: source_native_version.into(),
    };
    let content_part = ContentPartRef {
        source_native_part_id: part.source_native_part_id.clone(),
        logical_path: part.logical_path.clone(),
        ordinal: part.ordinal,
    };
    let raw = RawBinding {
        sha256: Sha256::digest(&part.raw_bytes).into(),
        size_bytes: part.raw_bytes.len() as u64,
        media_type: "text/plain".into(),
    };
    let profile = ExtractionProfileId::parse(&unit.profile).map_err(|e| e.to_string())?;
    let binding = UnitAuthorityBinding {
        version: version.clone(),
        part: content_part.clone(),
        source_snapshot: "synthetic-snapshot-v1".into(),
        authoritative_representation_ref: part.authoritative_representation_ref.clone(),
        raw: raw.clone(),
        detected_format: FormatId::Text,
        archive_inner_format: None,
        profile: profile.clone(),
        parser_build_id: "synthetic-trusted-projection-v1".into(),
        archive_plan: None,
    };
    let knowledge_unit = KnowledgeUnit {
        unit_id: UnitId::parse(&unit.unit_id).map_err(|e| e.to_string())?,
        version,
        part: content_part,
        parent_unit_id: None,
        ordinal: unit.unit_ordinal,
        kind: UnitKind::PlainText,
        text: unit.text.clone(),
        locator: NativeLocator::Text {
            line_start: unit.line_start,
            line_end: unit.line_end,
        },
        text_sha256: Sha256::digest(unit.text.as_bytes()).into(),
        provenance: UnitProvenance {
            source_snapshot: unit.source_snapshot.clone(),
            authoritative_representation_ref: unit.authoritative_representation_ref.clone(),
            raw,
            detected_format: FormatId::Text,
            archive_inner_format: None,
            profile,
            parser_build_id: unit.parser_build_id.clone(),
        },
    };
    validate_part_units(&binding, std::slice::from_ref(&knowledge_unit))
        .map_err(|e| e.to_string())?;
    Ok(knowledge_unit)
}

/// Contract-only probe. No embeddings are generated and no Vector ranking is run.
#[derive(Clone, Debug)]
pub struct MockVectorHit {
    pub generation_id: Uuid,
    pub parent_resource_id: Uuid,
    pub source_native_version: String,
    pub source_native_part_id: String,
    pub unit_id: String,
    pub profile: String,
    pub raw_sha256: String,
    pub text_sha256: String,
    pub embedding: Vec<f32>,
    pub authority_scope_matches: bool,
    pub lease_valid: bool,
}

pub fn validate_mock_vector_hit(
    unit: &FrozenUnit,
    hit: &MockVectorHit,
    pinned_generation: Uuid,
    expected_dimension: usize,
    retention: RetentionMode,
    current_read: bool,
    current_version: bool,
) -> Result<(), &'static str> {
    if expected_dimension == 0
        || hit.embedding.len() != expected_dimension
        || hit.embedding.iter().any(|value| !value.is_finite())
    {
        return Err("dimension or finite-value mismatch");
    }
    if hit.generation_id != pinned_generation
        || unit.generation_id != pinned_generation
        || hit.parent_resource_id != unit.resource_id
        || hit.source_native_version != unit.source_native_version
        || hit.source_native_part_id != unit.source_native_part_id
        || hit.unit_id != unit.unit_id
        || hit.profile != unit.profile
        || hit.raw_sha256 != unit.raw_sha256
        || hit.text_sha256 != unit.text_sha256
    {
        return Err("stale Vector binding");
    }
    if retention != RetentionMode::PersistentResource
        || !hit.authority_scope_matches
        || !hit.lease_valid
    {
        return Err("retention, scope, or lease forbids persistent Vector artifact");
    }
    if !current_read || !current_version {
        return Err("Source current Read or Version denied");
    }
    Ok(())
}
