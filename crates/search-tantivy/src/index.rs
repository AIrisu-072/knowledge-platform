use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use search_core::id::{ResourceId, SourceId};
use search_core::knowledge_unit::KnowledgeUnit;
use search_core::projection::{ProjectionGenerationKey, ProjectionGenerationManifest};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, RetentionMode};
use tantivy::{Index, IndexReader, doc};

use crate::body::{IndexedUnitDoc, UnitIndex, build_unit_index, enumerate};
use crate::schema::{
    LEXICAL_SCHEMA_VERSION, LexicalFields, kind_token, lexical_schema, normalize_exact,
};

/// Content explicitly furnished by the owning Source for indexing. There is
/// intentionally no conversion from ResourceBody or DSI extraction evidence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSuppliedBody {
    source_id: SourceId,
    text: String,
}

impl SourceSuppliedBody {
    pub fn new(source_id: SourceId, text: impl Into<String>) -> Self {
        Self {
            source_id,
            text: text.into(),
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

/// Source-provided lexical fields. The adapter never extracts or infers text
/// from a DiscoverableResource or a document-inspection result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalDocument {
    pub resource_ref: ResourceId,
    pub kind: ResourceKind,
    pub canonical_name: String,
    pub title: Option<String>,
    pub aliases: Vec<String>,
    pub high_signal_text: Option<String>,
    pub body: Option<SourceSuppliedBody>,
    pub locator: Option<String>,
}

/// A single Source snapshot's lexical fields and independently supplied
/// projection versions. Callers must assemble the documents from this named
/// snapshot rather than copying provenance from the generation manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalBuildInput {
    source_id: SourceId,
    source_snapshot: String,
    projection_schema_version: String,
    lens_version: u32,
    documents: Vec<LexicalDocument>,
    body_units: Option<BodyUnits>,
    /// The analyzer this input is indexed with; it must equal the manifest's.
    analyzer_version: String,
    /// A committed Unit index of an earlier generation to start from.
    base_units_dir: Option<std::path::PathBuf>,
}

impl LexicalBuildInput {
    pub fn new(
        source_id: SourceId,
        source_snapshot: impl Into<String>,
        projection_schema_version: impl Into<String>,
        lens_version: u32,
        documents: Vec<LexicalDocument>,
    ) -> Self {
        Self {
            source_id,
            source_snapshot: source_snapshot.into(),
            projection_schema_version: projection_schema_version.into(),
            lens_version,
            documents,
            body_units: None,
            analyzer_version: crate::analyzer::LEGACY_ANALYZER_VERSION.into(),
            base_units_dir: None,
        }
    }

    /// Build the Unit index from this earlier generation's committed Unit
    /// index when it can serve as a base; the result is the same Units.
    pub fn with_base_units_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.base_units_dir = Some(dir.into());
        self
    }

    /// Index with a supported analyzer instead of the legacy default.
    pub fn with_analyzer_version(mut self, analyzer_version: impl Into<String>) -> Self {
        self.analyzer_version = analyzer_version.into();
        self
    }

    pub fn analyzer_version(&self) -> &str {
        &self.analyzer_version
    }

    /// Make the generation body-ready (schema-2) with exactly these verified
    /// Units. An empty collection is still body-ready: every item had no text.
    pub fn with_body_units(mut self, units: Vec<KnowledgeUnit>) -> Self {
        self.body_units = Some(BodyUnits(Arc::new(units), Arc::default()));
        self
    }

    /// [`Self::with_body_units`] over Units the caller keeps owning (T12): the
    /// build reads them in place instead of a copy.
    pub fn with_body_unit_source(mut self, units: Arc<dyn UnitSource>) -> Self {
        self.body_units = Some(BodyUnits(units, Arc::default()));
        self
    }

    pub(crate) fn documents(&self) -> &[LexicalDocument] {
        &self.documents
    }

    pub(crate) fn body_units(&self) -> Option<Vec<&KnowledgeUnit>> {
        self.body_units.as_ref().map(|units| units.0.units())
    }

    /// The body Units' seal entries their source already holds, read from
    /// the source once per input.
    pub(crate) fn body_seal_entries(&self) -> Option<Arc<Vec<crate::UnitSealEntry>>> {
        self.body_units.as_ref().and_then(|units| {
            units
                .1
                .get_or_init(|| units.0.seal_entries().map(Arc::new))
                .clone()
        })
    }
}

/// Units a lexical build reads without owning them.
pub trait UnitSource: Send + Sync {
    /// Every Unit, in the caller's order.
    fn units(&self) -> Vec<&KnowledgeUnit>;

    /// Every Unit's ID and `unit_doc_hash`, when the source keeps them for
    /// Units it has hashed before; `None` makes the build hash every Unit.
    fn seal_entries(&self) -> Option<Vec<crate::UnitSealEntry>> {
        None
    }
}

impl UnitSource for Vec<KnowledgeUnit> {
    fn units(&self) -> Vec<&KnowledgeUnit> {
        self.iter().collect()
    }
}

/// The body Units of one build input and their seal entries once read;
/// equal when they hold the same Units.
#[derive(Clone)]
struct BodyUnits(
    Arc<dyn UnitSource>,
    Arc<std::sync::OnceLock<Option<Arc<Vec<crate::UnitSealEntry>>>>>,
);

impl std::fmt::Debug for BodyUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BodyUnits").field(&self.0.units()).finish()
    }
}

impl PartialEq for BodyUnits {
    fn eq(&self, other: &Self) -> bool {
        self.0.units() == other.0.units()
    }
}

impl Eq for BodyUnits {}

#[derive(Debug, thiserror::Error)]
pub enum LexicalIndexError {
    #[error("lexical Source does not match generation Source")]
    SourceMismatch,
    #[error("lexical Source snapshot does not match generation Source snapshot")]
    SourceSnapshotMismatch,
    #[error("lexical projection schema version does not match generation schema version")]
    SchemaVersionMismatch,
    #[error("generation schema version is not supported by the Tantivy lexical adapter")]
    UnsupportedSchemaVersion,
    #[error("lexical Lens version does not match generation Lens version")]
    LensVersionMismatch,
    #[error("source-supplied body belongs to another Source")]
    BodySourceMismatch,
    #[error("Source does not permit retained body indexing")]
    BodyNotPermitted,
    #[error("Source retention does not permit a persistent lexical index")]
    PersistenceDenied,
    #[error("generation analyzer is not a supported analyzer of this input")]
    AnalyzerMismatch,
    #[error("lexical generation already exists")]
    DuplicateGeneration,
    #[error("lexical resource occurs more than once in generation")]
    DuplicateResource,
    #[error("lexical resources exceed Source snapshot resource count")]
    ResourceCountExceeded,
    #[error("lexical generation is unknown")]
    UnknownGeneration,
    #[error("lexical index lock is poisoned")]
    LockPoisoned,
    #[error("body Unit belongs to another Source")]
    UnitSourceMismatch,
    #[error("body Unit parent Resource is not in the generation")]
    UnitParentMissing,
    #[error("body Unit occurs more than once in generation")]
    DuplicateUnit,
    #[error("body Unit document cannot be encoded or decoded")]
    UnitEncoding,
    #[error("lexical generation is not body-ready")]
    NotBodyReady,
    #[error("persisted lexical generation is missing, foreign or inconsistent")]
    PersistedMismatch,
    #[error("lexical index file I/O failed")]
    Io,
    #[error(transparent)]
    Tantivy(#[from] tantivy::TantivyError),
}

pub(crate) struct DocumentMetadata {
    pub resource_ref: ResourceId,
    pub kind: ResourceKind,
    pub locator: Option<String>,
    pub provenance: Option<String>,
}

pub(crate) struct GenerationIndex {
    pub index: Index,
    pub reader: IndexReader,
    pub fields: LexicalFields,
    pub documents: BTreeMap<String, DocumentMetadata>,
    pub units: Option<UnitIndex>,
    /// The tokenizer of this generation's fields (read when a fault rebuilds
    /// its Unit index).
    #[cfg_attr(not(feature = "fault-injection"), allow(dead_code))]
    pub tokenizer: &'static str,
}

/// A generation is assembled privately, then inserted once. Existing keyed
/// segments remain readable when another generation is built.
#[derive(Default)]
pub struct TantivyLexicalIndex {
    pub(crate) generations: RwLock<BTreeMap<ProjectionGenerationKey, Arc<GenerationIndex>>>,
}

impl TantivyLexicalIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove a privately built generation after the shared projection
    /// publication rejects it. Callers must never pass a published key.
    pub fn discard_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        Ok(self
            .generations
            .write()
            .map_err(|_| LexicalIndexError::LockPoisoned)?
            .remove(&key)
            .is_some())
    }

    pub fn build_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError> {
        self.assemble(manifest, source, input, None)
    }

    /// Builds the generation into `dir` (Resource index, Unit index and the
    /// Source-supplied input sidecar) and registers it for queries.
    pub fn build_generation_at(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
        dir: &std::path::Path,
    ) -> Result<(), LexicalIndexError> {
        self.assemble(manifest, source, input, Some(dir))
    }

    fn assemble(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
        dir: Option<&std::path::Path>,
    ) -> Result<(), LexicalIndexError> {
        let key = manifest.key();
        if source.source_id != key.source_id || input.source_id != key.source_id {
            return Err(LexicalIndexError::SourceMismatch);
        }
        if input.source_snapshot != manifest.source_snapshot {
            return Err(LexicalIndexError::SourceSnapshotMismatch);
        }
        if manifest.projection_schema_version != LEXICAL_SCHEMA_VERSION {
            return Err(LexicalIndexError::UnsupportedSchemaVersion);
        }
        if input.projection_schema_version != manifest.projection_schema_version {
            return Err(LexicalIndexError::SchemaVersionMismatch);
        }
        if input.lens_version != manifest.lens_version {
            return Err(LexicalIndexError::LensVersionMismatch);
        }
        if !matches!(
            source.retention_mode,
            RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
        ) {
            return Err(LexicalIndexError::PersistenceDenied);
        }
        if manifest.analyzer_version.as_deref() != Some(input.analyzer_version.as_str()) {
            return Err(LexicalIndexError::AnalyzerMismatch);
        }
        let tokenizer = crate::analyzer::tokenizer_name(&input.analyzer_version)
            .ok_or(LexicalIndexError::AnalyzerMismatch)?;
        // A lexical generation may index only a subset of the Source snapshot,
        // but it cannot contain more distinct Resources than the manifest.
        if u64::try_from(input.documents.len())
            .map_err(|_| LexicalIndexError::ResourceCountExceeded)?
            > manifest.resource_count
        {
            return Err(LexicalIndexError::ResourceCountExceeded);
        }
        if self
            .generations
            .read()
            .map_err(|_| LexicalIndexError::LockPoisoned)?
            .contains_key(&key)
        {
            return Err(LexicalIndexError::DuplicateGeneration);
        }
        for document in &input.documents {
            if let Some(body) = &document.body {
                if body.source_id != source.source_id {
                    return Err(LexicalIndexError::BodySourceMismatch);
                }
                if source.retention_mode != RetentionMode::PersistentResource
                    || !source.supports(DiscoveryMode::LocalContentSearch)
                {
                    return Err(LexicalIndexError::BodyNotPermitted);
                }
            }
        }
        let body_units = input.body_units();
        if let Some(units) = &body_units {
            if !units.is_empty()
                && (source.retention_mode != RetentionMode::PersistentResource
                    || !source.supports(DiscoveryMode::LocalContentSearch))
            {
                return Err(LexicalIndexError::BodyNotPermitted);
            }
            let parents: std::collections::BTreeSet<_> = input
                .documents
                .iter()
                .map(|document| document.resource_ref)
                .collect();
            let mut seen = std::collections::HashSet::with_capacity(units.len());
            for unit in units.iter() {
                if unit.version.source_id != key.source_id {
                    return Err(LexicalIndexError::UnitSourceMismatch);
                }
                if !parents.contains(&unit.version.resource_id) {
                    return Err(LexicalIndexError::UnitParentMissing);
                }
                if !seen.insert(unit.unit_id) {
                    return Err(LexicalIndexError::DuplicateUnit);
                }
            }
        }
        if let Some(dir) = dir {
            crate::persist::write_sidecar(dir, &manifest, source, &input)?;
        }
        let unit_index = body_units
            .as_deref()
            .map(|units| match dir {
                Some(dir) => {
                    let target = dir.join(crate::persist::UNITS_DIR);
                    let from_base = match &input.base_units_dir {
                        Some(base) => crate::body::build_unit_index_from_base(
                            units,
                            input.body_seal_entries(),
                            base,
                            &target,
                            tokenizer,
                        )?,
                        None => None,
                    };
                    match from_base {
                        Some(index) => Ok(index),
                        None => crate::body::build_unit_index_at(units, &target, tokenizer),
                    }
                }
                None => build_unit_index(units, tokenizer),
            })
            .transpose()?;
        let mut documents = input.documents;
        documents.sort_by_key(|document| document.resource_ref);
        if documents
            .windows(2)
            .any(|pair| pair[0].resource_ref == pair[1].resource_ref)
        {
            return Err(LexicalIndexError::DuplicateResource);
        }

        let (schema, fields) = lexical_schema(tokenizer);
        let index = match dir {
            Some(dir) => {
                let path = dir.join(crate::persist::RESOURCES_DIR);
                std::fs::create_dir_all(&path).map_err(|_| LexicalIndexError::Io)?;
                Index::create_in_dir(path, schema)?
            }
            None => Index::create_in_ram(schema),
        };
        crate::analyzer::register(&index);
        let mut writer = index.writer(15_000_000)?;
        let mut metadata = BTreeMap::new();
        for document in documents {
            let id = document.resource_ref.as_uuid().to_string();
            let mut indexed = doc!(
                fields.resource_ref => id.as_str(),
                fields.kind => kind_token(document.kind),
                fields.canonical_exact => normalize_exact(&document.canonical_name),
                fields.canonical_name => document.canonical_name.as_str()
            );
            if let Some(title) = &document.title {
                indexed.add_text(fields.title, title);
                indexed.add_text(fields.title_exact, normalize_exact(title));
            }
            for alias in &document.aliases {
                indexed.add_text(fields.aliases, alias);
                indexed.add_text(fields.aliases_exact, normalize_exact(alias));
            }
            if let Some(high_signal_text) = &document.high_signal_text {
                indexed.add_text(fields.high_signal_text, high_signal_text);
            }
            if let Some(body) = &document.body {
                indexed.add_text(fields.body, &body.text);
            }
            writer.add_document(indexed)?;
            metadata.insert(
                id,
                DocumentMetadata {
                    resource_ref: document.resource_ref,
                    kind: document.kind,
                    locator: document.locator,
                    provenance: source.provenance.clone(),
                },
            );
        }
        writer.commit()?;
        writer.wait_merging_threads()?;
        let reader = index.reader()?;
        reader.reload()?;
        let generation = Arc::new(GenerationIndex {
            index,
            reader,
            fields,
            documents: metadata,
            units: unit_index,
            tokenizer,
        });
        let mut generations = self
            .generations
            .write()
            .map_err(|_| LexicalIndexError::LockPoisoned)?;
        if generations.contains_key(&key) {
            return Err(LexicalIndexError::DuplicateGeneration);
        }
        generations.insert(key, generation);
        Ok(())
    }

    /// Searchable Unit documents of a body-ready generation, read back from the
    /// committed index rather than from builder inputs.
    pub fn enumerate_unit_docs(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<Vec<IndexedUnitDoc>, LexicalIndexError> {
        let generation = self.generation(key)?;
        let units = generation
            .units
            .as_ref()
            .ok_or(LexicalIndexError::NotBodyReady)?;
        enumerate(key, units)
    }

    pub fn is_body_ready(&self, key: ProjectionGenerationKey) -> Result<bool, LexicalIndexError> {
        Ok(self.generation(key)?.units.is_some())
    }

    /// Test-only corruption of a committed Unit index, used to prove that seals
    /// compare the actual index instead of builder inputs.
    #[cfg(feature = "fault-injection")]
    pub fn inject_unit_fault(
        &self,
        key: ProjectionGenerationKey,
        fault: UnitIndexFault,
    ) -> Result<(), LexicalIndexError> {
        let mut generations = self
            .generations
            .write()
            .map_err(|_| LexicalIndexError::LockPoisoned)?;
        let current = generations
            .get(&key)
            .cloned()
            .ok_or(LexicalIndexError::UnknownGeneration)?;
        let units = current
            .units
            .as_ref()
            .ok_or(LexicalIndexError::NotBodyReady)?;
        let mut docs = enumerate(key, units)?;
        match fault {
            UnitIndexFault::DropFirst => {
                docs.remove(0);
            }
            UnitIndexFault::DuplicateFirst => docs.push(docs[0].clone()),
            UnitIndexFault::ReplaceFirstText(text) => docs[0].text = text,
        }
        let rebuilt: Vec<KnowledgeUnit> = docs
            .into_iter()
            .map(|doc| KnowledgeUnit {
                unit_id: doc.unit_id,
                version: doc.version,
                part: doc.part,
                parent_unit_id: None,
                ordinal: doc.ordinal,
                kind: doc.kind,
                text: doc.text,
                locator: doc.locator,
                text_sha256: doc.text_sha256,
                provenance: search_core::knowledge_unit::UnitProvenance {
                    source_snapshot: String::new(),
                    authoritative_representation_ref: doc.authoritative_representation_ref,
                    raw: doc.raw,
                    detected_format: search_core::knowledge_unit::FormatId::Text,
                    archive_inner_format: None,
                    profile: doc.profile,
                    parser_build_id: "fault".into(),
                },
            })
            .collect();
        let replaced = GenerationIndex {
            index: current.index.clone(),
            reader: current.index.reader()?,
            fields: current.fields,
            documents: current
                .documents
                .iter()
                .map(|(id, metadata)| {
                    (
                        id.clone(),
                        DocumentMetadata {
                            resource_ref: metadata.resource_ref,
                            kind: metadata.kind,
                            locator: metadata.locator.clone(),
                            provenance: metadata.provenance.clone(),
                        },
                    )
                })
                .collect(),
            units: Some(build_unit_index(
                &rebuilt.iter().collect::<Vec<_>>(),
                current.tokenizer,
            )?),
            tokenizer: current.tokenizer,
        };
        generations.insert(key, Arc::new(replaced));
        Ok(())
    }

    pub(crate) fn generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<Arc<GenerationIndex>, LexicalIndexError> {
        self.generations
            .read()
            .map_err(|_| LexicalIndexError::LockPoisoned)?
            .get(&key)
            .cloned()
            .ok_or(LexicalIndexError::UnknownGeneration)
    }
}

/// Committed-index corruptions available only with the fault-injection feature.
#[cfg(feature = "fault-injection")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitIndexFault {
    DropFirst,
    DuplicateFirst,
    ReplaceFirstText(String),
}
