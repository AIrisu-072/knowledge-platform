//! P7-04: typed durable payloads of one generation bundle.
//!
//! Each stored DTO is restored through `deny_unknown_fields` types and every
//! digest is recomputed from the restored values with the P1 encoders; a stored
//! digest column is never trusted on its own. Lexical index bytes and Graph
//! rows are verified by their own owners (P7-05 / P3) before READY.
//!
//! The Unit manifest is stored as one content-addressed segment per item
//! (SD-T11 5), shared by every generation that has the same item; the
//! `unit_manifest` payload row keeps only its header. A segment is verified
//! before it is written and again when this process first reads it, then
//! kept in a process-wide cache so an unchanged item is not read twice.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use search_application::ports::SemanticRegistrySnapshot;
use search_application::search_core::knowledge_unit::{
    restamp_source_snapshot, share_unit_context,
};
use search_application::search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_projection_memory::generation_digest;
use search_source_document::{
    BodyCoverageArtifact, BodyCoverageItem, BodyItemEntry, BodyUnitManifest,
    GenerationBundleReceipt, compute_bundle_receipt_from, profile_set_digest, segment_digest,
    unit_manifest_receipt_from_segments, validate_restored_manifest_skipping,
};
use search_tantivy::{UnitSealEntry, unit_doc_hash};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use sqlx::{Connection as _, PgConnection, PgPool, Row};

pub const PAYLOAD_DTO_VERSION: &str = "v1";
/// Upper bound of one restored payload document, in bytes of its JSON text.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024 * 1024;
/// A payload whose JSON text is longer is stored as ordered text chunks of
/// at most this many bytes, below the 256 MiB limit of one JSONB value.
const CHUNK_BYTES: usize = 64 * 1024 * 1024;
/// Recorded on every segment this writer verified before inserting it.
pub const SEGMENT_VERIFIER: &str = "search-runtime-payload-v1";
/// Verified segments kept per process; above this the cache keeps only the
/// segments of the generation being read.
const SEGMENT_CACHE_ITEMS: usize = 200_000;
/// Units kept in cached segments (about 1.5 KB each). A generation with more
/// is not kept at all, so a large Source is never held twice (T12).
const SEGMENT_CACHE_UNITS: usize = 250_000;

/// The `unit_manifest` payload row of a segmented generation. Its items are
/// the generation's ordered `search_generation_segment` rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitManifestHeaderV1 {
    key: ProjectionGenerationKey,
    source_snapshot: String,
    segments: u64,
}

/// Segments verified by this process, by digest text, without their Source
/// snapshot (each generation rebinds its own).
fn segment_cache() -> &'static Mutex<HashMap<String, Arc<BodyItemEntry>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<BodyItemEntry>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Forgets every verified segment, as a restarted process would; the next
/// read of each segment verifies it again.
pub fn forget_verified_segments() {
    if let Ok(mut cache) = segment_cache().lock() {
        cache.clear();
    }
    if let Ok(mut cache) = summary_cache().lock() {
        cache.clear();
    }
    if let Ok(mut cache) = restored_summaries().lock() {
        cache.clear();
    }
}

/// An item as stored in a segment: every Unit's Source snapshot is cleared.
fn unbound(entry: &BodyItemEntry) -> BodyItemEntry {
    let mut stored = entry.clone();
    restamp_source_snapshot(&mut stored.units, "");
    stored
}

/// The digest `entry` is stored under as a Unit segment.
pub fn stored_segment_digest(entry: &BodyItemEntry) -> Result<String, BundleError> {
    Ok(sha256_text(
        &segment_digest(&unbound(entry)).map_err(|_| BundleError::Digest)?,
    ))
}

/// Checks a segment read from the database before it is cached: its digest,
/// Unit count and every Unit's text digest.
fn verified_segment(digest: &str, count: i64, text: &str) -> Result<BodyItemEntry, BundleError> {
    let mut entry = restore::<BodyItemEntry>(text)?;
    // Decoding gives each Unit its own copy of the Part's shared fields.
    share_unit_context(&mut entry.units);
    if sha256_text(&segment_digest(&entry).map_err(|_| BundleError::Digest)?) != digest
        || u64::try_from(count).ok() != Some(entry.units.len() as u64)
    {
        return Err(BundleError::Digest);
    }
    for unit in &entry.units {
        let actual: [u8; 32] = sha2::Sha256::digest(unit.text.as_bytes()).into();
        if actual != unit.text_sha256 || !unit.provenance.source_snapshot.is_empty() {
            return Err(BundleError::Digest);
        }
    }
    Ok(entry)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPayloadV1 {
    pub resources: Vec<CompiledResourceProjection>,
    pub registry: SemanticRegistrySnapshot,
}

/// Every durable part of one bundle except external artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBundleV1 {
    pub manifest: ProjectionGenerationManifest,
    pub projection: ProjectionPayloadV1,
    pub unit_manifest: BodyUnitManifest,
    pub coverage: BodyCoverageArtifact,
    pub receipt: GenerationBundleReceipt,
}

/// Restored payload DTOs, checked except for the external artifact receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredPayloadV1 {
    pub projection: ProjectionPayloadV1,
    pub unit_manifest: BodyUnitManifest,
    pub coverage: BodyCoverageArtifact,
}

type RestoredParts = (
    ProjectionPayloadV1,
    UnitManifestHeaderV1,
    BodyCoverageArtifact,
    std::collections::BTreeMap<String, (String, i64)>,
);

/// Restored payload DTOs without the Units (see
/// [`PgPayloadStore::restore_without_units`]), checked except for the
/// external artifact receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredSummaryV1 {
    pub projection: ProjectionPayloadV1,
    pub units: UnitManifestSummaryV1,
    pub coverage: BodyCoverageArtifact,
}

/// A generation's Unit manifest without its Units: what its receipts and the
/// lexical seal need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitManifestSummaryV1 {
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    /// The Unit manifest receipt, from every segment digest.
    pub receipt: search_source_document::ArtifactReceipt,
    pub items: usize,
    pub profile_set_digest: [u8; 32],
    /// Every Unit's ID and `unit_doc_hash`, in manifest order.
    pub units: Vec<UnitSealEntry>,
    /// Each item's stored Unit segment digest, in coverage item order.
    pub segments: Vec<String>,
}

/// A bundle whose payload digests were all recomputed. Not a READY proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPayloadV1 {
    pub key: ProjectionGenerationKey,
    pub receipt: GenerationBundleReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleError {
    /// Unknown version or field, missing or duplicate row, oversized DTO.
    Shape,
    /// Keys, Source snapshots, registry or counts disagree.
    Binding,
    /// A recomputed digest differs from the stored one.
    Digest,
    /// The database refused the write: no BUILDING parent with a live guard.
    Rejected,
    StoreUnknown,
}

impl From<sqlx::Error> for BundleError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23514" | "23503" | "23505" | "42501") => Self::Rejected,
            _ => Self::StoreUnknown,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    dto_version: String,
    body: T,
}

fn envelope<T: Serialize>(body: &T) -> Result<serde_json::Value, BundleError> {
    serde_json::to_value(Envelope {
        dto_version: PAYLOAD_DTO_VERSION.into(),
        body,
    })
    .map_err(|_| BundleError::Shape)
}

fn restore<T: DeserializeOwned>(text: &str) -> Result<T, BundleError> {
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(BundleError::Shape);
    }
    let envelope: Envelope<T> = serde_json::from_str(text).map_err(|_| BundleError::Shape)?;
    if envelope.dto_version != PAYLOAD_DTO_VERSION {
        return Err(BundleError::Shape);
    }
    Ok(envelope.body)
}

/// The stored rows of one payload: the envelope itself when it fits, else
/// `{"dto_version", "chunk"}` objects whose strings concatenate to its text.
fn payload_rows(
    value: &serde_json::Value,
    chunk_bytes: usize,
) -> Result<Vec<serde_json::Value>, BundleError> {
    let text = serde_json::to_string(value).map_err(|_| BundleError::Shape)?;
    if text.len() <= chunk_bytes {
        return Ok(vec![value.clone()]);
    }
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(BundleError::Shape);
    }
    let mut rows = Vec::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let mut end = chunk_bytes.min(rest.len());
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            // A chunk smaller than one character cannot split the text.
            return Err(BundleError::Shape);
        }
        rows.push(serde_json::json!({
            "dto_version": PAYLOAD_DTO_VERSION,
            "chunk": &rest[..end],
        }));
        rest = &rest[end..];
    }
    Ok(rows)
}

/// One payload kind restored from its rows: its envelope text, digest and
/// count. Chunks must be 0..n without gaps and agree on digest and count.
struct PayloadText {
    kind: String,
    text: String,
    digest: String,
    count: i64,
}

async fn read_payloads(
    pool: &PgPool,
    key: ProjectionGenerationKey,
) -> Result<Vec<PayloadText>, BundleError> {
    // Payload rows of tens of MiB grow the connection's buffers, which the
    // pool would otherwise keep for the connection's lifetime.
    let mut conn = pool.acquire().await?;
    let rows = sqlx::query(
        "SELECT kind, chunk, dto_version, payload::text AS payload, payload ? 'chunk' AS chunked, \
         payload ->> 'chunk' AS chunk_text, logical_digest, logical_count \
         FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2 \
         ORDER BY kind, chunk",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(&mut *conn)
    .await;
    conn.shrink_buffers();
    let rows = rows?;
    let mut out: Vec<PayloadText> = Vec::new();
    let mut chunks_of_last = 0i32;
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let chunk: i32 = row.try_get("chunk")?;
        let version: String = row.try_get("dto_version")?;
        if version != PAYLOAD_DTO_VERSION {
            return Err(BundleError::Shape);
        }
        let chunked: bool = row.try_get("chunked")?;
        let piece: String = if chunked {
            row.try_get::<Option<String>, _>("chunk_text")?
                .ok_or(BundleError::Shape)?
        } else {
            row.try_get("payload")?
        };
        let digest: String = row.try_get("logical_digest")?;
        let count: i64 = row.try_get("logical_count")?;
        let continues = out.last().is_some_and(|last| last.kind == kind);
        if continues {
            let last = out.last_mut().ok_or(BundleError::Shape)?;
            // A whole-envelope row is never continued; chunks are contiguous.
            if !chunked || chunk != chunks_of_last || last.digest != digest || last.count != count {
                return Err(BundleError::Shape);
            }
            if last.text.len() + piece.len() > MAX_PAYLOAD_BYTES {
                return Err(BundleError::Shape);
            }
            last.text.push_str(&piece);
            chunks_of_last += 1;
        } else {
            if chunk != 0 {
                return Err(BundleError::Shape);
            }
            out.push(PayloadText {
                kind,
                text: piece,
                digest,
                count,
            });
            chunks_of_last = 1;
        }
    }
    Ok(out)
}

fn sha256_text(digest: &[u8; 32]) -> String {
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// Segments read without their Units: verified once per process, then kept
/// as their coverage item, digest and Unit count.
struct SegmentSummary {
    item: BodyCoverageItem,
    digest: [u8; 32],
    /// The item's profile and parser build, when it has a profile.
    profile: Option<(String, String)>,
    units: Vec<UnitSealEntry>,
}

/// Summaries kept per process (a few hundred bytes each plus 48 bytes per
/// Unit); above this the cache keeps only the segments of the generation
/// being read.
const SUMMARY_CACHE_ITEMS: usize = 1_000_000;
/// Segments read per query by `restore_without_units`.
const SUMMARY_FETCH_BATCH: usize = 64;
/// Batches read and summarized at once: decoding and hashing every Unit
/// dominates a cold load (at 10,000 documents 26 s on one thread). The Unit
/// text of at most this many batches is held at a time.
const SUMMARY_FETCH_PARALLEL: usize = 8;

/// Whole restored summaries kept per process, newest last: the one a load
/// re-verified and then reads (each holds every Unit's seal entry).
const RESTORED_SUMMARIES: usize = 1;

type RestoredSummaries = Mutex<Vec<(ProjectionGenerationKey, String, Arc<RestoredSummaryV1>)>>;

fn restored_summaries() -> &'static RestoredSummaries {
    static CACHE: OnceLock<RestoredSummaries> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn restore_flight() -> &'static tokio::sync::Mutex<()> {
    static FLIGHT: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    FLIGHT.get_or_init(Default::default)
}

fn summary_cache() -> &'static Mutex<HashMap<String, Arc<SegmentSummary>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<SegmentSummary>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Verifies a segment read from the database as `assemble` does, applies the
/// per-item checks bound to `header`'s Source snapshot and keeps its summary.
/// Reads one batch of listed segments and summarizes each on a blocking
/// thread.
async fn summarize_batch(
    pool: PgPool,
    batch: Vec<String>,
    header: UnitManifestHeaderV1,
) -> Result<Vec<(String, SegmentSummary)>, BundleError> {
    let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT segment_digest, dto_version, payload::text, unit_count \
         FROM search_unit_segment WHERE segment_digest = ANY($1)",
    )
    .bind(&batch)
    .fetch_all(&pool)
    .await?;
    if rows.len() != batch.len() {
        return Err(BundleError::Shape);
    }
    tokio::task::spawn_blocking(move || {
        rows.into_iter()
            .map(|(digest, version, text, count)| {
                if version != PAYLOAD_DTO_VERSION {
                    return Err(BundleError::Shape);
                }
                let summary = summarize_segment(&digest, count, &text, &header)?;
                Ok((digest, summary))
            })
            .collect()
    })
    .await
    .map_err(|_| BundleError::StoreUnknown)?
}

fn summarize_segment(
    digest: &str,
    count: i64,
    text: &str,
    header: &UnitManifestHeaderV1,
) -> Result<SegmentSummary, BundleError> {
    let mut entry = verified_segment(digest, count, text)?;
    let segment = segment_digest(&entry).map_err(|_| BundleError::Digest)?;
    let profile = entry
        .profile
        .as_ref()
        .map(|profile| (profile.as_str().to_owned(), entry.parser_build_id.clone()));
    let units = entry
        .units
        .iter()
        .map(|unit| {
            Ok(UnitSealEntry {
                unit_id: unit.unit_id,
                hash: unit_doc_hash(unit).map_err(|_| BundleError::Digest)?,
            })
        })
        .collect::<Result<Vec<_>, BundleError>>()?;
    restamp_source_snapshot(&mut entry.units, &header.source_snapshot);
    let single = BodyUnitManifest {
        key: header.key,
        source_snapshot: header.source_snapshot.clone(),
        entries: vec![entry],
    };
    let coverage =
        validate_restored_manifest_skipping(&single, |_| false).map_err(|_| BundleError::Digest)?;
    let item = coverage
        .items
        .into_iter()
        .next()
        .ok_or(BundleError::Digest)?;
    Ok(SegmentSummary {
        item,
        digest: segment,
        profile,
        units,
    })
}

/// The manifest order of an item (Resource, part ordinal, path, native ID).
fn coverage_order(item: &BodyCoverageItem) -> (search_core::id::ResourceId, u32, &str, &str) {
    (
        item.version.resource_id,
        item.part.ordinal,
        item.part.logical_path.as_str(),
        item.part.source_native_part_id.as_str(),
    )
}

/// The checks shared by both restores: the projection digest, the derived
/// coverage and every stored digest column.
fn check_restored(
    manifest: &ProjectionGenerationManifest,
    projection: &ProjectionPayloadV1,
    coverage: &BodyCoverageArtifact,
    derived: &BodyCoverageArtifact,
    units_receipt: &search_source_document::ArtifactReceipt,
    columns: &std::collections::BTreeMap<String, (String, i64)>,
) -> Result<(), BundleError> {
    let key = manifest.key();
    let projection_digest =
        generation_digest(key.source_id, &projection.resources, &projection.registry)
            .map_err(|_| BundleError::Digest)?;
    if projection_digest != manifest.digest {
        return Err(BundleError::Digest);
    }
    if derived != coverage {
        return Err(BundleError::Binding);
    }
    let coverage_receipt =
        search_source_document::coverage_receipt(coverage).map_err(|_| BundleError::Digest)?;
    let expected = [
        (
            "projection",
            manifest.digest.clone(),
            manifest.resource_count,
        ),
        (
            "unit_manifest",
            sha256_text(&units_receipt.digest),
            units_receipt.count,
        ),
        (
            "body_coverage",
            sha256_text(&coverage_receipt.digest),
            coverage_receipt.count,
        ),
    ];
    for (kind, digest, count) in expected {
        match columns.get(kind) {
            Some((stored, stored_count))
                if *stored == digest && u64::try_from(*stored_count).ok() == Some(count) => {}
            _ => return Err(BundleError::Digest),
        }
    }
    Ok(())
}

/// Segment digests whose items passed the per-item checks in this process.
fn validated_segments() -> &'static Mutex<std::collections::HashSet<[u8; 32]>> {
    static VALIDATED: OnceLock<Mutex<std::collections::HashSet<[u8; 32]>>> = OnceLock::new();
    VALIDATED.get_or_init(Default::default)
}

/// Each item's segment digest and Unit count, in manifest order.
fn segment_digests(manifest: &BodyUnitManifest) -> Result<Vec<([u8; 32], u64)>, BundleError> {
    manifest
        .entries
        .iter()
        .map(|entry| {
            segment_digest(entry)
                .map(|digest| (digest, entry.units.len() as u64))
                .map_err(|_| BundleError::Digest)
        })
        .collect()
}

/// Validates the Unit manifest, skipping the per-item checks of segments this
/// process already validated (SD-T11 5), and returns its coverage artifact and
/// receipt. Order, Source binding and the derived coverage always cover every
/// item; the receipt is recomputed from every segment digest.
fn validate_units(
    manifest: &BodyUnitManifest,
    segments: &[([u8; 32], u64)],
) -> Result<
    (
        BodyCoverageArtifact,
        search_source_document::ArtifactReceipt,
    ),
    BundleError,
> {
    let known: Vec<bool> = {
        let validated = validated_segments()
            .lock()
            .map_err(|_| BundleError::StoreUnknown)?;
        segments
            .iter()
            .map(|(digest, _)| validated.contains(digest))
            .collect()
    };
    // The segment digest binds each Unit's text digest, not its text: every
    // Unit's text is checked against its digest even when the item is known.
    for (entry, known) in manifest.entries.iter().zip(&known) {
        if *known
            && entry.units.iter().any(|unit| {
                <[u8; 32]>::from(sha2::Sha256::digest(unit.text.as_bytes())) != unit.text_sha256
            })
        {
            return Err(BundleError::Digest);
        }
    }
    let coverage = validate_restored_manifest_skipping(manifest, |index| known[index])
        .map_err(|_| BundleError::Digest)?;
    let receipt = unit_manifest_receipt_from_segments(manifest.key, segments)
        .map_err(|_| BundleError::Digest)?;
    let mut validated = validated_segments()
        .lock()
        .map_err(|_| BundleError::StoreUnknown)?;
    if validated.len() + segments.len() > SEGMENT_CACHE_ITEMS {
        validated.clear();
    }
    validated.extend(segments.iter().map(|(digest, _)| *digest));
    Ok((coverage, receipt))
}

/// Recomputes the projection-only digest, the Unit manifest and coverage
/// receipts and the composite digest from the restored DTOs.
pub fn validate_stored_bundle_v1(
    bundle: &StoredBundleV1,
) -> Result<ValidatedPayloadV1, BundleError> {
    let key = bundle.manifest.key();
    let snapshot = &bundle.manifest.source_snapshot;
    if bundle.unit_manifest.key != key
        || bundle.coverage.key != key
        || bundle.receipt.key != key
        || &bundle.unit_manifest.source_snapshot != snapshot
        || &bundle.receipt.source_snapshot != snapshot
        || bundle.projection.registry.version != bundle.manifest.semantic_registry_version
        || u64::try_from(bundle.projection.resources.len()).ok()
            != Some(bundle.manifest.resource_count)
    {
        return Err(BundleError::Binding);
    }
    let projection = generation_digest(
        key.source_id,
        &bundle.projection.resources,
        &bundle.projection.registry,
    )
    .map_err(|_| BundleError::Digest)?;
    if projection != bundle.manifest.digest {
        return Err(BundleError::Digest);
    }
    let segments = segment_digests(&bundle.unit_manifest)?;
    let (derived, units) = validate_units(&bundle.unit_manifest, &segments)?;
    if derived != bundle.coverage {
        return Err(BundleError::Binding);
    }
    let receipt = compute_bundle_receipt_from(
        key,
        snapshot,
        &bundle.manifest.digest,
        units,
        bundle.unit_manifest.entries.len(),
        profile_set_digest(&bundle.unit_manifest).map_err(|_| BundleError::Digest)?,
        &bundle.coverage,
        bundle.receipt.lexical,
        bundle.receipt.graph,
    )
    .map_err(|_| BundleError::Digest)?;
    if receipt != bundle.receipt {
        return Err(BundleError::Digest);
    }
    Ok(ValidatedPayloadV1 { key, receipt })
}

/// Payload rows of `search_generation_payload`. The database admits child
/// writes only for a BUILDING FULL parent with its live exact full guard.
#[derive(Clone)]
pub struct PgPayloadStore {
    pool: PgPool,
    chunk_bytes: usize,
}

impl PgPayloadStore {
    /// The Unit manifest of `key` from its ordered segments: cached segments
    /// are reused, the others are read once, verified and cached.
    async fn assemble(
        &self,
        key: ProjectionGenerationKey,
        header: UnitManifestHeaderV1,
    ) -> Result<BodyUnitManifest, BundleError> {
        if header.key != key {
            return Err(BundleError::Binding);
        }
        let list = self.segment_list(key, &header).await?;
        let missing: Vec<String> = {
            let cache = segment_cache()
                .lock()
                .map_err(|_| BundleError::StoreUnknown)?;
            let mut wanted: Vec<String> = list
                .iter()
                .filter(|digest| !cache.contains_key(*digest))
                .cloned()
                .collect();
            wanted.sort();
            wanted.dedup();
            wanted
        };
        let mut fetched = HashMap::new();
        if !missing.is_empty() {
            let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
                "SELECT segment_digest, dto_version, payload::text, unit_count \
                 FROM search_unit_segment WHERE segment_digest = ANY($1)",
            )
            .bind(&missing)
            .fetch_all(&self.pool)
            .await?;
            if rows.len() != missing.len() {
                return Err(BundleError::Shape);
            }
            for (digest, version, text, count) in rows {
                if version != PAYLOAD_DTO_VERSION {
                    return Err(BundleError::Shape);
                }
                let entry = verified_segment(&digest, count, &text)?;
                fetched.insert(digest, Arc::new(entry));
            }
        }
        let mut cache = segment_cache()
            .lock()
            .map_err(|_| BundleError::StoreUnknown)?;
        cache.extend(fetched);
        let mut entries = Vec::with_capacity(list.len());
        for digest in &list {
            let mut entry = BodyItemEntry::clone(cache.get(digest).ok_or(BundleError::Shape)?);
            restamp_source_snapshot(&mut entry.units, &header.source_snapshot);
            entries.push(entry);
        }
        if cache.len() > SEGMENT_CACHE_ITEMS {
            let current: std::collections::BTreeSet<&String> = list.iter().collect();
            cache.retain(|digest, _| current.contains(digest));
        }
        if cache.values().map(|entry| entry.units.len()).sum::<usize>() > SEGMENT_CACHE_UNITS {
            cache.clear();
        }
        Ok(BodyUnitManifest {
            key,
            source_snapshot: header.source_snapshot,
            entries,
        })
    }

    /// The ordered segment digests of `key`, checked against its header.
    async fn segment_list(
        &self,
        key: ProjectionGenerationKey,
        header: &UnitManifestHeaderV1,
    ) -> Result<Vec<String>, BundleError> {
        let list: Vec<(i32, String)> = sqlx::query_as(
            "SELECT ordinal, segment_digest FROM search_generation_segment \
             WHERE source_id=$1 AND generation_id=$2 ORDER BY ordinal",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        if list.len() as u64 != header.segments
            || list
                .iter()
                .enumerate()
                .any(|(at, (ordinal, _))| usize::try_from(*ordinal).ok() != Some(at))
        {
            return Err(BundleError::Shape);
        }
        Ok(list.into_iter().map(|(_, digest)| digest).collect())
    }

    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            chunk_bytes: CHUNK_BYTES,
        }
    }

    /// A smaller chunk size (at most the default), e.g. to exercise chunked
    /// rows without a payload of tens of MiB.
    pub fn with_chunk_bytes(mut self, chunk_bytes: usize) -> Self {
        self.chunk_bytes = chunk_bytes.clamp(4, CHUNK_BYTES);
        self
    }

    /// Validates the bundle, then writes its three payload rows in one commit.
    pub async fn store(&self, bundle: &StoredBundleV1) -> Result<(), BundleError> {
        // Payload rows of tens of MiB grow the connection's buffers, which the
        // pool would otherwise keep for the connection's lifetime.
        let mut conn = self.pool.acquire().await?;
        let stored = self.store_on(&mut conn, bundle).await;
        conn.shrink_buffers();
        stored
    }

    async fn store_on(
        &self,
        conn: &mut PgConnection,
        bundle: &StoredBundleV1,
    ) -> Result<(), BundleError> {
        let validated = validate_stored_bundle_v1(bundle)?;
        let receipt = &validated.receipt;
        let rows = [
            (
                "projection",
                envelope(&bundle.projection)?,
                bundle.manifest.digest.clone(),
                bundle.manifest.resource_count,
            ),
            (
                "unit_manifest",
                envelope(&UnitManifestHeaderV1 {
                    key: validated.key,
                    source_snapshot: bundle.unit_manifest.source_snapshot.clone(),
                    segments: bundle.unit_manifest.entries.len() as u64,
                })?,
                sha256_text(&receipt.unit_manifest.digest),
                receipt.unit_manifest.count,
            ),
            (
                "body_coverage",
                envelope(&bundle.coverage)?,
                sha256_text(&receipt.body_coverage.digest),
                receipt.body_coverage.count,
            ),
        ];
        let digests = bundle
            .unit_manifest
            .entries
            .iter()
            .map(|entry| {
                segment_digest(entry)
                    .map(|digest| sha256_text(&digest))
                    .map_err(|_| BundleError::Digest)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut tx = conn.begin().await?;
        let present: std::collections::BTreeSet<String> = sqlx::query_scalar(
            "SELECT segment_digest FROM search_unit_segment WHERE segment_digest = ANY($1)",
        )
        .bind(&digests)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let mut written = std::collections::BTreeSet::new();
        for (entry, digest) in bundle.unit_manifest.entries.iter().zip(&digests) {
            if present.contains(digest) || !written.insert(digest.clone()) {
                continue;
            }
            sqlx::query(
                "INSERT INTO search_unit_segment \
                 (segment_digest,dto_version,payload,unit_count,verified_build) \
                 VALUES ($1,$2,$3,$4,$5) ON CONFLICT (segment_digest) DO NOTHING",
            )
            .bind(digest)
            .bind(PAYLOAD_DTO_VERSION)
            .bind(envelope(&unbound(entry))?)
            .bind(i64::try_from(entry.units.len()).map_err(|_| BundleError::Shape)?)
            .bind(SEGMENT_VERIFIER)
            .execute(&mut *tx)
            .await?;
        }
        let ordinals: Vec<i32> = (0..digests.len())
            .map(|ordinal| i32::try_from(ordinal).map_err(|_| BundleError::Shape))
            .collect::<Result<_, _>>()?;
        sqlx::query(
            "INSERT INTO search_generation_segment (source_id,generation_id,ordinal,segment_digest) \
             SELECT $1, $2, ordinal, digest FROM UNNEST($3::int[], $4::text[]) AS t(ordinal, digest)",
        )
        .bind(validated.key.source_id.as_uuid())
        .bind(validated.key.generation_id.as_uuid())
        .bind(&ordinals)
        .bind(&digests)
        .execute(&mut *tx)
        .await?;
        for (kind, payload, digest, count) in rows {
            let count = i64::try_from(count).map_err(|_| BundleError::Binding)?;
            for (chunk, part) in payload_rows(&payload, self.chunk_bytes)?
                .into_iter()
                .enumerate()
            {
                sqlx::query(
                    "INSERT INTO search_generation_payload \
                     (source_id,generation_id,kind,chunk,dto_version,payload,logical_digest, \
                      logical_count) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                )
                .bind(validated.key.source_id.as_uuid())
                .bind(validated.key.generation_id.as_uuid())
                .bind(kind)
                .bind(i32::try_from(chunk).map_err(|_| BundleError::Shape)?)
                .bind(PAYLOAD_DTO_VERSION)
                .bind(part)
                .bind(&digest)
                .bind(count)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// The projection, Unit manifest header and coverage payloads of `key`,
    /// with each kind's stored digest and count.
    async fn read_parts(&self, key: ProjectionGenerationKey) -> Result<RestoredParts, BundleError> {
        let (mut projection, mut units, mut coverage) = (None, None, None);
        let mut columns = std::collections::BTreeMap::new();
        for PayloadText {
            kind,
            text,
            digest,
            count,
        } in read_payloads(&self.pool, key).await?
        {
            let slot_taken = match kind.as_str() {
                "projection" => projection
                    .replace(restore::<ProjectionPayloadV1>(&text)?)
                    .is_some(),
                "unit_manifest" => units
                    .replace(restore::<UnitManifestHeaderV1>(&text)?)
                    .is_some(),
                "body_coverage" => coverage
                    .replace(restore::<BodyCoverageArtifact>(&text)?)
                    .is_some(),
                _ => return Err(BundleError::Shape),
            };
            if slot_taken {
                return Err(BundleError::Shape);
            }
            columns.insert(kind, (digest, count));
        }
        let (Some(projection), Some(header), Some(coverage)) = (projection, units, coverage) else {
            return Err(BundleError::Shape);
        };
        Ok((projection, header, coverage, columns))
    }

    /// Restores the payload DTOs of `manifest` and checks everything that does
    /// not depend on external artifacts: the projection-only digest, the Unit
    /// manifest structure, derived coverage and the stored digest columns.
    pub async fn restore(
        &self,
        manifest: &ProjectionGenerationManifest,
    ) -> Result<RestoredPayloadV1, BundleError> {
        let key = manifest.key();
        let (projection, header, coverage, columns) = self.read_parts(key).await?;
        let unit_manifest = self.assemble(key, header).await?;
        if unit_manifest.key != key
            || coverage.key != key
            || unit_manifest.source_snapshot != manifest.source_snapshot
            || projection.registry.version != manifest.semantic_registry_version
            || u64::try_from(projection.resources.len()).ok() != Some(manifest.resource_count)
        {
            return Err(BundleError::Binding);
        }
        let segments = segment_digests(&unit_manifest)?;
        let (derived, units_receipt) = validate_units(&unit_manifest, &segments)?;
        check_restored(
            manifest,
            &projection,
            &coverage,
            &derived,
            &units_receipt,
            &columns,
        )?;
        Ok(RestoredPayloadV1 {
            projection,
            unit_manifest,
            coverage,
        })
    }

    /// Restores and checks `manifest`'s payloads as [`Self::restore`] does,
    /// without assembling its Units: each segment is verified when this
    /// process first reads it and then kept only as its coverage item, digest
    /// and Unit count. For a reader that needs no Unit text (T12).
    pub async fn restore_without_units(
        &self,
        manifest: &ProjectionGenerationManifest,
    ) -> Result<RestoredSummaryV1, BundleError> {
        let key = manifest.key();
        // The same generation restored and checked by this process before.
        let cached = || -> Result<Option<RestoredSummaryV1>, BundleError> {
            Ok(restored_summaries()
                .lock()
                .map_err(|_| BundleError::StoreUnknown)?
                .iter()
                .find(|(at, digest, _)| *at == key && *digest == manifest.digest)
                .map(|(_, _, restored)| (**restored).clone()))
        };
        if let Some(restored) = cached()? {
            return Ok(restored);
        }
        // One restore at a time: a concurrent reader of the same generation
        // waits for it and takes its result instead of restoring it again.
        let _flight = restore_flight().lock().await;
        if let Some(restored) = cached()? {
            return Ok(restored);
        }
        let restored = self.restore_summary_uncached(manifest).await?;
        let mut cache = restored_summaries()
            .lock()
            .map_err(|_| BundleError::StoreUnknown)?;
        cache.push((key, manifest.digest.clone(), Arc::new(restored.clone())));
        if cache.len() > RESTORED_SUMMARIES {
            cache.remove(0);
        }
        Ok(restored)
    }

    async fn restore_summary_uncached(
        &self,
        manifest: &ProjectionGenerationManifest,
    ) -> Result<RestoredSummaryV1, BundleError> {
        let key = manifest.key();
        let (projection, header, coverage, columns) = self.read_parts(key).await?;
        if header.key != key
            || coverage.key != key
            || header.source_snapshot != manifest.source_snapshot
            || projection.registry.version != manifest.semantic_registry_version
            || u64::try_from(projection.resources.len()).ok() != Some(manifest.resource_count)
        {
            return Err(BundleError::Binding);
        }
        let list = self.segment_list(key, &header).await?;
        let missing: Vec<String> = {
            let cache = summary_cache()
                .lock()
                .map_err(|_| BundleError::StoreUnknown)?;
            let mut wanted: Vec<String> = list
                .iter()
                .filter(|digest| !cache.contains_key(*digest))
                .cloned()
                .collect();
            wanted.sort();
            wanted.dedup();
            wanted
        };
        let mut fetched = HashMap::with_capacity(missing.len());
        let mut batches = missing.chunks(SUMMARY_FETCH_BATCH).map(<[String]>::to_vec);
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            while tasks.len() < SUMMARY_FETCH_PARALLEL {
                let Some(batch) = batches.next() else {
                    break;
                };
                tasks.spawn(summarize_batch(self.pool.clone(), batch, header.clone()));
            }
            // An error drops the set, which aborts the other batches.
            let Some(done) = tasks.join_next().await else {
                break;
            };
            for (digest, summary) in done.map_err(|_| BundleError::StoreUnknown)?? {
                fetched.insert(digest, Arc::new(summary));
            }
        }
        let mut cache = summary_cache()
            .lock()
            .map_err(|_| BundleError::StoreUnknown)?;
        cache.extend(fetched);
        let mut items = Vec::with_capacity(list.len());
        let mut segments = Vec::with_capacity(list.len());
        let mut units = Vec::new();
        let mut profiles = Vec::new();
        for digest in &list {
            let summary = cache.get(digest).ok_or(BundleError::Shape)?;
            if summary.item.version.source_id != key.source_id {
                return Err(BundleError::Digest);
            }
            items.push(summary.item.clone());
            segments.push((summary.digest, u64::from(summary.item.unit_count)));
            units.extend_from_slice(&summary.units);
            profiles.extend(summary.profile.clone());
        }
        if cache.len() > SUMMARY_CACHE_ITEMS {
            let current: std::collections::BTreeSet<&String> = list.iter().collect();
            cache.retain(|digest, _| current.contains(digest));
        }
        drop(cache);
        if items
            .windows(2)
            .any(|pair| coverage_order(&pair[0]) >= coverage_order(&pair[1]))
        {
            return Err(BundleError::Digest);
        }
        let items_len = items.len();
        let derived = BodyCoverageArtifact { key, items };
        let units_receipt =
            unit_manifest_receipt_from_segments(key, &segments).map_err(|_| BundleError::Digest)?;
        let profile_set_digest = search_source_document::profile_set_digest_from(
            profiles
                .iter()
                .map(|(profile, build)| (profile.as_str(), build.as_str())),
        )
        .map_err(|_| BundleError::Digest)?;
        check_restored(
            manifest,
            &projection,
            &coverage,
            &derived,
            &units_receipt,
            &columns,
        )?;
        Ok(RestoredSummaryV1 {
            projection,
            units: UnitManifestSummaryV1 {
                key,
                source_snapshot: header.source_snapshot,
                receipt: units_receipt,
                items: items_len,
                profile_set_digest,
                units,
                segments: list,
            },
            coverage,
        })
    }

    /// The listed Unit segments, each verified as a restore verifies it, with
    /// every Unit's Source snapshot cleared. Not kept in the process cache.
    pub async fn unit_segments(
        &self,
        digests: &[String],
    ) -> Result<HashMap<String, BodyItemEntry>, BundleError> {
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT segment_digest, dto_version, payload::text, unit_count \
             FROM search_unit_segment WHERE segment_digest = ANY($1)",
        )
        .bind(digests)
        .fetch_all(&self.pool)
        .await?;
        let mut out = HashMap::with_capacity(rows.len());
        for (digest, version, text, count) in rows {
            if version != PAYLOAD_DTO_VERSION {
                return Err(BundleError::Shape);
            }
            let entry = verified_segment(&digest, count, &text)?;
            out.insert(digest, entry);
        }
        if digests.iter().any(|digest| !out.contains_key(digest)) {
            return Err(BundleError::Shape);
        }
        Ok(out)
    }

    /// Restores the payload rows of `manifest` and revalidates them against the
    /// bundle receipt supplied by its owner. Stored digest columns must equal
    /// the recomputed values.
    pub async fn load(
        &self,
        manifest: &ProjectionGenerationManifest,
        receipt: &GenerationBundleReceipt,
    ) -> Result<StoredBundleV1, BundleError> {
        let key = manifest.key();
        let (mut projection, mut units, mut coverage) = (None, None, None);
        let mut columns = std::collections::BTreeMap::new();
        for PayloadText {
            kind,
            text,
            digest,
            count,
        } in read_payloads(&self.pool, key).await?
        {
            let slot_taken = match kind.as_str() {
                "projection" => projection
                    .replace(restore::<ProjectionPayloadV1>(&text)?)
                    .is_some(),
                "unit_manifest" => units
                    .replace(restore::<UnitManifestHeaderV1>(&text)?)
                    .is_some(),
                "body_coverage" => coverage
                    .replace(restore::<BodyCoverageArtifact>(&text)?)
                    .is_some(),
                _ => return Err(BundleError::Shape),
            };
            if slot_taken {
                return Err(BundleError::Shape);
            }
            columns.insert(kind, (digest, count));
        }
        let (Some(projection), Some(header), Some(coverage)) = (projection, units, coverage) else {
            return Err(BundleError::Shape);
        };
        let unit_manifest = self.assemble(key, header).await?;
        let bundle = StoredBundleV1 {
            manifest: manifest.clone(),
            projection,
            unit_manifest,
            coverage,
            receipt: receipt.clone(),
        };
        let validated = validate_stored_bundle_v1(&bundle)?;
        let expected = [
            (
                "projection",
                manifest.digest.clone(),
                manifest.resource_count,
            ),
            (
                "unit_manifest",
                sha256_text(&validated.receipt.unit_manifest.digest),
                validated.receipt.unit_manifest.count,
            ),
            (
                "body_coverage",
                sha256_text(&validated.receipt.body_coverage.digest),
                validated.receipt.body_coverage.count,
            ),
        ];
        for (kind, digest, count) in expected {
            match columns.get(kind) {
                Some((stored, stored_count))
                    if *stored == digest && u64::try_from(*stored_count).ok() == Some(count) => {}
                _ => return Err(BundleError::Digest),
            }
        }
        Ok(bundle)
    }
}
