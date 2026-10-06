//! E (P2-07): the PostgreSQL Vector index and generation ports.
//!
//! `PgVectorIndex` writes each stage's entries with their vector bytes and a
//! digest it recomputes on every read; a mismatch makes the stage
//! unavailable, never a silent result. Search is an exact cosine scan of the
//! pinned stage with the configured similarity floor: entries below it are
//! never candidates. `PgVectorGenerations` publishes a manifest only while
//! its P7 bundle key is still the Source's current key and its authority
//! scope is still at the epoch the build read, in one transaction.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::search_core::id::ProjectionGenerationId;
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

#[derive(Serialize, Deserialize)]
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

/// One stage loaded for scans.
struct LoadedStage {
    model_id: EmbeddingModelId,
    entries: Vec<(VectorHitRef, Vec<f32>)>,
}

/// The exact-scan index over PostgreSQL rows.
pub struct PgVectorIndex {
    pool: PgPool,
    floor: f32,
    loaded: Mutex<BTreeMap<String, Arc<LoadedStage>>>,
}

impl PgVectorIndex {
    /// `floor`: the minimum cosine similarity of a candidate.
    pub fn new(pool: PgPool, floor: f32) -> Self {
        Self {
            pool,
            floor,
            loaded: Mutex::new(BTreeMap::new()),
        }
    }

    async fn load(&self, index_digest: &str) -> Result<Arc<LoadedStage>, SearchError> {
        if let Some(stage) = self
            .loaded
            .lock()
            .map_err(|_| unavailable("lock"))?
            .get(index_digest)
        {
            return Ok(stage.clone());
        }
        let stage = sqlx::query(
            "SELECT model_id, entry_count FROM search_vector_stage WHERE index_digest=$1",
        )
        .bind(index_digest)
        .fetch_optional(&self.pool)
        .await
        .map_err(sql)?
        .ok_or_else(|| unavailable("missing stage"))?;
        let model_id =
            EmbeddingModelId::parse(&stage.try_get::<String, _>("model_id").map_err(sql)?)
                .map_err(|_| unavailable("stage model"))?;
        let count: i32 = stage.try_get("entry_count").map_err(sql)?;
        let rows = sqlx::query(
            "SELECT entry_ref, vector, vector_sha256 FROM search_vector_entry \
             WHERE index_digest=$1 ORDER BY ordinal",
        )
        .bind(index_digest)
        .fetch_all(&self.pool)
        .await
        .map_err(sql)?;
        if i32::try_from(rows.len()).ok() != Some(count) {
            return Err(unavailable("stage entry count"));
        }
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let bytes: Vec<u8> = row.try_get("vector").map_err(sql)?;
            if sha256_text(&bytes) != row.try_get::<String, _>("vector_sha256").map_err(sql)? {
                return Err(unavailable("vector bytes changed"));
            }
            let dto: EntryDto = serde_json::from_value(row.try_get("entry_ref").map_err(sql)?)
                .map_err(|_| unavailable("entry decode"))?;
            let values = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect();
            entries.push((dto.hit, values));
        }
        let stage = Arc::new(LoadedStage { model_id, entries });
        self.loaded
            .lock()
            .map_err(|_| unavailable("lock"))?
            .insert(index_digest.to_owned(), stage.clone());
        Ok(stage)
    }

    fn forget(&self, index_digest: &str) {
        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.remove(index_digest);
        }
    }
}

impl VectorIndexPort for PgVectorIndex {
    fn stage<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
        embeddings: &'a [BoundEmbedding],
    ) -> BoxFuture<'a, VectorIndexDescriptor> {
        Box::pin(async move {
            let entries: Vec<VectorEntryRef> =
                embeddings.iter().map(BoundEmbedding::entry_ref).collect();
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
            .bind(i32::try_from(embeddings.len()).map_err(|_| unavailable("too many entries"))?)
            .execute(&mut *tx)
            .await
            .map_err(sql)?;
            for (ordinal, (embedding, entry)) in embeddings.iter().zip(&entries).enumerate() {
                let bytes = vector_bytes(embedding.values());
                sqlx::query(
                    "INSERT INTO search_vector_entry (index_digest,ordinal,authority_scope_key, \
                     entry_ref,vector,vector_sha256) VALUES ($1,$2,$3,$4,$5,$6)",
                )
                .bind(&descriptor.index_digest)
                .bind(i32::try_from(ordinal).map_err(|_| unavailable("ordinal"))?)
                .bind(&entry.cache_key.authority_scope_key)
                .bind(serde_json::to_value(EntryDto::of(entry)).map_err(|_| unavailable("entry"))?)
                .bind(&bytes)
                .bind(sha256_text(&bytes))
                .execute(&mut *tx)
                .await
                .map_err(sql)?;
            }
            tx.commit().await.map_err(sql)?;
            Ok(descriptor)
        })
    }

    fn staged_entries<'a>(
        &'a self,
        index: &'a VectorIndexDescriptor,
    ) -> BoxFuture<'a, Vec<VectorEntryRef>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT e.entry_ref FROM search_vector_entry e \
                 JOIN search_vector_stage s USING (index_digest) \
                 WHERE e.index_digest=$1 ORDER BY e.ordinal",
            )
            .bind(&index.index_digest)
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            rows.into_iter()
                .map(|row| {
                    let dto: EntryDto =
                        serde_json::from_value(row.try_get("entry_ref").map_err(sql)?)
                            .map_err(|_| unavailable("entry decode"))?;
                    dto.into_entry()
                })
                .collect()
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
            if stage.model_id != pin.manifest().model_id || query.model_id() != &stage.model_id {
                return Err(unavailable("model binding"));
            }
            let values = query.values();
            let mut scored: Vec<(f32, usize)> = stage
                .entries
                .iter()
                .enumerate()
                .map(|(position, (_, vector))| {
                    (
                        vector.iter().zip(values).map(|(a, b)| a * b).sum::<f32>(),
                        position,
                    )
                })
                .filter(|(similarity, _)| *similarity >= self.floor)
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            Ok(scored
                .into_iter()
                .take(window)
                .enumerate()
                .map(|(rank, (similarity, position))| RankedVectorHit {
                    hit: stage.entries[position].0.clone(),
                    model_id: stage.model_id.clone(),
                    rank: rank + 1,
                    raw_similarity: similarity,
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
            Ok(())
        })
    }

    fn purge_scope<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let purged =
                sqlx::query("DELETE FROM search_vector_entry WHERE authority_scope_key=$1")
                    .bind(authority_scope_key)
                    .execute(&self.pool)
                    .await
                    .map_err(sql)?
                    .rows_affected();
            if let Ok(mut loaded) = self.loaded.lock() {
                loaded.clear();
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

impl PgVectorIndex {
    /// Stored vectors of the Source's published stages by cache key, for
    /// reuse when the same Unit text appears in a newer bundle.
    pub async fn values_by_cache_key(
        &self,
        source: search_application::search_core::id::SourceId,
        model: &EmbeddingModelId,
    ) -> Result<std::collections::HashMap<EmbeddingCacheKey, Vec<f32>>, SearchError> {
        let digests: Vec<String> = sqlx::query_scalar(
            "SELECT index_digest FROM search_vector_generation WHERE source_id=$1 AND model_id=$2",
        )
        .bind(source.as_uuid())
        .bind(model.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(sql)?;
        let mut out = std::collections::HashMap::new();
        for digest in digests {
            let rows = sqlx::query(
                "SELECT entry_ref, vector, vector_sha256 FROM search_vector_entry WHERE index_digest=$1",
            )
            .bind(&digest)
            .fetch_all(&self.pool)
            .await
            .map_err(sql)?;
            for row in rows {
                let bytes: Vec<u8> = row.try_get("vector").map_err(sql)?;
                if sha256_text(&bytes) != row.try_get::<String, _>("vector_sha256").map_err(sql)? {
                    continue;
                }
                let dto: EntryDto = serde_json::from_value(row.try_get("entry_ref").map_err(sql)?)
                    .map_err(|_| unavailable("entry decode"))?;
                let values = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| f32::from_le_bytes(*chunk))
                    .collect();
                out.insert(dto.cache_key, values);
            }
        }
        Ok(out)
    }
}
