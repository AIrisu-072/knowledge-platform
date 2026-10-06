//! P7-05 support: a lexical generation written to a directory and reopened
//! from it. Resource text fields are indexed but not stored, so the
//! Source-supplied input travels as a sidecar; Units are read back from the
//! committed Unit index. Reopening never trusts builder-side counts.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use search_core::id::{ProjectionGenerationId, SourceId};
use search_core::projection::ProjectionGenerationManifest;
use search_core::source::DiscoverableSource;
use serde::{Deserialize, Serialize};
use tantivy::schema::{TantivyDocument, Value};
use tantivy::{Index, IndexReader};

use crate::body::{
    BODY_LEXICAL_SCHEMA_VERSION, IndexedUnitDoc, LexicalInputDigest, UnitIndex, enumerate,
    lexical_input_digest, open_unit_index, stored_units,
};
use crate::index::{
    DocumentMetadata, GenerationIndex, LexicalBuildInput, LexicalDocument, LexicalIndexError,
    TantivyLexicalIndex,
};
use crate::schema::{LEXICAL_SCHEMA_VERSION, LexicalFields, lexical_schema};

pub(crate) const RESOURCES_DIR: &str = "resources";
pub(crate) const UNITS_DIR: &str = "units";
pub(crate) const SIDECAR: &str = "lexical-input.json";
const FORMAT: &str = "lexical-generation:v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedInput {
    format: String,
    source_id: SourceId,
    generation_id: ProjectionGenerationId,
    source_snapshot: String,
    projection_schema_version: String,
    lens_version: u32,
    analyzer_version: String,
    provenance: Option<String>,
    documents: Vec<LexicalDocument>,
    body_ready: bool,
}

pub(crate) fn write_sidecar(
    dir: &Path,
    manifest: &ProjectionGenerationManifest,
    source: &DiscoverableSource,
    input: &LexicalBuildInput,
) -> Result<(), LexicalIndexError> {
    std::fs::create_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
    let sidecar = PersistedInput {
        format: FORMAT.into(),
        source_id: manifest.source_id,
        generation_id: manifest.generation_id,
        source_snapshot: manifest.source_snapshot.clone(),
        projection_schema_version: manifest.projection_schema_version.clone(),
        lens_version: manifest.lens_version,
        analyzer_version: input.analyzer_version().into(),
        provenance: source.provenance.clone(),
        documents: input.documents().to_vec(),
        body_ready: input.body_units().is_some(),
    };
    let bytes = serde_json::to_vec(&sidecar).map_err(|_| LexicalIndexError::PersistedMismatch)?;
    std::fs::write(dir.join(SIDECAR), bytes).map_err(|_| LexicalIndexError::Io)
}

/// What a reopened generation directory actually contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedLexical {
    /// P1 lexical input digest recomputed from the sidecar and stored Units.
    pub logical: LexicalInputDigest,
    pub schema_version: &'static str,
    pub analyzer_version: &'static str,
    pub resource_docs: u64,
    /// Searchable Unit documents read back from the committed Unit index.
    pub units: Vec<IndexedUnitDoc>,
}

struct Opened {
    persisted: PersistedLexical,
    index: Index,
    reader: IndexReader,
    tokenizer: &'static str,
    fields: LexicalFields,
    documents: BTreeMap<String, DocumentMetadata>,
    units: Option<UnitIndex>,
}

fn mismatch() -> LexicalIndexError {
    LexicalIndexError::PersistedMismatch
}

fn open(
    manifest: &ProjectionGenerationManifest,
    source: &DiscoverableSource,
    dir: &Path,
) -> Result<Opened, LexicalIndexError> {
    let raw = std::fs::read(dir.join(SIDECAR)).map_err(|_| LexicalIndexError::Io)?;
    let sidecar: PersistedInput = serde_json::from_slice(&raw).map_err(|_| mismatch())?;
    if sidecar.format != FORMAT
        || sidecar.source_id != manifest.source_id
        || source.source_id != manifest.source_id
        || sidecar.generation_id != manifest.generation_id
        || sidecar.source_snapshot != manifest.source_snapshot
        || sidecar.projection_schema_version != manifest.projection_schema_version
        || sidecar.lens_version != manifest.lens_version
        || manifest.analyzer_version.as_deref() != Some(sidecar.analyzer_version.as_str())
        || sidecar.provenance != source.provenance
    {
        return Err(mismatch());
    }
    // Generations built with an earlier supported analyzer stay readable.
    let analyzer_version =
        crate::analyzer::supported(&sidecar.analyzer_version).ok_or_else(mismatch)?;
    let tokenizer = crate::analyzer::tokenizer_name(analyzer_version).ok_or_else(mismatch)?;
    let index = Index::open_in_dir(dir.join(RESOURCES_DIR))?;
    crate::analyzer::register(&index);
    let (schema, fields) = lexical_schema(tokenizer);
    if index.schema() != schema {
        return Err(mismatch());
    }
    let reader = index.reader()?;
    reader.reload()?;
    let searcher = reader.searcher();
    let mut stored_refs = Vec::new();
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        let alive = segment.alive_bitset();
        for doc_id in 0..segment.max_doc() {
            if alive.is_some_and(|bits| !bits.is_alive(doc_id)) {
                continue;
            }
            let document: TantivyDocument = searcher.doc(tantivy::DocAddress::new(
                u32::try_from(segment_ord).map_err(|_| mismatch())?,
                doc_id,
            ))?;
            let id = document
                .get_first(fields.resource_ref)
                .and_then(|value| value.as_str())
                .ok_or_else(mismatch)?;
            stored_refs.push(id.to_owned());
        }
    }
    let mut documents = sidecar.documents;
    documents.sort_by_key(|document| document.resource_ref);
    let expected: Vec<String> = documents
        .iter()
        .map(|document| document.resource_ref.as_uuid().to_string())
        .collect();
    stored_refs.sort();
    if stored_refs != expected || expected.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(mismatch());
    }
    let units_dir = dir.join(UNITS_DIR);
    let units = if sidecar.body_ready {
        Some(open_unit_index(&units_dir, tokenizer)?)
    } else if units_dir.exists() {
        return Err(mismatch());
    } else {
        None
    };
    let mut input = LexicalBuildInput::new(
        manifest.source_id,
        manifest.source_snapshot.clone(),
        manifest.projection_schema_version.clone(),
        manifest.lens_version,
        documents.clone(),
    )
    .with_analyzer_version(analyzer_version);
    let unit_docs = match &units {
        Some(units) => {
            input = input.with_body_units(stored_units(units)?);
            enumerate(manifest.key(), units)?
        }
        None => Vec::new(),
    };
    let logical = lexical_input_digest(&input)?;
    let metadata = documents
        .iter()
        .map(|document| {
            (
                document.resource_ref.as_uuid().to_string(),
                DocumentMetadata {
                    resource_ref: document.resource_ref,
                    kind: document.kind,
                    locator: document.locator.clone(),
                    provenance: sidecar.provenance.clone(),
                },
            )
        })
        .collect();
    Ok(Opened {
        persisted: PersistedLexical {
            logical,
            schema_version: if sidecar.body_ready {
                BODY_LEXICAL_SCHEMA_VERSION
            } else {
                LEXICAL_SCHEMA_VERSION
            },
            analyzer_version,
            resource_docs: u64::try_from(documents.len()).map_err(|_| mismatch())?,
            units: unit_docs,
        },
        index,
        reader,
        tokenizer,
        fields,
        documents: metadata,
        units,
    })
}

impl TantivyLexicalIndex {
    /// Reopens a generation directory without registering it and recomputes
    /// everything from the committed indexes and the sidecar.
    pub fn inspect_persisted(
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        dir: &Path,
    ) -> Result<PersistedLexical, LexicalIndexError> {
        open(manifest, source, dir).map(|opened| opened.persisted)
    }

    /// Reopens a persisted generation and registers it for queries.
    pub fn load_generation_at(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        dir: &Path,
    ) -> Result<PersistedLexical, LexicalIndexError> {
        let opened = open(manifest, source, dir)?;
        let generation = Arc::new(GenerationIndex {
            index: opened.index,
            reader: opened.reader,
            fields: opened.fields,
            documents: opened.documents,
            units: opened.units,
            tokenizer: opened.tokenizer,
        });
        let mut generations = self
            .generations
            .write()
            .map_err(|_| LexicalIndexError::LockPoisoned)?;
        if generations.contains_key(&manifest.key()) {
            return Err(LexicalIndexError::DuplicateGeneration);
        }
        generations.insert(manifest.key(), generation);
        Ok(opened.persisted)
    }
}
