//! P7-04: typed durable payloads of one generation bundle.
//!
//! Each stored DTO is restored through `deny_unknown_fields` types and every
//! digest is recomputed from the restored values with the P1 encoders; a stored
//! digest column is never trusted on its own. Lexical index bytes and Graph
//! rows are verified by their own owners (P7-05 / P3) before READY.

use search_application::ports::SemanticRegistrySnapshot;
use search_application::search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_projection_memory::generation_digest;
use search_source_document::{
    BodyCoverageArtifact, BodyUnitManifest, GenerationBundleReceipt, compute_bundle_receipt,
    validate_restored_manifest,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

pub const PAYLOAD_DTO_VERSION: &str = "v1";
/// Upper bound of one restored payload document, in bytes of its JSON text.
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024 * 1024;
/// A payload whose JSON text is longer is stored as ordered text chunks of
/// at most this many bytes, below the 256 MiB limit of one JSONB value.
const CHUNK_BYTES: usize = 64 * 1024 * 1024;

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
    let rows = sqlx::query(
        "SELECT kind, chunk, dto_version, payload::text AS payload, payload ? 'chunk' AS chunked, \
         payload ->> 'chunk' AS chunk_text, logical_digest, logical_count \
         FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2 \
         ORDER BY kind, chunk",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(pool)
    .await?;
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
    let derived =
        validate_restored_manifest(&bundle.unit_manifest).map_err(|_| BundleError::Digest)?;
    if derived != bundle.coverage {
        return Err(BundleError::Binding);
    }
    let receipt = compute_bundle_receipt(
        key,
        snapshot,
        &bundle.manifest.digest,
        &bundle.unit_manifest,
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
                envelope(&bundle.unit_manifest)?,
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
        let mut tx = self.pool.begin().await?;
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

    /// Restores the payload DTOs of `manifest` and checks everything that does
    /// not depend on external artifacts: the projection-only digest, the Unit
    /// manifest structure, derived coverage and the stored digest columns.
    pub async fn restore(
        &self,
        manifest: &ProjectionGenerationManifest,
    ) -> Result<RestoredPayloadV1, BundleError> {
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
                "unit_manifest" => units.replace(restore::<BodyUnitManifest>(&text)?).is_some(),
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
        let (Some(projection), Some(unit_manifest), Some(coverage)) = (projection, units, coverage)
        else {
            return Err(BundleError::Shape);
        };
        if unit_manifest.key != key
            || coverage.key != key
            || unit_manifest.source_snapshot != manifest.source_snapshot
            || projection.registry.version != manifest.semantic_registry_version
            || u64::try_from(projection.resources.len()).ok() != Some(manifest.resource_count)
        {
            return Err(BundleError::Binding);
        }
        let projection_digest =
            generation_digest(key.source_id, &projection.resources, &projection.registry)
                .map_err(|_| BundleError::Digest)?;
        if projection_digest != manifest.digest {
            return Err(BundleError::Digest);
        }
        if validate_restored_manifest(&unit_manifest).map_err(|_| BundleError::Digest)? != coverage
        {
            return Err(BundleError::Binding);
        }
        let units_receipt = search_source_document::unit_manifest_receipt(&unit_manifest)
            .map_err(|_| BundleError::Digest)?;
        let coverage_receipt =
            search_source_document::coverage_receipt(&coverage).map_err(|_| BundleError::Digest)?;
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
        Ok(RestoredPayloadV1 {
            projection,
            unit_manifest,
            coverage,
        })
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
                "unit_manifest" => units.replace(restore::<BodyUnitManifest>(&text)?).is_some(),
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
        let (Some(projection), Some(unit_manifest), Some(coverage)) = (projection, units, coverage)
        else {
            return Err(BundleError::Shape);
        };
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
