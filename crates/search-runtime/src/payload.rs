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
/// Upper bound of one stored payload document, in bytes of its JSON text.
const MAX_PAYLOAD_BYTES: usize = 256 * 1024 * 1024;

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
}

impl PgPayloadStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
            sqlx::query(
                "INSERT INTO search_generation_payload \
                 (source_id,generation_id,kind,dto_version,payload,logical_digest,logical_count) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7)",
            )
            .bind(validated.key.source_id.as_uuid())
            .bind(validated.key.generation_id.as_uuid())
            .bind(kind)
            .bind(PAYLOAD_DTO_VERSION)
            .bind(payload)
            .bind(digest)
            .bind(i64::try_from(count).map_err(|_| BundleError::Binding)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
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
        let rows = sqlx::query(
            "SELECT kind, dto_version, payload::text AS payload, logical_digest, logical_count \
             FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2 \
             ORDER BY kind",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        let (mut projection, mut units, mut coverage) = (None, None, None);
        let mut columns = std::collections::BTreeMap::new();
        for row in rows {
            let kind: String = row.try_get("kind")?;
            let version: String = row.try_get("dto_version")?;
            let text: String = row.try_get("payload")?;
            if version != PAYLOAD_DTO_VERSION {
                return Err(BundleError::Shape);
            }
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
            let digest: String = row.try_get("logical_digest")?;
            let count: i64 = row.try_get("logical_count")?;
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
