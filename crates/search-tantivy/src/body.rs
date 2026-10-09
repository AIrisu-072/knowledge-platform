//! P1-E01 body-ready lexical generations (`schema-2`). Unit documents live in a
//! separately typed index inside the generation, so Resource ranking and its
//! statistics are unchanged. Enumeration reads the actually committed index.
//!
//! A generation may start from the Unit index of an earlier one (SD-T11 5):
//! its segment files are hard-linked, never rewritten, Units that changed or
//! disappeared are deleted and new ones added in one commit. Decoded segments
//! are cached per process by segment and store-file identity, so an unchanged
//! segment is read and checked once per process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use search_core::id::ResourceId;
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, KnowledgeUnit, NativeLocator, RawBinding,
    ResourceVersionRef, UnitId, UnitKind,
};
use search_core::projection::ProjectionGenerationKey;
use sha2::{Digest, Sha256};
use tantivy::indexer::NoMergePolicy;
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions, Value,
};
use tantivy::{Index, IndexReader, SegmentReader, Term, doc};

use crate::index::{LexicalBuildInput, LexicalIndexError};
use crate::schema::kind_token;

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
    /// The directory of a persisted index; decoded segments are cached only then.
    pub dir: Option<PathBuf>,
}

/// One searchable Unit document reduced to its ID and the digest of every
/// stored field and its text (`unit_doc_hash`). Reopening, sealing and the
/// logical digest work on these instead of holding every Unit's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSealEntry {
    pub unit_id: UnitId,
    pub hash: [u8; 32],
}

/// `document-unit:v1`: SHA-256 of the Unit's JSON with its text and Source
/// snapshot removed, followed by its text. Equal for the builder's Unit and
/// the one read back from the index.
pub fn unit_doc_hash(unit: &KnowledgeUnit) -> Result<[u8; 32], LexicalIndexError> {
    let mut hasher = Sha256::new();
    hasher.update(b"document-unit:v1\0");
    frame(&mut hasher, metadata_json(unit)?.as_bytes());
    frame(&mut hasher, unit.text.as_bytes());
    Ok(hasher.finalize().into())
}

/// The entries of one segment by doc id (deleted docs included).
type SegmentEntries = Arc<Vec<UnitSealEntry>>;

/// Segments read in this process: segment ID and the identity of its store
/// file (device, inode, size, modification time; linking changes the change
/// time, so it is not part of the identity) to their entries and the last
/// read pass that used them.
#[derive(Default)]
struct SegmentCache {
    pass: u64,
    segments: HashMap<(String, [u64; 5]), (u64, SegmentEntries)>,
}

fn segment_cache() -> &'static Mutex<SegmentCache> {
    static CACHE: OnceLock<Mutex<SegmentCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Segments with at least this many documents are read in parallel.
const PARALLEL_READ_MIN_DOCS: u32 = 50_000;
/// Threads reading one segment.
const PARALLEL_READ_THREADS: usize = 8;

/// Read passes a cached segment stays unused before it is dropped: the
/// segments of the index read last and of the one read before it (a build's
/// base and its result) stay, those a merge replaced do not.
const KEPT_PASSES: u64 = 2;

#[cfg(unix)]
fn file_identity(path: &Path) -> Option<[u64; 5]> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some([
        meta.dev(),
        meta.ino(),
        meta.size(),
        u64::try_from(meta.mtime()).ok()?,
        u64::try_from(meta.mtime_nsec()).ok()?,
    ])
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Option<[u64; 5]> {
    None
}

fn segment_entries(
    units: &UnitIndex,
    segment_ord: usize,
    segment: &SegmentReader,
    pass: u64,
) -> Result<SegmentEntries, LexicalIndexError> {
    let id = segment.segment_id().uuid_string();
    let key = units.dir.as_ref().and_then(|dir| {
        file_identity(&dir.join(format!("{id}.store"))).map(|identity| (id.clone(), identity))
    });
    if let Some(key) = &key
        && let Some((used, hit)) = segment_cache()
            .lock()
            .map_err(|_| LexicalIndexError::LockPoisoned)?
            .segments
            .get_mut(key)
        && hit.len() == segment.max_doc() as usize
    {
        *used = pass;
        return Ok(hit.clone());
    }
    let searcher = units.reader.searcher();
    let ord = u32::try_from(segment_ord).map_err(|_| LexicalIndexError::UnitEncoding)?;
    let read = |docs: std::ops::Range<u32>| {
        docs.map(|doc_id| {
            let document: TantivyDocument = searcher.doc(tantivy::DocAddress::new(ord, doc_id))?;
            let unit = read_unit(&document, units.fields)?;
            Ok(UnitSealEntry {
                unit_id: unit.unit_id,
                hash: unit_doc_hash(&unit)?,
            })
        })
        .collect::<Result<Vec<_>, LexicalIndexError>>()
    };
    // Reading and hashing every stored document dominates a cold seal; a
    // large segment is read in parallel ranges, joined in document order.
    let max_doc = segment.max_doc();
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(PARALLEL_READ_THREADS);
    let entries = if max_doc < PARALLEL_READ_MIN_DOCS || threads < 2 {
        read(0..max_doc)?
    } else {
        let step = max_doc.div_ceil(threads as u32);
        let parts = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..max_doc)
                .step_by(step as usize)
                .map(|start| {
                    let read = &read;
                    scope.spawn(move || read(start..(start + step).min(max_doc)))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or(Err(LexicalIndexError::UnitEncoding))
                })
                .collect::<Vec<_>>()
        });
        let mut entries = Vec::with_capacity(max_doc as usize);
        for part in parts {
            entries.extend(part?);
        }
        entries
    };
    let entries = Arc::new(entries);
    if let Some(key) = key {
        segment_cache()
            .lock()
            .map_err(|_| LexicalIndexError::LockPoisoned)?
            .segments
            .insert(key, (pass, entries.clone()));
    }
    Ok(entries)
}

/// The entry of every live Unit document, read one document at a time.
pub(crate) fn unit_entries(units: &UnitIndex) -> Result<Vec<UnitSealEntry>, LexicalIndexError> {
    let pass = {
        let mut cache = segment_cache()
            .lock()
            .map_err(|_| LexicalIndexError::LockPoisoned)?;
        cache.pass += 1;
        cache.pass
    };
    let searcher = units.reader.searcher();
    let mut out = Vec::new();
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        let entries = segment_entries(units, segment_ord, segment, pass)?;
        let alive = segment.alive_bitset();
        for (doc_id, entry) in entries.iter().enumerate() {
            let doc_id = u32::try_from(doc_id).map_err(|_| LexicalIndexError::UnitEncoding)?;
            if alive.is_some_and(|bits| !bits.is_alive(doc_id)) {
                continue;
            }
            out.push(*entry);
        }
    }
    segment_cache()
        .lock()
        .map_err(|_| LexicalIndexError::LockPoisoned)?
        .segments
        .retain(|_, (used, _)| *used + KEPT_PASSES > pass);
    Ok(out)
}

/// Every live Unit of the committed index, decoded (no cache).
fn live_units(units: &UnitIndex) -> Result<Vec<KnowledgeUnit>, LexicalIndexError> {
    let searcher = units.reader.searcher();
    let mut out = Vec::new();
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        let alive = segment.alive_bitset();
        let ord = u32::try_from(segment_ord).map_err(|_| LexicalIndexError::UnitEncoding)?;
        for doc_id in 0..segment.max_doc() {
            if alive.is_some_and(|bits| !bits.is_alive(doc_id)) {
                continue;
            }
            let document: TantivyDocument = searcher.doc(tantivy::DocAddress::new(ord, doc_id))?;
            out.push(read_unit(&document, units.fields)?);
        }
    }
    Ok(out)
}

fn unit_schema(tokenizer: &str) -> (Schema, UnitFields) {
    let mut builder = Schema::builder();
    let body = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(tokenizer)
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
    // The body text is stored once, in the searchable body field. The Source
    // snapshot is per generation and not stored, so a linked segment fits any.
    let mut stored = unit.clone();
    stored.text = String::new();
    Arc::make_mut(&mut stored.provenance)
        .source_snapshot
        .clear();
    serde_json::to_string(&stored).map_err(|_| LexicalIndexError::UnitEncoding)
}

pub(crate) fn build_unit_index(
    units: &[&KnowledgeUnit],
    tokenizer: &str,
) -> Result<UnitIndex, LexicalIndexError> {
    let (schema, fields) = unit_schema(tokenizer);
    fill_unit_index(Index::create_in_ram(schema), fields, units)
}

pub(crate) fn build_unit_index_at(
    units: &[&KnowledgeUnit],
    dir: &std::path::Path,
    tokenizer: &str,
) -> Result<UnitIndex, LexicalIndexError> {
    std::fs::create_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
    let (schema, fields) = unit_schema(tokenizer);
    let mut built = fill_unit_index(Index::create_in_dir(dir, schema)?, fields, units)?;
    built.dir = Some(dir.to_path_buf());
    Ok(built)
}

/// The same Units as `build_unit_index_at`, starting from the committed Unit
/// index in `base`: its files are hard-linked into `dir` and never modified,
/// Units that are gone or differ are deleted and the rest added. Returns
/// `None` (and leaves no files) when `base` cannot serve as a base; the caller
/// then builds from nothing. Merging is off, so linked segments stay shared.
pub(crate) fn build_unit_index_from_base(
    units: &[&KnowledgeUnit],
    entries: Option<Arc<Vec<UnitSealEntry>>>,
    base: &Path,
    dir: &Path,
    tokenizer: &str,
) -> Result<Option<UnitIndex>, LexicalIndexError> {
    let Ok(previous) = open_unit_index(base, tokenizer) else {
        return Ok(None);
    };
    let Ok(previous_units) = unit_entries(&previous) else {
        return Ok(None);
    };
    std::fs::create_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
    let linked = (|| -> std::io::Result<()> {
        for entry in std::fs::read_dir(base)? {
            let entry = entry?;
            let name = entry.file_name();
            let kind = entry.file_type()?;
            if kind.is_file() && !name.to_string_lossy().ends_with(".lock") {
                std::fs::hard_link(entry.path(), dir.join(&name))?;
            }
        }
        Ok(())
    })();
    if linked.is_err() {
        std::fs::remove_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
        return Ok(None);
    }
    // A base collected while it was being linked leaves an index that does
    // not open; build from nothing instead.
    let Ok(mut index) = open_unit_index(dir, tokenizer) else {
        std::fs::remove_dir_all(dir).map_err(|_| LexicalIndexError::Io)?;
        return Ok(None);
    };
    let before: HashMap<UnitId, [u8; 32]> = previous_units
        .iter()
        .map(|entry| (entry.unit_id, entry.hash))
        .collect();
    // The entries the Unit source keeps, else every Unit hashed here.
    let after: HashMap<UnitId, [u8; 32]> = match entries {
        Some(entries) if entries.len() == units.len() => entries
            .iter()
            .map(|entry| (entry.unit_id, entry.hash))
            .collect(),
        _ => units
            .iter()
            .map(|unit| Ok((unit.unit_id, unit_doc_hash(unit)?)))
            .collect::<Result<_, LexicalIndexError>>()?,
    };
    if after.len() != units.len() {
        return Err(LexicalIndexError::DuplicateUnit);
    }
    let mut writer: tantivy::IndexWriter = index.index.writer(15_000_000)?;
    writer.set_merge_policy(Box::new(NoMergePolicy));
    for (unit_id, hash) in &before {
        if after.get(unit_id) != Some(hash) {
            writer.delete_term(Term::from_field_text(
                index.fields.unit_id,
                &unit_id.to_string(),
            ));
        }
    }
    for unit in units {
        if before.get(&unit.unit_id) != after.get(&unit.unit_id) {
            writer.add_document(unit_document(index.fields, unit)?)?;
        }
    }
    writer.commit()?;
    merge_small_segments(&index.index, &mut writer)?;
    writer.wait_merging_threads()?;
    index.reader.reload()?;
    index.dir = Some(dir.to_path_buf());
    Ok(Some(index))
}

/// Segments with fewer documents than this count as small.
const SMALL_SEGMENT_DOCS: u32 = 10_000;
/// Small segments a Unit index keeps before they are merged into one. Every
/// query looks each term up in every segment: at 10,000 documents an index
/// of 707 segments (one per update) spent most of a query there.
const SMALL_SEGMENT_LIMIT: usize = 8;

/// Merges the committed small segments into one once there are more than
/// `SMALL_SEGMENT_LIMIT`; large segments, and their files, are kept.
fn merge_small_segments(
    index: &Index,
    writer: &mut tantivy::IndexWriter,
) -> Result<(), LexicalIndexError> {
    let small: Vec<_> = index
        .searchable_segment_metas()?
        .iter()
        .filter(|meta| meta.max_doc() < SMALL_SEGMENT_DOCS)
        .map(|meta| meta.id())
        .collect();
    if small.len() > SMALL_SEGMENT_LIMIT {
        writer.merge(&small).wait()?;
    }
    Ok(())
}

/// Merges every committed segment into one when there are more than
/// `SMALL_SEGMENT_LIMIT` (a full build writes many).
fn merge_all_segments(
    index: &Index,
    writer: &mut tantivy::IndexWriter,
) -> Result<(), LexicalIndexError> {
    let segments = index.searchable_segment_ids()?;
    if segments.len() > SMALL_SEGMENT_LIMIT {
        writer.merge(&segments).wait()?;
    }
    Ok(())
}

/// Opens a committed Unit index and checks it has the Unit schema.
pub(crate) fn open_unit_index(
    dir: &std::path::Path,
    tokenizer: &str,
) -> Result<UnitIndex, LexicalIndexError> {
    let index = Index::open_in_dir(dir)?;
    crate::analyzer::register(&index);
    let (schema, fields) = unit_schema(tokenizer);
    if index.schema() != schema {
        return Err(LexicalIndexError::PersistedMismatch);
    }
    let reader = index.reader()?;
    reader.reload()?;
    Ok(UnitIndex {
        index,
        reader,
        fields,
        dir: Some(dir.to_path_buf()),
    })
}

fn unit_document(
    fields: UnitFields,
    unit: &KnowledgeUnit,
) -> Result<TantivyDocument, LexicalIndexError> {
    Ok(doc!(
        fields.unit_id => unit.unit_id.to_string(),
        fields.parent_resource => unit.version.resource_id.as_uuid().to_string(),
        fields.metadata => metadata_json(unit)?,
        fields.body => unit.text.as_str()
    ))
}

fn fill_unit_index(
    index: Index,
    fields: UnitFields,
    units: &[&KnowledgeUnit],
) -> Result<UnitIndex, LexicalIndexError> {
    crate::analyzer::register(&index);
    let mut writer = index.writer(15_000_000)?;
    for unit in units {
        writer.add_document(unit_document(fields, unit)?)?;
    }
    writer.commit()?;
    merge_all_segments(&index, &mut writer)?;
    writer.wait_merging_threads()?;
    let reader = index.reader()?;
    reader.reload()?;
    Ok(UnitIndex {
        index,
        reader,
        fields,
        dir: None,
    })
}

pub(crate) fn enumerate(
    key: ProjectionGenerationKey,
    units: &UnitIndex,
) -> Result<Vec<IndexedUnitDoc>, LexicalIndexError> {
    let mut docs: Vec<IndexedUnitDoc> = live_units(units)?
        .into_iter()
        .map(|unit| IndexedUnitDoc {
            generation: key,
            parent_resource: unit.version.resource_id,
            authoritative_representation_ref: unit
                .provenance
                .authoritative_representation_ref
                .clone(),
            raw: unit.provenance.raw.clone(),
            profile: unit.provenance.profile.clone(),
            version: Arc::unwrap_or_clone(unit.version),
            part: Arc::unwrap_or_clone(unit.part),
            unit_id: unit.unit_id,
            ordinal: unit.ordinal,
            kind: unit.kind,
            locator: unit.locator,
            text_sha256: unit.text_sha256,
            text: unit.text,
        })
        .collect();
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
    let units = input.body_units();
    let entries = match input.body_seal_entries() {
        Some(entries) => entries,
        None => Arc::new(
            units
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|unit| {
                    Ok(UnitSealEntry {
                        unit_id: unit.unit_id,
                        hash: unit_doc_hash(unit)?,
                    })
                })
                .collect::<Result<Vec<_>, LexicalIndexError>>()?,
        ),
    };
    lexical_digest(
        units.is_some(),
        input.analyzer_version(),
        input.documents(),
        &entries,
    )
}

/// `document-lexical-input:v2`: schema/analyzer, every permitted Resource
/// field, then each Unit's ID and `unit_doc_hash` in Unit ID order.
pub(crate) fn lexical_digest(
    body_ready: bool,
    analyzer_version: &str,
    documents: &[crate::index::LexicalDocument],
    entries: &[UnitSealEntry],
) -> Result<LexicalInputDigest, LexicalIndexError> {
    let mut hasher = Sha256::new();
    hasher.update(b"document-lexical-input:v2\0");
    let schema = if body_ready {
        BODY_LEXICAL_SCHEMA_VERSION
    } else {
        crate::schema::LEXICAL_SCHEMA_VERSION
    };
    frame(&mut hasher, schema.as_bytes());
    frame(&mut hasher, analyzer_version.as_bytes());
    let mut documents: Vec<_> = documents.iter().collect();
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
    let mut ordered: Vec<&UnitSealEntry> = entries.iter().collect();
    ordered.sort_unstable_by_key(|entry| entry.unit_id);
    hasher.update((ordered.len() as u32).to_be_bytes());
    for entry in &ordered {
        frame(&mut hasher, &entry.unit_id.text_bytes());
        hasher.update(entry.hash);
    }
    Ok(LexicalInputDigest {
        digest: hasher.finalize().into(),
        count: (documents.len() + ordered.len()) as u64,
    })
}
