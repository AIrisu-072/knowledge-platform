//! E (P2-07): the PostgreSQL Vector index and generation ports.
//!
//! `PgVectorIndex` stores each vector value once per model and cache key, and
//! a stage as an ordered list of segments, one per Unit segment, that hold
//! the entry references (stage 3). Value bytes and segment entries are
//! checked against their stored digests on every read; a mismatch makes the
//! stage unavailable, never a silent result. Search is an exact cosine scan of the
//! pinned stage with the configured similarity floor: entries below it are
//! never candidates. `PgVectorGenerations` publishes a manifest only while
//! its P7 bundle key is still the Source's current key and its authority
//! scope is still at the epoch the build read, in one transaction.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::knowledge_unit::{EmbeddingCacheKey, VectorHitRef};
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelId, QueryEmbedding, RankedVectorHit, VectorEntryRef,
    VectorIndexDescriptor, VectorProjectionManifest,
};
use search_application::vector::{PinnedVectorGeneration, VectorGenerationPort, VectorIndexPort};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::sync::watch;
use uuid::Uuid;

pub const VECTOR_ENGINE: &str = "exact-cosine";
pub const VECTOR_ENGINE_BUILD: &str = "search-runtime-vector-v1";

fn unavailable(what: &str) -> SearchError {
    SearchError::SourceUnavailable(format!("vector store: {what}"))
}

fn sql(error: sqlx::Error) -> SearchError {
    unavailable(&error.to_string())
}

fn sha256_text(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn hex32(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex32(value: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    if value.len() != 64 {
        return None;
    }
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn vector_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryDto {
    hit: VectorHitRef,
    model_id: EmbeddingModelId,
    vector_digest: String,
    cache_key: EmbeddingCacheKey,
}

impl EntryDto {
    fn of(entry: &VectorEntryRef) -> Self {
        Self {
            hit: entry.hit.clone(),
            model_id: entry.model_id.clone(),
            vector_digest: hex32(&entry.vector_digest),
            cache_key: entry.cache_key.clone(),
        }
    }

    fn into_entry(self) -> Result<VectorEntryRef, SearchError> {
        Ok(VectorEntryRef {
            hit: self.hit,
            model_id: self.model_id,
            vector_digest: from_hex32(&self.vector_digest)
                .ok_or_else(|| unavailable("entry digest"))?,
            cache_key: self.cache_key,
        })
    }
}

/// One entry of a Vector segment: the entry reference without its
/// generation, and the key of its stored value.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentEntryDto {
    entry: EntryDto,
    cache_digest: String,
}

/// A segment as read from the database and checked against its digest.
struct StoredSegment {
    model_id: String,
    entries: Vec<SegmentEntryDto>,
}

/// The digest a vector value is stored under: one per model and cache key.
pub fn cache_digest(key: &EmbeddingCacheKey) -> Result<String, SearchError> {
    let mut bytes = b"search-vector-cache-key:v1\n".to_vec();
    serde_json::to_writer(&mut bytes, key).map_err(|_| unavailable("cache key"))?;
    Ok(sha256_text(&bytes))
}

/// The Vector segment of one Unit segment under one model and authority:
/// its entries follow from these alone, so an equal digest is reused.
pub fn segment_digest_for(
    model: &EmbeddingModelId,
    unit_segment: &str,
    authority_scope_key: &str,
    retention_lease_id: &str,
    lifetime_scope_id: &str,
) -> String {
    let mut bytes = b"search-vector-segment:v1".to_vec();
    for part in [
        model.as_str(),
        unit_segment,
        authority_scope_key,
        retention_lease_id,
        lifetime_scope_id,
    ] {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    sha256_text(&bytes)
}

/// Stored entries name no generation; the reader stamps the stage's own.
fn unbound(hit: &VectorHitRef) -> VectorHitRef {
    let mut hit = hit.clone();
    hit.generation.generation_id = ProjectionGenerationId::from_uuid(Uuid::nil());
    hit
}

fn entries_digest(entries: &[SegmentEntryDto]) -> Result<String, SearchError> {
    Ok(sha256_text(
        &serde_json::to_vec(entries).map_err(|_| unavailable("segment entries"))?,
    ))
}

fn values_of(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

/// Rows fetched per query.
const FETCH_BATCH: usize = 4_096;
/// Segments fetched per query.
const SEGMENT_FETCH_BATCH: usize = 256;

/// One loaded segment for scans: hits without generation, vectors in order.
struct LoadedSegment {
    hits: Vec<VectorHitRef>,
    vectors: Vec<f32>,
}

/// One stage loaded for scans.
struct LoadedStage {
    model_id: EmbeddingModelId,
    bundle: ProjectionGenerationKey,
    dimension: usize,
    segments: Vec<Arc<LoadedSegment>>,
}

type LoadOutcome = watch::Receiver<Option<Result<Arc<LoadedStage>, String>>>;

/// The exact-scan index over PostgreSQL rows.
pub struct PgVectorIndex {
    pool: PgPool,
    floor: f32,
    loaded: Arc<Mutex<BTreeMap<String, Arc<LoadedStage>>>>,
    /// Scan segments of the loaded stage, shared by the next one.
    scan_segments: Arc<Mutex<HashMap<String, Arc<LoadedSegment>>>>,
    /// Verified segment entries of the last stage this process wrote or read.
    stored_segments: Arc<Mutex<HashMap<String, Arc<StoredSegment>>>>,
    /// The stage being loaded and the outcome of its load task. The load
    /// runs detached, so a request that gives up at its deadline does not
    /// cancel it; later requests wait for the same outcome.
    loading: Arc<Mutex<Option<(String, LoadOutcome)>>>,
}

impl PgVectorIndex {
    /// `floor`: the minimum cosine similarity of a candidate.
    pub fn new(pool: PgPool, floor: f32) -> Self {
        Self {
            pool,
            floor,
            loaded: Default::default(),
            scan_segments: Default::default(),
            stored_segments: Default::default(),
            loading: Default::default(),
        }
    }

    /// The same index and caches, for a detached load task.
    fn detached(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            floor: self.floor,
            loaded: self.loaded.clone(),
            scan_segments: self.scan_segments.clone(),
            stored_segments: self.stored_segments.clone(),
            loading: self.loading.clone(),
        }
    }

    /// Reads segments and checks each against its entry digest and count.
    async fn fetch_segments(
        &self,
        digests: &[String],
    ) -> Result<HashMap<String, StoredSegment>, SearchError> {
        let mut out = HashMap::with_capacity(digests.len());
        for batch in digests.chunks(SEGMENT_FETCH_BATCH) {
            let rows = sqlx::query(
                "SELECT segment_digest, model_id, entry_count, entries, entries_sha256 \
                 FROM search_vector_segment WHERE segment_digest = ANY($1)",
            )
            .bind(batch)
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            for row in rows {
                let entries: Vec<SegmentEntryDto> =
                    serde_json::from_value(row.try_get("entries").map_err(sql)?)
                        .map_err(|_| unavailable("segment decode"))?;
                let count: i32 = row.try_get("entry_count").map_err(sql)?;
                if usize::try_from(count).ok() != Some(entries.len())
                    || entries_digest(&entries)?
                        != row.try_get::<String, _>("entries_sha256").map_err(sql)?
                {
                    return Err(unavailable("segment changed"));
                }
                out.insert(
                    row.try_get("segment_digest").map_err(sql)?,
                    StoredSegment {
                        model_id: row.try_get("model_id").map_err(sql)?,
                        entries,
                    },
                );
            }
        }
        if digests.iter().any(|digest| !out.contains_key(digest)) {
            return Err(unavailable("missing segment"));
        }
        Ok(out)
    }

    /// The entries of `digests` in order, stamped with `bundle`. The cache
    /// keeps only these segments afterwards.
    async fn segment_entries(
        &self,
        bundle: ProjectionGenerationKey,
        model: &EmbeddingModelId,
        digests: &[String],
    ) -> Result<Vec<VectorEntryRef>, SearchError> {
        let missing: Vec<String> = {
            let cache = self
                .stored_segments
                .lock()
                .map_err(|_| unavailable("lock"))?;
            let mut seen = HashSet::new();
            digests
                .iter()
                .filter(|digest| !cache.contains_key(*digest) && seen.insert(*digest))
                .cloned()
                .collect()
        };
        let fetched = self.fetch_segments(&missing).await?;
        let segments: Vec<Arc<StoredSegment>> = {
            let mut cache = self
                .stored_segments
                .lock()
                .map_err(|_| unavailable("lock"))?;
            cache.extend(
                fetched
                    .into_iter()
                    .map(|(digest, segment)| (digest, Arc::new(segment))),
            );
            let listed: HashSet<&String> = digests.iter().collect();
            cache.retain(|digest, _| listed.contains(digest));
            digests
                .iter()
                .map(|digest| {
                    cache
                        .get(digest)
                        .cloned()
                        .ok_or_else(|| unavailable("segment"))
                })
                .collect::<Result<_, _>>()?
        };
        let mut entries = Vec::with_capacity(segments.iter().map(|s| s.entries.len()).sum());
        for segment in segments {
            if segment.model_id != model.as_str() {
                return Err(unavailable("segment model"));
            }
            for stored in &segment.entries {
                let mut entry = stored.entry.clone().into_entry()?;
                if entry.hit.generation.source_id != bundle.source_id {
                    return Err(unavailable("segment Source"));
                }
                entry.hit.generation = bundle;
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Stored vectors by cache digest; a value whose bytes changed is absent.
    pub async fn cached_values(
        &self,
        model: &EmbeddingModelId,
        digests: &[String],
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        let mut out = HashMap::with_capacity(digests.len());
        for batch in digests.chunks(FETCH_BATCH) {
            let rows = sqlx::query(
                "SELECT cache_digest, vector, vector_sha256 FROM search_vector_value \
                 WHERE model_id=$1 AND cache_digest = ANY($2)",
            )
            .bind(model.as_str())
            .bind(batch)
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            for row in rows {
                let bytes: Vec<u8> = row.try_get("vector").map_err(sql)?;
                if sha256_text(&bytes) != row.try_get::<String, _>("vector_sha256").map_err(sql)? {
                    continue;
                }
                out.insert(row.try_get("cache_digest").map_err(sql)?, values_of(&bytes));
            }
        }
        Ok(out)
    }

    /// The listed segments that are stored already.
    pub async fn existing_segments(
        &self,
        digests: &[String],
    ) -> Result<HashSet<String>, SearchError> {
        let mut out = HashSet::new();
        for batch in digests.chunks(FETCH_BATCH) {
            let rows: Vec<String> = sqlx::query_scalar(
                "SELECT segment_digest FROM search_vector_segment WHERE segment_digest = ANY($1)",
            )
            .bind(batch)
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            out.extend(rows);
        }
        Ok(out)
    }

    /// Writes the values of `embeddings` and one segment of their entries.
    /// A segment or value that exists already is kept as it is.
    pub async fn put_segment(
        &self,
        segment_digest: &str,
        model: &EmbeddingModelId,
        authority_scope_key: &str,
        embeddings: &[BoundEmbedding],
    ) -> Result<(), SearchError> {
        let mut digests = Vec::with_capacity(embeddings.len());
        let mut vectors = Vec::with_capacity(embeddings.len());
        let mut sums = Vec::with_capacity(embeddings.len());
        let mut entries = Vec::with_capacity(embeddings.len());
        for embedding in embeddings {
            let entry = embedding.entry_ref();
            if entry.cache_key.authority_scope_key != authority_scope_key
                || entry.model_id != *model
            {
                return Err(unavailable("segment binding"));
            }
            let digest = cache_digest(&entry.cache_key)?;
            let bytes = vector_bytes(embedding.values());
            sums.push(sha256_text(&bytes));
            vectors.push(bytes);
            digests.push(digest.clone());
            let mut dto = EntryDto::of(&entry);
            dto.hit = unbound(&dto.hit);
            entries.push(SegmentEntryDto {
                entry: dto,
                cache_digest: digest,
            });
        }
        let mut tx = self.pool.begin().await.map_err(sql)?;
        for start in (0..digests.len()).step_by(FETCH_BATCH) {
            let end = (start + FETCH_BATCH).min(digests.len());
            sqlx::query(
                "INSERT INTO search_vector_value (model_id,cache_digest,authority_scope_key, \
                 vector,vector_sha256) SELECT $1, d, $2, v, s \
                 FROM UNNEST($3::text[], $4::bytea[], $5::text[]) AS t(d, v, s) \
                 ON CONFLICT (model_id, cache_digest) DO NOTHING",
            )
            .bind(model.as_str())
            .bind(authority_scope_key)
            .bind(&digests[start..end])
            .bind(&vectors[start..end])
            .bind(&sums[start..end])
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
        }
        sqlx::query(
            "INSERT INTO search_vector_segment (segment_digest,model_id,authority_scope_key, \
             entry_count,entries,entries_sha256) VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT (segment_digest) DO NOTHING",
        )
        .bind(segment_digest)
        .bind(model.as_str())
        .bind(authority_scope_key)
        .bind(i32::try_from(entries.len()).map_err(|_| unavailable("segment size"))?)
        .bind(serde_json::to_value(&entries).map_err(|_| unavailable("segment entries"))?)
        .bind(entries_digest(&entries)?)
        .execute(&mut *tx)
        .await
        .map_err(sql)?;
        tx.commit().await.map_err(sql)?;
        Ok(())
    }

    /// Writes an unpublished stage that lists `segments` in order, and
    /// returns its descriptor with the entries it holds.
    pub async fn stage_segments(
        &self,
        bundle: ProjectionGenerationKey,
        model: &EmbeddingModelId,
        segments: &[String],
    ) -> Result<(VectorIndexDescriptor, Vec<VectorEntryRef>), SearchError> {
        let entries = self.segment_entries(bundle, model, segments).await?;
        let receipt: Vec<u8> = entries
            .iter()
            .flat_map(|entry| entry.vector_digest.to_vec())
            .collect();
        let nonce = Uuid::now_v7();
        let descriptor = VectorIndexDescriptor {
            engine: VECTOR_ENGINE.into(),
            engine_build: VECTOR_ENGINE_BUILD.into(),
            parameters_digest: sha256_text(
                format!("metric=cosine;floor={}", self.floor).as_bytes(),
            ),
            index_receipt_digest: sha256_text(&receipt),
            index_digest: sha256_text(
                format!(
                    "{}:{}:{}:{}",
                    bundle.source_id.as_uuid(),
                    bundle.generation_id.as_uuid(),
                    model,
                    nonce
                )
                .as_bytes(),
            ),
        };
        let mut tx = self.pool.begin().await.map_err(sql)?;
        sqlx::query(
            "INSERT INTO search_vector_stage (index_digest,source_id,generation_id,model_id, \
             descriptor,entry_count,created_at) VALUES ($1,$2,$3,$4,$5,$6,clock_timestamp())",
        )
        .bind(&descriptor.index_digest)
        .bind(bundle.source_id.as_uuid())
        .bind(bundle.generation_id.as_uuid())
        .bind(model.as_str())
        .bind(serde_json::to_value(&descriptor).map_err(|_| unavailable("descriptor"))?)
        .bind(i32::try_from(entries.len()).map_err(|_| unavailable("too many entries"))?)
        .execute(&mut *tx)
        .await
        .map_err(sql)?;
        let ordinals: Vec<i32> = (0..segments.len())
            .map(|ordinal| i32::try_from(ordinal).map_err(|_| unavailable("ordinal")))
            .collect::<Result<_, _>>()?;
        for start in (0..segments.len()).step_by(FETCH_BATCH) {
            let end = (start + FETCH_BATCH).min(segments.len());
            sqlx::query(
                "INSERT INTO search_vector_stage_segment (index_digest,ordinal,segment_digest) \
                 SELECT $1, o, d FROM UNNEST($2::int4[], $3::text[]) AS t(o, d)",
            )
            .bind(&descriptor.index_digest)
            .bind(&ordinals[start..end])
            .bind(&segments[start..end])
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
        }
        tx.commit().await.map_err(sql)?;
        Ok((descriptor, entries))
    }

    /// The stage's bundle key, model, entry count and ordered segment list.
    async fn stage_list(
        &self,
        index_digest: &str,
    ) -> Result<
        (
            ProjectionGenerationKey,
            EmbeddingModelId,
            usize,
            Vec<String>,
        ),
        SearchError,
    > {
        let stage = sqlx::query(
            "SELECT source_id, generation_id, model_id, entry_count FROM search_vector_stage \
             WHERE index_digest=$1",
        )
        .bind(index_digest)
        .fetch_optional(&self.pool)
        .await
        .map_err(sql)?
        .ok_or_else(|| unavailable("missing stage"))?;
        let bundle = ProjectionGenerationKey {
            source_id: SourceId::from_uuid(stage.try_get("source_id").map_err(sql)?),
            generation_id: ProjectionGenerationId::from_uuid(
                stage.try_get("generation_id").map_err(sql)?,
            ),
        };
        let model_id =
            EmbeddingModelId::parse(&stage.try_get::<String, _>("model_id").map_err(sql)?)
                .map_err(|_| unavailable("stage model"))?;
        let count = usize::try_from(stage.try_get::<i32, _>("entry_count").map_err(sql)?)
            .map_err(|_| unavailable("stage entry count"))?;
        let rows: Vec<(i32, String)> = sqlx::query_as(
            "SELECT ordinal, segment_digest FROM search_vector_stage_segment \
             WHERE index_digest=$1 ORDER BY ordinal",
        )
        .bind(index_digest)
        .fetch_all(&self.pool)
        .await
        .map_err(sql)?;
        if rows
            .iter()
            .enumerate()
            .any(|(at, (ordinal, _))| usize::try_from(*ordinal).ok() != Some(at))
        {
            return Err(unavailable("stage segment list"));
        }
        Ok((
            bundle,
            model_id,
            count,
            rows.into_iter().map(|(_, digest)| digest).collect(),
        ))
    }

    /// The loaded stage of `index_digest`; a stage not loaded yet is loaded
    /// by one detached task that every waiting request shares.
    async fn load(&self, index_digest: &str) -> Result<Arc<LoadedStage>, SearchError> {
        if let Some(stage) = self
            .loaded
            .lock()
            .map_err(|_| unavailable("lock"))?
            .get(index_digest)
        {
            return Ok(stage.clone());
        }
        let mut outcome = {
            let mut loading = self.loading.lock().map_err(|_| unavailable("lock"))?;
            match loading.as_ref() {
                Some((digest, outcome)) if digest == index_digest => outcome.clone(),
                _ => {
                    let (sender, outcome) = watch::channel(None);
                    let task = self.detached();
                    let digest = index_digest.to_owned();
                    tokio::spawn(async move {
                        let result = task.load_now(&digest).await.map_err(|error| error.to_string());
                        if let Ok(mut loading) = task.loading.lock()
                            && loading.as_ref().is_some_and(|(at, _)| *at == digest)
                        {
                            *loading = None;
                        }
                        let _ = sender.send(Some(result));
                    });
                    *loading = Some((index_digest.to_owned(), outcome.clone()));
                    outcome
                }
            }
        };
        let result = outcome
            .wait_for(Option::is_some)
            .await
            .map_err(|_| unavailable("stage load stopped"))?
            .clone();
        match result {
            Some(Ok(stage)) => Ok(stage),
            Some(Err(error)) => Err(SearchError::SourceUnavailable(error)),
            None => Err(unavailable("stage load")),
        }
    }

    async fn load_now(&self, index_digest: &str) -> Result<Arc<LoadedStage>, SearchError> {
        let (bundle, model_id, count, digests) = self.stage_list(index_digest).await?;
        let cached: HashMap<String, Arc<LoadedSegment>> = {
            let cache = self.scan_segments.lock().map_err(|_| unavailable("lock"))?;
            digests
                .iter()
                .filter_map(|digest| Some((digest.clone(), cache.get(digest)?.clone())))
                .collect()
        };
        let mut fresh: HashMap<String, Arc<LoadedSegment>> = HashMap::new();
        let missing: Vec<String> = {
            let mut seen = HashSet::new();
            digests
                .iter()
                .filter(|digest| !cached.contains_key(*digest) && seen.insert(*digest))
                .cloned()
                .collect()
        };
        for batch in missing.chunks(SEGMENT_FETCH_BATCH) {
            let stored = self.fetch_segments(batch).await?;
            let keys: Vec<String> = stored
                .values()
                .flat_map(|segment| segment.entries.iter().map(|e| e.cache_digest.clone()))
                .collect();
            let values = self.cached_values(&model_id, &keys).await?;
            for (digest, segment) in stored {
                if segment.model_id != model_id.as_str() {
                    return Err(unavailable("segment model"));
                }
                let mut hits = Vec::with_capacity(segment.entries.len());
                let mut vectors = Vec::new();
                for stored in segment.entries {
                    let value = values
                        .get(&stored.cache_digest)
                        .ok_or_else(|| unavailable("missing vector value"))?;
                    vectors.extend_from_slice(value);
                    hits.push(stored.entry.hit);
                }
                fresh.insert(digest, Arc::new(LoadedSegment { hits, vectors }));
            }
        }
        let segments: Vec<Arc<LoadedSegment>> = digests
            .iter()
            .map(|digest| {
                cached
                    .get(digest)
                    .or_else(|| fresh.get(digest))
                    .cloned()
                    .ok_or_else(|| unavailable("segment"))
            })
            .collect::<Result<_, _>>()?;
        let total: usize = segments.iter().map(|segment| segment.hits.len()).sum();
        let values: usize = segments.iter().map(|segment| segment.vectors.len()).sum();
        if total != count {
            return Err(unavailable("stage entry count"));
        }
        let dimension = values.checked_div(total).unwrap_or(0);
        if segments
            .iter()
            .any(|segment| segment.vectors.len() != segment.hits.len() * dimension)
        {
            return Err(unavailable("vector dimension"));
        }
        let stage = Arc::new(LoadedStage {
            model_id,
            bundle,
            dimension,
            segments,
        });
        // Only the newest stage stays loaded; a request that pinned an older
        // one keeps its own reference.
        if let Ok(mut cache) = self.scan_segments.lock() {
            *cache = digests
                .iter()
                .zip(&stage.segments)
                .map(|(digest, segment)| (digest.clone(), segment.clone()))
                .collect();
        }
        let mut loaded = self.loaded.lock().map_err(|_| unavailable("lock"))?;
        loaded.clear();
        loaded.insert(index_digest.to_owned(), stage.clone());
        Ok(stage)
    }

    fn forget(&self, index_digest: &str) {
        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.remove(index_digest);
        }
    }

    /// Deletes segments no stage lists. Younger ones are kept: a build may
    /// have written them and not yet listed them in its stage.
    async fn collect_segments(&self) -> Result<(), SearchError> {
        sqlx::query(
            "DELETE FROM search_vector_segment s \
             WHERE s.created_at < clock_timestamp() - interval '10 minutes' \
             AND NOT EXISTS (SELECT 1 FROM search_vector_stage_segment ss \
             WHERE ss.segment_digest = s.segment_digest)",
        )
        .execute(&self.pool)
        .await
        .map_err(sql)?;
        Ok(())
    }
}

/// Consecutive entries of one Version × Part form one segment.
fn adhoc_segments(embeddings: &[BoundEmbedding]) -> Vec<&[BoundEmbedding]> {
    let mut out = Vec::new();
    let mut start = 0;
    for index in 1..=embeddings.len() {
        let split = index == embeddings.len() || {
            let (a, b) = (
                embeddings[index - 1].entry_ref(),
                embeddings[index].entry_ref(),
            );
            a.hit.version != b.hit.version || a.hit.part != b.hit.part
        };
        if split {
            out.push(&embeddings[start..index]);
            start = index;
        }
    }
    out
}

impl VectorIndexPort for PgVectorIndex {
    fn stage<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
        embeddings: &'a [BoundEmbedding],
    ) -> BoxFuture<'a, VectorIndexDescriptor> {
        Box::pin(async move {
            let mut digests = Vec::new();
            for segment in adhoc_segments(embeddings) {
                let entries: Vec<EntryDto> = segment
                    .iter()
                    .map(|embedding| {
                        let mut dto = EntryDto::of(&embedding.entry_ref());
                        dto.hit = unbound(&dto.hit);
                        dto
                    })
                    .collect();
                let mut bytes = b"search-vector-adhoc-segment:v1\n".to_vec();
                serde_json::to_writer(&mut bytes, &entries).map_err(|_| unavailable("segment"))?;
                let digest = sha256_text(&bytes);
                let scope = segment[0].entry_ref().cache_key.authority_scope_key;
                self.put_segment(&digest, model, &scope, segment).await?;
                digests.push(digest);
            }
            Ok(self.stage_segments(bundle, model, &digests).await?.0)
        })
    }

    fn staged_entries<'a>(
        &'a self,
        index: &'a VectorIndexDescriptor,
    ) -> BoxFuture<'a, Vec<VectorEntryRef>> {
        Box::pin(async move {
            let (bundle, model, count, digests) = self.stage_list(&index.index_digest).await?;
            let entries = self.segment_entries(bundle, &model, &digests).await?;
            if entries.len() != count {
                return Err(unavailable("stage entry count"));
            }
            Ok(entries)
        })
    }

    fn stages<'a>(&'a self) -> BoxFuture<'a, Vec<VectorIndexDescriptor>> {
        Box::pin(async move {
            let rows: Vec<serde_json::Value> = sqlx::query_scalar(
                "SELECT descriptor FROM search_vector_stage ORDER BY index_digest",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            rows.into_iter()
                .map(|value| {
                    serde_json::from_value(value).map_err(|_| unavailable("descriptor decode"))
                })
                .collect()
        })
    }

    fn search<'a>(
        &'a self,
        pin: &'a PinnedVectorGeneration,
        query: &'a QueryEmbedding,
        window: usize,
    ) -> BoxFuture<'a, Vec<RankedVectorHit>> {
        Box::pin(async move {
            let stage = self.load(&pin.manifest().index.index_digest).await?;
            if stage.model_id != pin.manifest().model_id
                || query.model_id() != &stage.model_id
                || stage.bundle != pin.manifest().bundle_key
            {
                return Err(unavailable("model binding"));
            }
            let values = query.values();
            if stage.dimension != 0 && values.len() != stage.dimension {
                return Err(unavailable("query dimension"));
            }
            let mut scored: Vec<(f32, usize, usize)> = Vec::new();
            for (at, segment) in stage.segments.iter().enumerate() {
                for (row, vector) in segment
                    .vectors
                    .chunks_exact(stage.dimension.max(1))
                    .enumerate()
                {
                    let similarity = vector.iter().zip(values).map(|(a, b)| a * b).sum::<f32>();
                    if similarity >= self.floor {
                        scored.push((similarity, at, row));
                    }
                }
            }
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then((a.1, a.2).cmp(&(b.1, b.2))));
            Ok(scored
                .into_iter()
                .take(window)
                .enumerate()
                .map(|(rank, (similarity, at, row))| {
                    let mut hit = stage.segments[at].hits[row].clone();
                    hit.generation = stage.bundle;
                    RankedVectorHit {
                        hit,
                        model_id: stage.model_id.clone(),
                        rank: rank + 1,
                        raw_similarity: similarity,
                    }
                })
                .collect())
        })
    }

    fn discard<'a>(&'a self, index: &'a VectorIndexDescriptor) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.forget(&index.index_digest);
            // A published stage stays until its manifest is withdrawn.
            sqlx::query(
                "DELETE FROM search_vector_stage s WHERE s.index_digest=$1 AND NOT EXISTS \
                 (SELECT 1 FROM search_vector_generation g WHERE g.index_digest=s.index_digest)",
            )
            .bind(&index.index_digest)
            .execute(&self.pool)
            .await
            .map_err(sql)?;
            self.collect_segments().await
        })
    }

    fn purge_scope<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql)?;
            sqlx::query(
                "DELETE FROM search_vector_stage_segment ss USING search_vector_segment s \
                 WHERE ss.segment_digest = s.segment_digest AND s.authority_scope_key=$1",
            )
            .bind(authority_scope_key)
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
            sqlx::query("DELETE FROM search_vector_segment WHERE authority_scope_key=$1")
                .bind(authority_scope_key)
                .execute(&mut *tx)
                .await
                .map_err(sql)?;
            let purged =
                sqlx::query("DELETE FROM search_vector_value WHERE authority_scope_key=$1")
                    .bind(authority_scope_key)
                    .execute(&mut *tx)
                    .await
                    .map_err(sql)?
                    .rows_affected();
            tx.commit().await.map_err(sql)?;
            if let Ok(mut loaded) = self.loaded.lock() {
                loaded.clear();
            }
            if let Ok(mut cache) = self.scan_segments.lock() {
                cache.clear();
            }
            if let Ok(mut cache) = self.stored_segments.lock() {
                cache.clear();
            }
            usize::try_from(purged).map_err(|_| unavailable("purge count"))
        })
    }
}

/// Published Vector manifests and authority scope epochs.
pub struct PgVectorGenerations {
    pool: PgPool,
}

impl PgVectorGenerations {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn manifest_of(value: serde_json::Value) -> Result<VectorProjectionManifest, SearchError> {
    serde_json::from_value(value).map_err(|_| unavailable("manifest decode"))
}

impl VectorGenerationPort for PgVectorGenerations {
    fn publish_if_current<'a>(
        &'a self,
        manifest: &'a VectorProjectionManifest,
        scope_epoch: u64,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let key = manifest.bundle_key;
            let mut tx = self.pool.begin().await.map_err(sql)?;
            let current: Option<Option<Uuid>> = sqlx::query_scalar(
                "SELECT current_generation_id FROM search_source_coordination \
                 WHERE source_id=$1 FOR SHARE",
            )
            .bind(key.source_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql)?;
            if current.flatten() != Some(key.generation_id.as_uuid()) {
                return Ok(false);
            }
            let epoch: Option<i64> = sqlx::query_scalar(
                "SELECT epoch FROM search_vector_scope_epoch WHERE authority_scope_key=$1 FOR SHARE",
            )
            .bind(&manifest.authority_scope_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql)?;
            if u64::try_from(epoch.unwrap_or(0)).ok() != Some(scope_epoch) {
                return Ok(false);
            }
            sqlx::query(
                "DELETE FROM search_vector_generation \
                 WHERE source_id=$1 AND generation_id=$2 AND model_id=$3",
            )
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .bind(manifest.model_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
            sqlx::query(
                "INSERT INTO search_vector_generation (source_id,generation_id,model_id, \
                 authority_scope_key,index_digest,manifest,published_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,clock_timestamp())",
            )
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .bind(manifest.model_id.as_str())
            .bind(&manifest.authority_scope_key)
            .bind(&manifest.index.index_digest)
            .bind(serde_json::to_value(manifest).map_err(|_| unavailable("manifest"))?)
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
            tx.commit().await.map_err(sql)?;
            Ok(true)
        })
    }

    fn scope_epoch<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, u64> {
        Box::pin(async move {
            let epoch: Option<i64> = sqlx::query_scalar(
                "SELECT epoch FROM search_vector_scope_epoch WHERE authority_scope_key=$1",
            )
            .bind(authority_scope_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(sql)?;
            u64::try_from(epoch.unwrap_or(0)).map_err(|_| unavailable("epoch"))
        })
    }

    fn advance_scope_epoch<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, u64> {
        Box::pin(async move {
            let epoch: i64 = sqlx::query_scalar(
                "INSERT INTO search_vector_scope_epoch (authority_scope_key, epoch) VALUES ($1, 1) \
                 ON CONFLICT (authority_scope_key) DO UPDATE \
                 SET epoch = search_vector_scope_epoch.epoch + 1 RETURNING epoch",
            )
            .bind(authority_scope_key)
            .fetch_one(&self.pool)
            .await
            .map_err(sql)?;
            u64::try_from(epoch).map_err(|_| unavailable("epoch"))
        })
    }

    fn pin_current<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
    ) -> BoxFuture<'a, Option<PinnedVectorGeneration>> {
        Box::pin(async move {
            let manifest: Option<serde_json::Value> = sqlx::query_scalar(
                "SELECT manifest FROM search_vector_generation \
                 WHERE source_id=$1 AND generation_id=$2 AND model_id=$3",
            )
            .bind(bundle.source_id.as_uuid())
            .bind(bundle.generation_id.as_uuid())
            .bind(model.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql)?;
            manifest
                .map(|value| manifest_of(value).map(PinnedVectorGeneration::new))
                .transpose()
        })
    }

    fn published<'a>(&'a self) -> BoxFuture<'a, Vec<VectorProjectionManifest>> {
        Box::pin(async move {
            let rows: Vec<serde_json::Value> = sqlx::query_scalar(
                "SELECT manifest FROM search_vector_generation ORDER BY source_id, generation_id",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            rows.into_iter().map(manifest_of).collect()
        })
    }

    fn withdraw<'a>(&'a self, manifest: &'a VectorProjectionManifest) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "DELETE FROM search_vector_generation WHERE source_id=$1 AND generation_id=$2 \
                 AND model_id=$3 AND index_digest=$4",
            )
            .bind(manifest.bundle_key.source_id.as_uuid())
            .bind(manifest.bundle_key.generation_id.as_uuid())
            .bind(manifest.model_id.as_str())
            .bind(&manifest.index.index_digest)
            .execute(&self.pool)
            .await
            .map_err(sql)?;
            Ok(())
        })
    }
}

#[allow(dead_code)]
fn _key(source: Uuid, generation: Uuid) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: search_application::search_core::id::SourceId::from_uuid(source),
        generation_id: ProjectionGenerationId::from_uuid(generation),
    }
}
