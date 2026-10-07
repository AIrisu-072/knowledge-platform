//! Provider-neutral, rebuildable Vector contracts over P1 KnowledgeUnits.
//!
//! These are consistency checks on already pinned records. A valid value does not
//! grant current Source Read, retention, Version/Part authority, or body evidence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::knowledge_unit::{
    EmbeddingCacheKey, ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator, UnitCodecError,
    UnitId, UnitKind, VectorAuthorityInput, VectorHitRef, cache_key_matches_authority,
    compatible_kind, matches_pinned_unit, normalize_unit_text, text_sha256,
};
use crate::projection::ProjectionGenerationKey;
use crate::source::RetentionMode;

const MODEL_DOMAIN: &[u8] = b"embedding-model:v1\0";
const VECTOR_DOMAIN: &[u8] = b"bound-vector:v1\0";
const UNIT_SET_DOMAIN: &[u8] = b"vector-unit-bindings:v1\0";
const ENTRY_SET_DOMAIN: &[u8] = b"vector-index-entries:v1\0";
const MANIFEST_DOMAIN: &[u8] = b"vector-projection-manifest:v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorContractError {
    Invalid(&'static str),
    Unit(UnitCodecError),
}

impl std::fmt::Display for VectorContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid Vector contract: {reason}"),
            Self::Unit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for VectorContractError {}

impl From<UnitCodecError> for VectorContractError {
    fn from(value: UnitCodecError) -> Self {
        Self::Unit(value)
    }
}

type Result<T> = std::result::Result<T, VectorContractError>;

fn present(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('\0')
}

fn pinned_revision(value: &str) -> bool {
    present(value) && !matches!(value, "main" | "master" | "latest" | "HEAD")
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn digest_string(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in bytes {
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

struct FrameHash(Sha256);

impl FrameHash {
    fn new(domain: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(domain);
        Self(hash)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let length =
            u32::try_from(bytes.len()).map_err(|_| VectorContractError::Invalid("frame length"))?;
        self.0.update(length.to_be_bytes());
        self.0.update(bytes);
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<()> {
        self.bytes(value.as_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<()> {
        self.bytes(&value.to_be_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<()> {
        self.bytes(&value.to_be_bytes())
    }

    fn time(&mut self, value: Option<OffsetDateTime>) -> Result<()> {
        match value {
            Some(value) => self.bytes(&value.unix_timestamp_nanos().to_be_bytes()),
            None => self.bytes(&[]),
        }
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorPrecision {
    F32,
    F16,
}

impl VectorPrecision {
    const fn tag(self) -> u8 {
        match self {
            Self::F32 => 1,
            Self::F16 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorNormalization {
    None,
    UnitL2,
}

impl VectorNormalization {
    const fn tag(self) -> u8 {
        match self {
            Self::None => 1,
            Self::UnitL2 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorMetric {
    Dot,
    Cosine,
    Euclidean,
}

impl VectorMetric {
    const fn tag(self) -> u8 {
        match self {
            Self::Dot => 1,
            Self::Cosine => 2,
            Self::Euclidean => 3,
        }
    }
}

/// All model and numerical inputs are part of the immutable model identity.
/// Chunking describes any embedding-side transformation after the P1 Unit cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingModelSpec {
    pub model_name: String,
    pub model_revision: String,
    pub weights_sha256: [u8; 32],
    pub tokenizer_revision: String,
    pub tokenizer_files_sha256: [u8; 32],
    pub tokenizer_config_sha256: [u8; 32],
    pub unicode_preprocessing: String,
    pub input_preprocessing: String,
    pub query_template: String,
    pub passage_template: String,
    pub pooling: String,
    pub attention_masking: String,
    pub max_tokens: u32,
    pub chunking: String,
    pub truncation: String,
    pub dimension: usize,
    pub precision: VectorPrecision,
    pub normalization: VectorNormalization,
    pub metric: VectorMetric,
    pub runtime_family: String,
    pub runtime_build: String,
    pub native_binary_sha256: Option<[u8; 32]>,
    pub deterministic_config_sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmbeddingModelId(String);

impl EmbeddingModelId {
    pub fn parse(value: &str) -> Result<Self> {
        if !valid_digest(value) {
            return Err(VectorContractError::Invalid("embedding model ID"));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EmbeddingModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for EmbeddingModelId {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EmbeddingModelId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

impl EmbeddingModelSpec {
    pub fn validate_and_id(&self) -> Result<EmbeddingModelId> {
        if !present(&self.model_name)
            || !pinned_revision(&self.model_revision)
            || !pinned_revision(&self.tokenizer_revision)
            || !present(&self.unicode_preprocessing)
            || !present(&self.input_preprocessing)
            || !present(&self.pooling)
            || !present(&self.attention_masking)
            || !present(&self.chunking)
            || !present(&self.truncation)
            || !present(&self.runtime_family)
            || !pinned_revision(&self.runtime_build)
            || self.max_tokens == 0
            || self.dimension == 0
            || self.query_template.matches("{text}").count() != 1
            || self.passage_template.matches("{text}").count() != 1
        {
            return Err(VectorContractError::Invalid("model specification"));
        }
        let dimension = u32::try_from(self.dimension)
            .map_err(|_| VectorContractError::Invalid("model dimension"))?;
        let mut hash = FrameHash::new(MODEL_DOMAIN);
        hash.string(&self.model_name)?;
        hash.string(&self.model_revision)?;
        hash.bytes(&self.weights_sha256)?;
        hash.string(&self.tokenizer_revision)?;
        hash.bytes(&self.tokenizer_files_sha256)?;
        hash.bytes(&self.tokenizer_config_sha256)?;
        hash.string(&self.unicode_preprocessing)?;
        hash.string(&self.input_preprocessing)?;
        hash.string(&self.query_template)?;
        hash.string(&self.passage_template)?;
        hash.string(&self.pooling)?;
        hash.string(&self.attention_masking)?;
        hash.u32(self.max_tokens)?;
        hash.string(&self.chunking)?;
        hash.string(&self.truncation)?;
        hash.u32(dimension)?;
        hash.bytes(&[self.precision.tag()])?;
        hash.bytes(&[self.normalization.tag()])?;
        hash.bytes(&[self.metric.tag()])?;
        hash.string(&self.runtime_family)?;
        hash.string(&self.runtime_build)?;
        match &self.native_binary_sha256 {
            Some(binary) => hash.bytes(binary)?,
            None => hash.bytes(&[])?,
        }
        hash.bytes(&self.deterministic_config_sha256)?;
        Ok(EmbeddingModelId(digest_string(&hash.finish())))
    }
}

fn validate_values(spec: &EmbeddingModelSpec, values: &[f32]) -> Result<()> {
    if values.len() != spec.dimension || values.iter().any(|value| !value.is_finite()) {
        return Err(VectorContractError::Invalid(
            "vector dimension or finite value",
        ));
    }
    let squared_norm: f64 = values.iter().map(|value| f64::from(*value).powi(2)).sum();
    if !squared_norm.is_finite()
        || ((spec.normalization == VectorNormalization::UnitL2
            && (squared_norm.sqrt() - 1.0).abs() > 0.001)
            || (spec.metric == VectorMetric::Cosine && squared_norm == 0.0))
    {
        return Err(VectorContractError::Invalid("vector norm"));
    }
    Ok(())
}

fn vector_digest(model_id: &EmbeddingModelId, values: &[f32]) -> Result<[u8; 32]> {
    let mut hash = FrameHash::new(VECTOR_DOMAIN);
    hash.string(model_id.as_str())?;
    hash.u32(
        u32::try_from(values.len())
            .map_err(|_| VectorContractError::Invalid("vector dimension"))?,
    )?;
    for value in values {
        // Signed zero is numerically identical and must have one digest.
        hash.bytes(&if *value == 0.0 { 0.0_f32 } else { *value }.to_be_bytes())?;
    }
    Ok(hash.finish())
}

fn hit_for(unit: &KnowledgeUnit, pinned: &VectorAuthorityInput) -> VectorHitRef {
    VectorHitRef {
        generation: pinned.generation,
        unit_id: unit.unit_id,
        version: (*unit.version).clone(),
        part: (*unit.part).clone(),
        authoritative_representation_ref: unit.provenance.authoritative_representation_ref.clone(),
        raw: unit.provenance.raw.clone(),
        profile: unit.provenance.profile.clone(),
        text_sha256: unit.text_sha256,
    }
}

fn valid_unit_kind_and_format(unit: &KnowledgeUnit) -> bool {
    match (
        &unit.locator,
        unit.provenance.detected_format,
        unit.provenance.archive_inner_format,
    ) {
        (NativeLocator::Archive { inner, .. }, FormatId::Zip, Some(leaf))
            if leaf != FormatId::Zip =>
        {
            compatible_kind(leaf, inner, unit.kind)
        }
        (NativeLocator::Archive { .. }, _, _) | (_, FormatId::Zip, _) => false,
        (locator, format, None) => compatible_kind(format, locator, unit.kind),
        _ => false,
    }
}

const fn unit_kind_tag(kind: UnitKind) -> u8 {
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

fn validate_unit(unit: &KnowledgeUnit, pinned: &VectorAuthorityInput) -> Result<()> {
    if unit.text.is_empty()
        || normalize_unit_text(&unit.text) != unit.text
        || text_sha256(&unit.text) != unit.text_sha256
        || UnitId::derive(
            &unit.version,
            &unit.part,
            &unit.provenance.profile,
            &unit.locator,
            unit.ordinal,
        )? != unit.unit_id
        || unit.parent_unit_id == Some(unit.unit_id)
        || !valid_unit_kind_and_format(unit)
        || !present(&unit.provenance.source_snapshot)
        || !present(&unit.provenance.authoritative_representation_ref)
        || !present(&unit.provenance.parser_build_id)
        || !unit
            .provenance
            .parser_build_id
            .bytes()
            .all(|byte| byte.is_ascii_graphic())
        || !present(&pinned.authority_scope_key)
        || !present(&pinned.retention_lease_id)
        || !matches_pinned_unit(&hit_for(unit, pinned), unit, pinned)
    {
        return Err(VectorContractError::Invalid("pinned Unit binding"));
    }
    let ephemeral = matches!(
        pinned.retention_mode,
        RetentionMode::SessionOnly | RetentionMode::NoRetention
    );
    if ephemeral == pinned.lifetime_scope_id.is_empty()
        || (pinned.retention_mode == RetentionMode::CacheWithExpiry
            && pinned.lease_expires_at.is_none())
    {
        return Err(VectorContractError::Invalid("retention lifetime binding"));
    }
    Ok(())
}

/// Validated vector bytes are still body-derived data, not an authority token.
#[derive(Debug, Clone)]
pub struct BoundEmbedding {
    model_id: EmbeddingModelId,
    values: Vec<f32>,
    vector_digest: [u8; 32],
    hit: VectorHitRef,
    cache_key: EmbeddingCacheKey,
}

impl BoundEmbedding {
    pub fn new(
        spec: &EmbeddingModelSpec,
        unit: &KnowledgeUnit,
        pinned: &VectorAuthorityInput,
        values: Vec<f32>,
    ) -> Result<Self> {
        let model_id = spec.validate_and_id()?;
        validate_unit(unit, pinned)?;
        validate_values(spec, &values)?;
        let vector_digest = vector_digest(&model_id, &values)?;
        let hit = hit_for(unit, pinned);
        let cache_key = EmbeddingCacheKey {
            embedding_model_id: model_id.to_string(),
            unit_id: unit.unit_id,
            text_sha256: unit.text_sha256,
            profile: unit.provenance.profile.clone(),
            source_id: unit.version.source_id,
            authority_scope_key: pinned.authority_scope_key.clone(),
            retention_lease_id: pinned.retention_lease_id.clone(),
            lifetime_scope_id: pinned.lifetime_scope_id.clone(),
        };
        Ok(Self {
            model_id,
            values,
            vector_digest,
            hit,
            cache_key,
        })
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn validate_binding(
        &self,
        spec: &EmbeddingModelSpec,
        unit: &KnowledgeUnit,
        pinned: &VectorAuthorityInput,
    ) -> Result<()> {
        validate_unit(unit, pinned)?;
        validate_values(spec, &self.values)?;
        if self.model_id != spec.validate_and_id()?
            || self.vector_digest != vector_digest(&self.model_id, &self.values)?
            || !matches_pinned_unit(&self.hit, unit, pinned)
            || !cache_key_matches_authority(&self.cache_key, unit, pinned)
            || self.cache_key.embedding_model_id != self.model_id.as_str()
        {
            return Err(VectorContractError::Invalid("bound embedding"));
        }
        Ok(())
    }

    pub fn entry_ref(&self) -> VectorEntryRef {
        VectorEntryRef {
            hit: self.hit.clone(),
            model_id: self.model_id.clone(),
            vector_digest: self.vector_digest,
            cache_key: self.cache_key.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryEmbedding {
    model_id: EmbeddingModelId,
    values: Vec<f32>,
}

impl QueryEmbedding {
    pub fn new(spec: &EmbeddingModelSpec, values: Vec<f32>) -> Result<Self> {
        let model_id = spec.validate_and_id()?;
        validate_values(spec, &values)?;
        Ok(Self { model_id, values })
    }

    pub fn model_id(&self) -> &EmbeddingModelId {
        &self.model_id
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

#[derive(Debug, Clone)]
pub struct RankedVectorHit {
    pub hit: VectorHitRef,
    pub model_id: EmbeddingModelId,
    pub rank: usize,
    pub raw_similarity: f32,
}

impl RankedVectorHit {
    pub fn validate(
        &self,
        spec: &EmbeddingModelSpec,
        unit: &KnowledgeUnit,
        pinned: &VectorAuthorityInput,
    ) -> Result<()> {
        validate_unit(unit, pinned)?;
        if self.model_id != spec.validate_and_id()?
            || self.rank == 0
            || !self.raw_similarity.is_finite()
            || !matches_pinned_unit(&self.hit, unit, pinned)
        {
            return Err(VectorContractError::Invalid("ranked Vector hit"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VectorActivationPolicy {
    #[default]
    Disabled,
    EligibleOptIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorUnitCoverage {
    Complete,
    PartialValidated,
}

impl VectorUnitCoverage {
    const fn tag(self) -> u8 {
        match self {
            Self::Complete => 1,
            Self::PartialValidated => 2,
        }
    }
}

/// Only P1-validated Units may enter this DTO. Unsupported and permanently
/// failed P1 items have no Unit here; Partial contributes validated Units only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorManifestUnit {
    pub unit: KnowledgeUnit,
    pub authority: VectorAuthorityInput,
    pub coverage: VectorUnitCoverage,
}

/// A comparison input, not a trusted P1 bundle or Source permission issuer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorManifestInput {
    pub bundle_key: ProjectionGenerationKey,
    pub body_receipt_digest: String,
    pub source_snapshot: String,
    pub authority_scope_key: String,
    pub retention_lease_id: String,
    pub lease_expires_at: Option<OffsetDateTime>,
    /// An externally verified assertion that still requires current Source
    /// validation before stage/publish/use; Core cannot issue that grant.
    pub source_declares_persistent_embedding_permission: bool,
    pub units: Vec<VectorManifestUnit>,
    pub nonindexed_retention_unit_ids: Vec<UnitId>,
    pub lexical_analyzer_revision: String,
    pub graph_schema_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorEntryRef {
    pub hit: VectorHitRef,
    pub model_id: EmbeddingModelId,
    pub vector_digest: [u8; 32],
    pub cache_key: EmbeddingCacheKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorIndexDescriptor {
    pub engine: String,
    pub engine_build: String,
    pub parameters_digest: String,
    pub index_receipt_digest: String,
    pub index_digest: String,
}

impl VectorIndexDescriptor {
    fn validate(&self) -> Result<()> {
        if !present(&self.engine)
            || !pinned_revision(&self.engine_build)
            || !valid_digest(&self.parameters_digest)
            || !valid_digest(&self.index_receipt_digest)
            || !valid_digest(&self.index_digest)
        {
            return Err(VectorContractError::Invalid("index descriptor"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorStorageKind {
    Volatile,
    Persistent,
}

impl VectorStorageKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Volatile => 1,
            Self::Persistent => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorReadiness {
    Staged,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorProjectionManifest {
    pub schema_version: u32,
    pub bundle_key: ProjectionGenerationKey,
    pub body_receipt_digest: String,
    pub source_snapshot: String,
    pub authority_scope_key: String,
    pub retention_lease_id: String,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub persistence_declared: bool,
    pub model_id: EmbeddingModelId,
    pub vector_dimension: u32,
    pub metric: VectorMetric,
    pub precision: VectorPrecision,
    pub normalization: VectorNormalization,
    pub storage: VectorStorageKind,
    pub index: VectorIndexDescriptor,
    pub extraction_profiles: Vec<ExtractionProfileId>,
    /// Comparison pin only; never part of the embedding model or cache key.
    pub lexical_analyzer_revision: String,
    pub graph_schema_revision: String,
    pub unit_bindings_digest: String,
    pub indexed_entries_digest: String,
    pub indexed_unit_count: u64,
    pub nonindexed_retention_unit_ids: Vec<UnitId>,
    pub created_at: OffsetDateTime,
    pub readiness: VectorReadiness,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorStageReceipt {
    pub schema_version: u32,
    pub bundle_key: ProjectionGenerationKey,
    pub body_receipt_digest: String,
    pub source_snapshot: String,
    pub authority_scope_key: String,
    pub retention_lease_id: String,
    pub model_id: EmbeddingModelId,
    pub indexed_count: u64,
    pub nonindexed_retention_count: u64,
    pub index_digest: String,
    pub manifest_digest: String,
}

struct ValidatedInput<'a> {
    units: BTreeMap<UnitId, &'a VectorManifestUnit>,
    profiles: Vec<ExtractionProfileId>,
    nonindexed: Vec<UnitId>,
    bindings_digest: String,
}

fn hash_unit(hash: &mut FrameHash, item: &VectorManifestUnit) -> Result<()> {
    let unit = &item.unit;
    let authority = &item.authority;
    hash.string(&unit.unit_id.to_string())?;
    hash.bytes(unit.version.source_id.as_uuid().as_bytes())?;
    hash.bytes(unit.version.resource_id.as_uuid().as_bytes())?;
    hash.string(&unit.version.source_native_version)?;
    hash.string(&unit.part.source_native_part_id)?;
    hash.string(&unit.part.logical_path)?;
    hash.u32(unit.part.ordinal)?;
    match unit.parent_unit_id {
        Some(parent) => {
            hash.bytes(&[1])?;
            hash.string(&parent.to_string())?;
        }
        None => hash.bytes(&[0])?,
    }
    hash.u32(unit.ordinal)?;
    hash.bytes(&[unit_kind_tag(unit.kind)])?;
    hash.string(&unit.provenance.source_snapshot)?;
    hash.string(&unit.provenance.authoritative_representation_ref)?;
    hash.bytes(&unit.provenance.raw.sha256)?;
    hash.u64(unit.provenance.raw.size_bytes)?;
    hash.string(&unit.provenance.raw.media_type)?;
    hash.bytes(&[unit.provenance.detected_format.tag()])?;
    hash.bytes(&[unit
        .provenance
        .archive_inner_format
        .map_or(0, FormatId::tag)])?;
    hash.string(unit.provenance.profile.as_str())?;
    hash.string(&unit.provenance.parser_build_id)?;
    hash.bytes(&unit.text_sha256)?;
    hash.string(&unit.text)?;
    hash.bytes(&unit.locator.encode()?)?;
    hash.bytes(authority.generation.source_id.as_uuid().as_bytes())?;
    hash.bytes(authority.generation.generation_id.as_uuid().as_bytes())?;
    hash.string(&authority.authority_scope_key)?;
    hash.string(&authority.retention_lease_id)?;
    hash.string(&authority.lifetime_scope_id)?;
    hash.bytes(&[match authority.retention_mode {
        RetentionMode::PersistentResource => 1,
        RetentionMode::PersistentDiscoveryMetadata => 2,
        RetentionMode::CacheWithExpiry => 3,
        RetentionMode::SessionOnly => 4,
        RetentionMode::NoRetention => 5,
    }])?;
    hash.time(authority.lease_expires_at)?;
    hash.bytes(&[item.coverage.tag()])?;
    Ok(())
}

fn validate_input(
    input: &VectorManifestInput,
    storage: VectorStorageKind,
    created_at: OffsetDateTime,
) -> Result<ValidatedInput<'_>> {
    if !valid_digest(&input.body_receipt_digest)
        || !present(&input.source_snapshot)
        || !present(&input.authority_scope_key)
        || !present(&input.retention_lease_id)
        || !pinned_revision(&input.lexical_analyzer_revision)
        || !pinned_revision(&input.graph_schema_revision)
        || input
            .lease_expires_at
            .is_some_and(|expiry| expiry <= created_at)
    {
        return Err(VectorContractError::Invalid("P1 bundle comparison input"));
    }
    let mut units = BTreeMap::new();
    let mut locators = BTreeSet::new();
    let mut profiles = BTreeSet::new();
    let mut expected_nonindexed = BTreeSet::new();
    for item in &input.units {
        let unit = &item.unit;
        let authority = &item.authority;
        validate_unit(unit, authority)?;
        if authority.generation != input.bundle_key
            || unit.provenance.source_snapshot != input.source_snapshot
            || authority.authority_scope_key != input.authority_scope_key
            || authority.retention_lease_id != input.retention_lease_id
            || authority.lease_expires_at != input.lease_expires_at
        {
            return Err(VectorContractError::Invalid(
                "Vector input authority binding",
            ));
        }
        let locator_key = (
            unit.version.source_id,
            unit.version.resource_id,
            unit.version.source_native_version.clone(),
            unit.part.source_native_part_id.clone(),
            unit.part.logical_path.clone(),
            unit.part.ordinal,
            unit.locator.encode()?,
        );
        if !locators.insert(locator_key) || units.insert(unit.unit_id, item).is_some() {
            return Err(VectorContractError::Invalid("duplicate Unit or locator"));
        }
        let prohibited = match storage {
            VectorStorageKind::Persistent => {
                authority.retention_mode != RetentionMode::PersistentResource
                    || !input.source_declares_persistent_embedding_permission
            }
            VectorStorageKind::Volatile => {
                authority.retention_mode == RetentionMode::PersistentDiscoveryMetadata
            }
        };
        if prohibited {
            expected_nonindexed.insert(unit.unit_id);
        }
        profiles.insert(unit.provenance.profile.clone());
    }
    for item in units.values() {
        if let Some(parent_id) = item.unit.parent_unit_id {
            let parent = units
                .get(&parent_id)
                .ok_or(VectorContractError::Invalid("missing parent Unit"))?;
            if parent.unit.version != item.unit.version
                || parent.unit.part != item.unit.part
                || parent.unit.ordinal >= item.unit.ordinal
            {
                return Err(VectorContractError::Invalid("parent Unit binding"));
            }
        }
    }
    let mut nonindexed_set = BTreeSet::new();
    for id in &input.nonindexed_retention_unit_ids {
        if !units.contains_key(id) || !nonindexed_set.insert(*id) {
            return Err(VectorContractError::Invalid("nonindexed retention Unit"));
        }
    }
    if nonindexed_set != expected_nonindexed {
        return Err(VectorContractError::Invalid("retention prohibition set"));
    }
    let mut hash = FrameHash::new(UNIT_SET_DOMAIN);
    hash.u64(units.len() as u64)?;
    for item in units.values() {
        hash_unit(&mut hash, item)?;
    }
    hash.u64(nonindexed_set.len() as u64)?;
    for id in &nonindexed_set {
        hash.string(&id.to_string())?;
    }
    Ok(ValidatedInput {
        units,
        profiles: profiles.into_iter().collect(),
        nonindexed: nonindexed_set.into_iter().collect(),
        bindings_digest: digest_string(&hash.finish()),
    })
}

fn validate_entries(
    model_id: &EmbeddingModelId,
    input: &ValidatedInput<'_>,
    entries: &[VectorEntryRef],
) -> Result<String> {
    let mut indexed = BTreeMap::new();
    let nonindexed: BTreeSet<_> = input.nonindexed.iter().copied().collect();
    for entry in entries {
        let unit_id = entry.hit.unit_id;
        let item = input
            .units
            .get(&unit_id)
            .ok_or(VectorContractError::Invalid("extra Vector entry"))?;
        if nonindexed.contains(&unit_id)
            || indexed.insert(unit_id, entry).is_some()
            || entry.model_id != *model_id
            || entry.cache_key.embedding_model_id != model_id.as_str()
            || !matches_pinned_unit(&entry.hit, &item.unit, &item.authority)
            || !cache_key_matches_authority(&entry.cache_key, &item.unit, &item.authority)
        {
            return Err(VectorContractError::Invalid("Vector entry binding"));
        }
    }
    if indexed.len() + nonindexed.len() != input.units.len() {
        return Err(VectorContractError::Invalid("missing Vector entry"));
    }
    let mut hash = FrameHash::new(ENTRY_SET_DOMAIN);
    hash.u64(indexed.len() as u64)?;
    for (id, entry) in indexed {
        hash.string(&id.to_string())?;
        hash.bytes(&entry.vector_digest)?;
        hash.string(&entry.model_id.to_string())?;
        hash.string(&entry.cache_key.authority_scope_key)?;
        hash.string(&entry.cache_key.retention_lease_id)?;
        hash.string(&entry.cache_key.lifetime_scope_id)?;
    }
    Ok(digest_string(&hash.finish()))
}

impl VectorProjectionManifest {
    pub fn stage(
        spec: &EmbeddingModelSpec,
        input: &VectorManifestInput,
        index: VectorIndexDescriptor,
        entries: &[VectorEntryRef],
        storage: VectorStorageKind,
        created_at: OffsetDateTime,
    ) -> Result<Self> {
        let model_id = spec.validate_and_id()?;
        index.validate()?;
        let checked = validate_input(input, storage, created_at)?;
        let entries_digest = validate_entries(&model_id, &checked, entries)?;
        let indexed_unit_count = u64::try_from(entries.len())
            .map_err(|_| VectorContractError::Invalid("Vector entry count"))?;
        let vector_dimension = u32::try_from(spec.dimension)
            .map_err(|_| VectorContractError::Invalid("model dimension"))?;
        let mut manifest = Self {
            schema_version: 1,
            bundle_key: input.bundle_key,
            body_receipt_digest: input.body_receipt_digest.clone(),
            source_snapshot: input.source_snapshot.clone(),
            authority_scope_key: input.authority_scope_key.clone(),
            retention_lease_id: input.retention_lease_id.clone(),
            lease_expires_at: input.lease_expires_at,
            persistence_declared: input.source_declares_persistent_embedding_permission,
            model_id,
            vector_dimension,
            metric: spec.metric,
            precision: spec.precision,
            normalization: spec.normalization,
            storage,
            index,
            extraction_profiles: checked.profiles,
            lexical_analyzer_revision: input.lexical_analyzer_revision.clone(),
            graph_schema_revision: input.graph_schema_revision.clone(),
            unit_bindings_digest: checked.bindings_digest,
            indexed_entries_digest: entries_digest,
            indexed_unit_count,
            nonindexed_retention_unit_ids: checked.nonindexed,
            created_at,
            readiness: VectorReadiness::Ready,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.recompute_digest()?;
        manifest.validate_against(input, entries)?;
        Ok(manifest)
    }

    /// Complete, bidirectional record check. Source-owned current permission,
    /// actual index bytes and P1 receipt authenticity stay with trusted ports.
    pub fn validate_against(
        &self,
        input: &VectorManifestInput,
        entries: &[VectorEntryRef],
    ) -> Result<VectorStageReceipt> {
        self.index.validate()?;
        if self.schema_version != 1
            || self.readiness != VectorReadiness::Ready
            || !valid_digest(self.model_id.as_str())
            || self.vector_dimension == 0
            || self.bundle_key != input.bundle_key
            || self.body_receipt_digest != input.body_receipt_digest
            || self.source_snapshot != input.source_snapshot
            || self.authority_scope_key != input.authority_scope_key
            || self.retention_lease_id != input.retention_lease_id
            || self.lease_expires_at != input.lease_expires_at
            || self.persistence_declared != input.source_declares_persistent_embedding_permission
            || self.lexical_analyzer_revision != input.lexical_analyzer_revision
            || self.graph_schema_revision != input.graph_schema_revision
        {
            return Err(VectorContractError::Invalid("Vector manifest input"));
        }
        let checked = validate_input(input, self.storage, self.created_at)?;
        let entries_digest = validate_entries(&self.model_id, &checked, entries)?;
        if self.extraction_profiles != checked.profiles
            || self.nonindexed_retention_unit_ids != checked.nonindexed
            || self.unit_bindings_digest != checked.bindings_digest
            || self.indexed_entries_digest != entries_digest
            || self.indexed_unit_count != entries.len() as u64
            || self.manifest_digest != self.recompute_digest()?
        {
            return Err(VectorContractError::Invalid("Vector manifest seal"));
        }
        Ok(VectorStageReceipt {
            schema_version: self.schema_version,
            bundle_key: self.bundle_key,
            body_receipt_digest: self.body_receipt_digest.clone(),
            source_snapshot: self.source_snapshot.clone(),
            authority_scope_key: self.authority_scope_key.clone(),
            retention_lease_id: self.retention_lease_id.clone(),
            model_id: self.model_id.clone(),
            indexed_count: self.indexed_unit_count,
            nonindexed_retention_count: checked.nonindexed.len() as u64,
            index_digest: self.index.index_digest.clone(),
            manifest_digest: self.manifest_digest.clone(),
        })
    }

    fn recompute_digest(&self) -> Result<String> {
        let mut hash = FrameHash::new(MANIFEST_DOMAIN);
        hash.u32(self.schema_version)?;
        hash.bytes(self.bundle_key.source_id.as_uuid().as_bytes())?;
        hash.bytes(self.bundle_key.generation_id.as_uuid().as_bytes())?;
        hash.string(&self.body_receipt_digest)?;
        hash.string(&self.source_snapshot)?;
        hash.string(&self.authority_scope_key)?;
        hash.string(&self.retention_lease_id)?;
        hash.time(self.lease_expires_at)?;
        hash.bytes(&[u8::from(self.persistence_declared)])?;
        hash.string(self.model_id.as_str())?;
        hash.u32(self.vector_dimension)?;
        hash.bytes(&[self.metric.tag()])?;
        hash.bytes(&[self.precision.tag()])?;
        hash.bytes(&[self.normalization.tag()])?;
        hash.bytes(&[self.storage.tag()])?;
        hash.string(&self.index.engine)?;
        hash.string(&self.index.engine_build)?;
        hash.string(&self.index.parameters_digest)?;
        hash.string(&self.index.index_receipt_digest)?;
        hash.string(&self.index.index_digest)?;
        hash.u64(self.extraction_profiles.len() as u64)?;
        for profile in &self.extraction_profiles {
            hash.string(profile.as_str())?;
        }
        hash.string(&self.lexical_analyzer_revision)?;
        hash.string(&self.graph_schema_revision)?;
        hash.string(&self.unit_bindings_digest)?;
        hash.string(&self.indexed_entries_digest)?;
        hash.u64(self.indexed_unit_count)?;
        hash.u64(self.nonindexed_retention_unit_ids.len() as u64)?;
        for id in &self.nonindexed_retention_unit_ids {
            hash.string(&id.to_string())?;
        }
        hash.bytes(&self.created_at.unix_timestamp_nanos().to_be_bytes())?;
        hash.bytes(&[match self.readiness {
            VectorReadiness::Staged => 1,
            VectorReadiness::Ready => 2,
        }])?;
        Ok(digest_string(&hash.finish()))
    }
}
