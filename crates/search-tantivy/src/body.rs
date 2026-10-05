//! P1-E01 body-ready lexical generations (`schema-2`). Unit documents live in a
//! separately typed index inside the generation, so Resource ranking and its
//! statistics are unchanged. Enumeration reads the actually committed index.

use search_core::id::ResourceId;
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, KnowledgeUnit, NativeLocator, RawBinding,
    ResourceVersionRef, UnitId, UnitKind,
};
use search_core::projection::ProjectionGenerationKey;
use sha2::{Digest, Sha256};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions, Value,
};
use tantivy::{Index, IndexReader, doc};

use crate::index::{LexicalBuildInput, LexicalIndexError};
use crate::schema::{ANALYZER_VERSION, kind_token};

/// Lexical schema of a body-ready generation.
pub const BODY_LEXICAL_SCHEMA_VERSION: &str = "schema-2";

/// One searchable Unit document reconstructed from stored index values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedUnitDoc {
    pub generation: ProjectionGenerationKey,
    pub parent_resource: ResourceId,
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub authoritative_representation_ref: String,
    pub raw: RawBinding,
    pub unit_id: UnitId,
    pub ordinal: u32,
    pub kind: UnitKind,
    pub locator: NativeLocator,
    pub profile: ExtractionProfileId,
    pub text_sha256: [u8; 32],
    pub text: String,
}

#[derive(Clone, Copy)]
pub(crate) struct UnitFields {
    pub unit_id: Field,
    pub parent_resource: Field,
    pub metadata: Field,
    pub body: Field,
}

pub(crate) struct UnitIndex {
    pub index: Index,
    pub reader: IndexReader,
    pub fields: UnitFields,
}

fn unit_schema() -> (Schema, UnitFields) {
    let mut builder = Schema::builder();
    let body = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let fields = UnitFields {
        unit_id: builder.add_text_field("unit_id", STRING | STORED),
        parent_resource: builder.add_text_field("parent_resource", STRING | STORED),
        metadata: builder.add_text_field("unit_metadata", STORED),
        body: builder.add_text_field("body", body),
    };
    (builder.build(), fields)
}

fn metadata_json(unit: &KnowledgeUnit) -> Result<String, LexicalIndexError> {
    // The body text is stored once, in the searchable body field.
    let mut stored = unit.clone();
    stored.text = String::new();
    serde_json::to_string(&stored).map_err(|_| LexicalIndexError::UnitEncoding)
}

pub(crate) fn build_unit_index(units: &[KnowledgeUnit]) -> Result<UnitIndex, LexicalIndexError> {
    let (schema, fields) = unit_schema();
    fill_unit_index(Index::create_in_ram(schema), fields, units)
}

pub(crate) fn build_unit_index_at(
    units: &[KnowledgeUnit],
    dir: &std::path::Path,
) -> Result<UnitIndex, LexicalIndexError> {
    std::fs::create_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
    let (schema, fields) = unit_schema();
    fill_unit_index(Index::create_in_dir(dir, schema)?, fields, units)
}

/// Opens a committed Unit index and checks it has the Unit schema.
pub(crate) fn open_unit_index(dir: &std::path::Path) -> Result<UnitIndex, LexicalIndexError> {
    let index = Index::open_in_dir(dir)?;
    let (schema, fields) = unit_schema();
    if index.schema() != schema {
        return Err(LexicalIndexError::PersistedMismatch);
    }
    let reader = index.reader()?;
    reader.reload()?;
    Ok(UnitIndex {
        index,
        reader,
        fields,
    })
}

fn fill_unit_index(
    index: Index,
    fields: UnitFields,
    units: &[KnowledgeUnit],
) -> Result<UnitIndex, LexicalIndexError> {
    let mut writer = index.writer(15_000_000)?;
    for unit in units {
        writer.add_document(doc!(
            fields.unit_id => unit.unit_id.to_string(),
            fields.parent_resource => unit.version.resource_id.as_uuid().to_string(),
            fields.metadata => metadata_json(unit)?,
            fields.body => unit.text.as_str()
        ))?;
    }
    writer.commit()?;
    writer.wait_merging_threads()?;
    let reader = index.reader()?;
    reader.reload()?;
    Ok(UnitIndex {
        index,
        reader,
        fields,
    })
}

/// Every committed Unit document, decoded back to the stored KnowledgeUnit.
pub(crate) fn stored_units(units: &UnitIndex) -> Result<Vec<KnowledgeUnit>, LexicalIndexError> {
    let searcher = units.reader.searcher();
    let mut out = Vec::new();
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        let alive = segment.alive_bitset();
        for doc_id in 0..segment.max_doc() {
            if alive.is_some_and(|bits| !bits.is_alive(doc_id)) {
                continue;
            }
            let document: TantivyDocument = searcher.doc(tantivy::DocAddress::new(
                u32::try_from(segment_ord).map_err(|_| LexicalIndexError::UnitEncoding)?,
                doc_id,
            ))?;
            out.push(read_unit(&document, units.fields)?);
        }
    }
    Ok(out)
}

pub(crate) fn enumerate(
    key: ProjectionGenerationKey,
    units: &UnitIndex,
) -> Result<Vec<IndexedUnitDoc>, LexicalIndexError> {
    let searcher = units.reader.searcher();
    let mut docs = Vec::new();
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        let alive = segment.alive_bitset();
        for doc_id in 0..segment.max_doc() {
            if alive.is_some_and(|bits| !bits.is_alive(doc_id)) {
                continue;
            }
            let document: TantivyDocument = searcher.doc(tantivy::DocAddress::new(
                u32::try_from(segment_ord).map_err(|_| LexicalIndexError::UnitEncoding)?,
                doc_id,
            ))?;
            let unit = read_unit(&document, units.fields)?;
            docs.push(IndexedUnitDoc {
                generation: key,
                parent_resource: unit.version.resource_id,
                authoritative_representation_ref: unit.provenance.authoritative_representation_ref,
                raw: unit.provenance.raw,
                profile: unit.provenance.profile,
                version: unit.version,
                part: unit.part,
                unit_id: unit.unit_id,
                ordinal: unit.ordinal,
                kind: unit.kind,
                locator: unit.locator,
                text_sha256: unit.text_sha256,
                text: unit.text,
            });
        }
    }
    docs.sort_by_key(|doc| doc.unit_id);
    Ok(docs)
}

/// Rebuilds the stored Unit and rejects documents whose indexed identity
/// fields disagree with the stored metadata.
pub(crate) fn read_unit(
    document: &TantivyDocument,
    fields: UnitFields,
) -> Result<KnowledgeUnit, LexicalIndexError> {
    let text = |field| {
        document
            .get_first(field)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or(LexicalIndexError::UnitEncoding)
    };
    let mut unit: KnowledgeUnit = serde_json::from_str(&text(fields.metadata)?)
        .map_err(|_| LexicalIndexError::UnitEncoding)?;
    unit.text = text(fields.body)?;
    if text(fields.unit_id)? != unit.unit_id.to_string()
        || text(fields.parent_resource)? != unit.version.resource_id.as_uuid().to_string()
    {
        return Err(LexicalIndexError::UnitEncoding);
    }
    Ok(unit)
}

/// Deterministic digest of the staged lexical input (never Tantivy segment bytes):
/// schema/analyzer, every permitted Resource field and each Unit's parent,
/// identity, part, locator, text digest and body text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexicalInputDigest {
    pub digest: [u8; 32],
    pub count: u64,
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u32).to_be_bytes());
    hasher.update(bytes);
}

fn optional(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        None => hasher.update([0]),
        Some(value) => {
            hasher.update([1]);
            frame(hasher, value.as_bytes());
        }
    }
}

pub fn lexical_input_digest(
    input: &LexicalBuildInput,
) -> Result<LexicalInputDigest, LexicalIndexError> {
    let mut hasher = Sha256::new();
    hasher.update(b"document-lexical-input:v1\0");
    let schema = if input.body_units().is_some() {
        BODY_LEXICAL_SCHEMA_VERSION
    } else {
        crate::schema::LEXICAL_SCHEMA_VERSION
    };
    frame(&mut hasher, schema.as_bytes());
    frame(&mut hasher, ANALYZER_VERSION.as_bytes());
    let mut documents: Vec<_> = input.documents().iter().collect();
    documents.sort_by_key(|document| document.resource_ref);
    hasher.update((documents.len() as u32).to_be_bytes());
    for document in &documents {
        hasher.update(document.resource_ref.as_uuid().as_bytes());
        frame(&mut hasher, kind_token(document.kind).as_bytes());
        frame(&mut hasher, document.canonical_name.as_bytes());
        optional(&mut hasher, document.title.as_deref());
        hasher.update((document.aliases.len() as u32).to_be_bytes());
        for alias in &document.aliases {
            frame(&mut hasher, alias.as_bytes());
        }
        optional(&mut hasher, document.high_signal_text.as_deref());
        optional(&mut hasher, document.body.as_ref().map(|body| body.text()));
        optional(&mut hasher, document.locator.as_deref());
    }
    let units = input.body_units().unwrap_or(&[]);
    let mut ordered: Vec<&KnowledgeUnit> = units.iter().collect();
    ordered.sort_by(|left, right| {
        (
            left.version.resource_id,
            left.part.ordinal,
            &left.part.logical_path,
            &left.part.source_native_part_id,
            left.ordinal,
        )
            .cmp(&(
                right.version.resource_id,
                right.part.ordinal,
                &right.part.logical_path,
                &right.part.source_native_part_id,
                right.ordinal,
            ))
    });
    hasher.update((ordered.len() as u32).to_be_bytes());
    for unit in &ordered {
        hasher.update(unit.version.resource_id.as_uuid().as_bytes());
        frame(&mut hasher, unit.unit_id.to_string().as_bytes());
        frame(&mut hasher, unit.part.source_native_part_id.as_bytes());
        frame(&mut hasher, unit.part.logical_path.as_bytes());
        hasher.update(unit.part.ordinal.to_be_bytes());
        frame(
            &mut hasher,
            &unit
                .locator
                .encode()
                .map_err(|_| LexicalIndexError::UnitEncoding)?,
        );
        hasher.update(unit.text_sha256);
        frame(&mut hasher, unit.text.as_bytes());
    }
    Ok(LexicalInputDigest {
        digest: hasher.finalize().into(),
        count: (documents.len() + ordered.len()) as u64,
    })
}
