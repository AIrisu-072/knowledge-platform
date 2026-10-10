//! P7-08: READY in one commit from the real stored payloads, the sealed
//! lexical directory and the PostgreSQL Graph rows.
//!
//! Every artifact is prevalidated first. One short transaction then locks the
//! P7 target, its full guard and the Graph parent, revalidates the Graph rows
//! on the same connection, and commits Graph READY, the bundle receipt and P7
//! READY together. The P1 Graph input digest and the P3 Graph content digest
//! are computed separately from the same restored records and only compared
//! (`GraphReceiptMappingV1`); one is never copied into the other's field.
//! READY never moves the current pointer, and files and rows are rechecked
//! again by pin and before results are returned.

use std::collections::BTreeSet;
use std::path::PathBuf;

use document_domain::DocumentId;
use search_application::graph_generation::{
    GraphBuildRef, GraphResourceRecord, GraphSourceMapping, GraphStageReport,
};
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::relation::TypedRelationInstance;
use search_application::search_core::source::DiscoverableSource;
use search_graph::{GRAPH_SCHEMA_VERSION, canonical_relation};
use search_source_document::{
    ArtifactReceipt, BodyBuildError, BodyCoverageArtifact, GenerationBundleReceipt,
    compute_bundle_receipt_from, graph_receipt,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::full_guard::{EventCandidateHandle, ManualBuildHandle};
use crate::lexical_artifact::{LexicalArtifactError, LexicalArtifactStore, LexicalSealV1};
use crate::payload::{BundleError, PgPayloadStore, RestoredSummaryV1, UnitManifestSummaryV1};

pub const GRAPH_BACKEND: &str = "postgresql";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyError {
    Payload(BundleError),
    Lexical(LexicalArtifactError),
    /// The Graph rows do not validate as this target's staged generation.
    Graph,
    /// P1 Graph input and P3 Graph content do not describe the same records.
    Mapping,
    /// The target is not BUILDING with its live, exact full guard any more.
    Fence,
    /// The handle has no Graph parent registered in its commit.
    NoGraph,
    StoreUnknown,
}

impl From<sqlx::Error> for ReadyError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23514" | "42501") => Self::Fence,
            _ => Self::StoreUnknown,
        }
    }
}

impl From<search_graph::GraphError> for ReadyError {
    fn from(error: search_graph::GraphError) -> Self {
        match error {
            search_graph::GraphError::FenceLost => Self::Fence,
            search_graph::GraphError::Store => Self::StoreUnknown,
            _ => Self::Graph,
        }
    }
}

/// Proof that one READY commit happened for these exact receipts. Private
/// fields; it never grants publication or a pin.
/// `compute_bundle_receipt` over a Unit manifest summary.
fn summary_bundle_receipt(
    key: ProjectionGenerationKey,
    source_snapshot: &str,
    projection_manifest_digest: &str,
    units: &UnitManifestSummaryV1,
    coverage: &BodyCoverageArtifact,
    lexical: ArtifactReceipt,
    graph: ArtifactReceipt,
) -> Result<GenerationBundleReceipt, BodyBuildError> {
    if units.key != key || units.source_snapshot != source_snapshot {
        return Err(BodyBuildError::Integrity("bundle key"));
    }
    compute_bundle_receipt_from(
        key,
        source_snapshot,
        projection_manifest_digest,
        units.receipt,
        units.items,
        units.profile_set_digest,
        coverage,
        lexical,
        graph,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBundle {
    key: ProjectionGenerationKey,
    receipt: GenerationBundleReceipt,
    graph: GraphStageReport,
}

impl VerifiedBundle {
    pub fn key(&self) -> ProjectionGenerationKey {
        self.key
    }
    pub fn receipt(&self) -> &GenerationBundleReceipt {
        &self.receipt
    }
    pub fn graph(&self) -> &GraphStageReport {
        &self.graph
    }
}

fn sha256_text(digest: &[u8; 32]) -> String {
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// Compares the P1 Graph staged input with the P3 Graph content. Both come
/// from the same restored records and are recomputed, never copied.
pub struct GraphReceiptMappingV1;

impl GraphReceiptMappingV1 {
    pub fn validate(
        manifest: &ProjectionGenerationManifest,
        p1: &ArtifactReceipt,
        p1_relations: &[TypedRelationInstance],
        p3: &GraphStageReport,
        p3_relations: &[TypedRelationInstance],
    ) -> Result<(), ReadyError> {
        let canonical = |relations: &[TypedRelationInstance]| {
            relations
                .iter()
                .map(|relation| canonical_relation(relation).map_err(|_| ReadyError::Mapping))
                .collect::<Result<BTreeSet<_>, _>>()
        };
        let mut unique = p1_relations.to_vec();
        unique.sort_by_key(|relation| relation.relation_id);
        unique.dedup_by_key(|relation| relation.relation_id);
        if p1.key != manifest.key()
            || p3.key != manifest.key()
            || p3.source_snapshot != manifest.source_snapshot
            || p3.projection_manifest_digest != manifest.digest
            || p3.graph_schema_version != GRAPH_SCHEMA_VERSION
            || p1.count != p3.relation_count
            || u64::try_from(unique.len()).ok() != Some(p1.count)
            || canonical(&unique)? != canonical(p3_relations)?
        {
            return Err(ReadyError::Mapping);
        }
        Ok(())
    }
}

/// Document owners of structural Graph Resources, as P1 records them.
type Owners = Vec<(search_application::search_core::id::ResourceId, DocumentId)>;

/// The structural owners of the generation last re-verified by this process,
/// from its verified Graph rows: the load that follows takes them instead of
/// reading the same immutable rows again.
type VerifiedOwners = std::sync::Mutex<Option<(ProjectionGenerationKey, String, Owners)>>;

fn verified_owners() -> &'static VerifiedOwners {
    static OWNERS: std::sync::OnceLock<VerifiedOwners> = std::sync::OnceLock::new();
    OWNERS.get_or_init(Default::default)
}

/// The owners `reverify` derived from `key`'s verified Graph rows, once.
pub(crate) fn take_verified_owners(
    key: ProjectionGenerationKey,
    manifest_digest: &str,
) -> Option<Owners> {
    let mut slot = verified_owners().lock().ok()?;
    match slot.take() {
        Some((at, digest, owners)) if at == key && digest == manifest_digest => Some(owners),
        _ => None,
    }
}

pub(crate) fn owners(resources: &[GraphResourceRecord]) -> Owners {
    resources
        .iter()
        .filter_map(|record| match &record.mapping {
            GraphSourceMapping::Document { document_id }
            | GraphSourceMapping::FolderPlacement { document_id, .. } => {
                Some((record.resource_ref, DocumentId::from_uuid(*document_id)))
            }
            _ => None,
        })
        .collect()
}

/// The single READY entry point for full builds with a Graph.
pub struct ReadyCoordinator {
    pool: PgPool,
    payloads: PgPayloadStore,
    lexical: LexicalArtifactStore,
    source: DiscoverableSource,
}

impl ReadyCoordinator {
    pub fn new(pool: PgPool, lexical_root: impl Into<PathBuf>, source: DiscoverableSource) -> Self {
        Self {
            payloads: PgPayloadStore::new(pool.clone()),
            lexical: LexicalArtifactStore::new(lexical_root, pool.clone()),
            pool,
            source,
        }
    }

    pub async fn ready_manual(
        &self,
        handle: &ManualBuildHandle,
    ) -> Result<VerifiedBundle, ReadyError> {
        let graph = handle.graph_target().ok_or(ReadyError::NoGraph)?;
        self.ready(handle.key(), &GraphBuildRef::Full(graph)).await
    }

    /// B6: an INCREMENTAL target becomes READY under its verified Graph
    /// build guard, with the same bundle checks as a full build.
    pub async fn ready_incremental(
        &self,
        handle: &search_application::graph_generation::BuildGuardHandle,
    ) -> Result<VerifiedBundle, ReadyError> {
        self.ready(handle.target_key(), &GraphBuildRef::Incremental(*handle))
            .await
    }

    pub async fn ready_event(
        &self,
        handle: &EventCandidateHandle,
    ) -> Result<VerifiedBundle, ReadyError> {
        let graph = handle.graph_target().ok_or(ReadyError::NoGraph)?;
        self.ready(handle.key(), &GraphBuildRef::Full(graph)).await
    }

    async fn manifest(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<ProjectionGenerationManifest, ReadyError> {
        let dto: serde_json::Value = sqlx::query_scalar(
            "SELECT projection_manifest FROM search_generation WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ReadyError::Fence)?;
        let manifest: ProjectionGenerationManifest = serde_json::from_value(
            dto.get("manifest")
                .cloned()
                .ok_or(ReadyError::StoreUnknown)?,
        )
        .map_err(|_| ReadyError::StoreUnknown)?;
        if manifest.key() != key {
            return Err(ReadyError::StoreUnknown);
        }
        Ok(manifest)
    }

    /// P7-12: re-verifies a READY generation from what is stored now: the
    /// payload DTOs and composite digest, the lexical directory, and the Graph
    /// rows against both their READY receipt and the projection. Nothing is
    /// written; any drift is an error and the key must not be served.
    pub async fn reverify(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<VerifiedBundle, ReadyError> {
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM search_generation WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        if state.as_deref() != Some("READY") {
            return Err(ReadyError::Fence);
        }
        let manifest = self.manifest(key).await?;
        // T12: checked from per-segment summaries; no Unit text is held. The
        // lexical directory is read while the payloads are restored and then
        // compared with them; the Graph rows are read meanwhile.
        let artifacts = async {
            let inspect = {
                let (lexical, manifest, source) =
                    (self.lexical.clone(), manifest.clone(), self.source.clone());
                tokio::task::spawn_blocking(move || lexical.inspect_final(&manifest, &source))
            };
            let restored = self
                .payloads
                .restore_without_units(&manifest)
                .await
                .map_err(ReadyError::Payload)?;
            let inspected = inspect
                .await
                .map_err(|_| ReadyError::StoreUnknown)?
                .map_err(ReadyError::Lexical)?;
            let seal = self
                .lexical
                .validate_inspected_summary(&manifest, &self.source, inspected, &restored.units)
                .await
                .map_err(ReadyError::Lexical)?;
            Ok::<_, ReadyError>((restored, seal))
        };
        let rows = async {
            search_graph::PostgresGraphStore::new(self.pool.clone())
                .recover_rows(key, &manifest.digest)
                .await
                .map_err(ReadyError::from)
        };
        let (
            (
                RestoredSummaryV1 {
                    projection,
                    units,
                    coverage,
                },
                seal,
            ),
            (graph, resources, relations),
        ) = tokio::try_join!(artifacts, rows)?;
        let report = GraphStageReport {
            key,
            source_snapshot: graph.source_snapshot,
            projection_manifest_digest: graph.projection_manifest_digest,
            source_mapping_digest: graph.source_mapping_digest,
            graph_content_digest: graph.graph_content_digest,
            resource_count: graph.resource_count,
            relation_count: graph.relation_count,
            graph_schema_version: graph.graph_schema_version,
        };
        let p1_relations: Vec<TypedRelationInstance> = projection
            .resources
            .iter()
            .flat_map(|resource| resource.relations.clone())
            .collect();
        let verified = owners(&resources);
        let p1_graph = graph_receipt(key, &projection.resources, &verified)
            .map_err(|_| ReadyError::Mapping)?;
        GraphReceiptMappingV1::validate(&manifest, &p1_graph, &p1_relations, &report, &relations)?;
        let receipt = summary_bundle_receipt(
            key,
            &manifest.source_snapshot,
            &manifest.digest,
            &units,
            &coverage,
            ArtifactReceipt {
                key,
                digest: seal.logical_digest,
                count: seal.logical_count,
            },
            p1_graph,
        )
        .map_err(|_| ReadyError::Payload(BundleError::Digest))?;
        let stored: Option<(String, String, String)> = sqlx::query_as(
            "SELECT composite_digest, graph_content_digest, graph_mapping_digest              FROM search_generation_receipt WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        if stored
            != Some((
                sha256_text(&receipt.composite_digest),
                report.graph_content_digest.clone(),
                report.source_mapping_digest.clone(),
            ))
        {
            return Err(ReadyError::Payload(BundleError::Digest));
        }
        if let Ok(mut slot) = verified_owners().lock() {
            *slot = Some((key, manifest.digest.clone(), verified));
        }
        Ok(VerifiedBundle {
            key,
            receipt,
            graph: report,
        })
    }

    async fn ready(
        &self,
        key: ProjectionGenerationKey,
        graph_target: &GraphBuildRef,
    ) -> Result<VerifiedBundle, ReadyError> {
        // Prevalidation outside every lock: payload DTOs, the lexical files and
        // the Graph rows, each recomputed from what is stored.
        let manifest = self.manifest(key).await?;
        // T12: checked from per-segment summaries; no Unit text is held.
        let RestoredSummaryV1 {
            projection,
            units,
            coverage,
        } = self
            .payloads
            .restore_without_units(&manifest)
            .await
            .map_err(ReadyError::Payload)?;
        let seal: LexicalSealV1 = self
            .lexical
            .reopen_and_validate_summary(&manifest, &self.source, &units)
            .await
            .map_err(ReadyError::Lexical)?;
        let mut connection = self.pool.acquire().await?;
        let (report, graph_resources, graph_relations) =
            search_graph::store::validate_on(&mut connection, graph_target).await?;
        drop(connection);
        let p1_relations: Vec<TypedRelationInstance> = projection
            .resources
            .iter()
            .flat_map(|resource| resource.relations.clone())
            .collect();
        let p1_graph = graph_receipt(key, &projection.resources, &owners(&graph_resources))
            .map_err(|_| ReadyError::Mapping)?;
        GraphReceiptMappingV1::validate(
            &manifest,
            &p1_graph,
            &p1_relations,
            &report,
            &graph_relations,
        )?;
        let lexical = ArtifactReceipt {
            key,
            digest: seal.logical_digest,
            count: seal.logical_count,
        };
        let receipt = summary_bundle_receipt(
            key,
            &manifest.source_snapshot,
            &manifest.digest,
            &units,
            &coverage,
            lexical,
            p1_graph,
        )
        .map_err(|_| ReadyError::Payload(BundleError::Digest))?;

        // One short transaction: target → guard → Graph parent, then commit.
        let mut tx = self.pool.begin().await?;
        let target = sqlx::query(
            "SELECT state, build_kind, full_guard_token, full_build_fence FROM search_generation \
             WHERE source_id=$1 AND generation_id=$2 FOR UPDATE",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ReadyError::Fence)?;
        if target.try_get::<String, _>("state")? != "BUILDING" {
            return Err(ReadyError::Fence);
        }
        let kind: String = target.try_get("build_kind")?;
        let guard = match graph_target {
            GraphBuildRef::Full(graph) => {
                let (token, fence) = (graph.guard_token(), graph.build_fence());
                if kind != "FULL"
                    || target.try_get::<Option<Uuid>, _>("full_guard_token")? != Some(token)
                    || target.try_get::<Option<i64>, _>("full_build_fence")? != Some(fence)
                {
                    return Err(ReadyError::Fence);
                }
                sqlx::query(
                    "SELECT 1 FROM search_generation_full_guard WHERE source_id=$1 \
                     AND target_generation_id=$2 AND guard_token=$3 AND build_fence=$4 \
                     AND expires_at > clock_timestamp() FOR UPDATE",
                )
                .bind(key.source_id.as_uuid())
                .bind(key.generation_id.as_uuid())
                .bind(token)
                .bind(fence)
                .fetch_optional(&mut *tx)
                .await?
            }
            GraphBuildRef::Incremental(handle) => {
                if kind != "INCREMENTAL" {
                    return Err(ReadyError::Fence);
                }
                sqlx::query(
                    "SELECT 1 FROM search_graph.build_guard WHERE source_id=$1 \
                     AND target_generation_id=$2 AND guard_token=$3 AND fence=$4 \
                     AND copy_verified_at IS NOT NULL \
                     AND expires_at > clock_timestamp() FOR UPDATE",
                )
                .bind(key.source_id.as_uuid())
                .bind(key.generation_id.as_uuid())
                .bind(handle.guard_token())
                .bind(handle.build_fence())
                .fetch_optional(&mut *tx)
                .await?
            }
        };
        guard.ok_or(ReadyError::Fence)?;
        let lexical_row: Option<(String, String)> = sqlx::query_as(
            "SELECT logical_digest, tree_digest FROM search_lexical_artifact \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        if lexical_row
            != Some((
                sha256_text(&seal.logical_digest),
                sha256_text(&seal.tree_digest),
            ))
        {
            return Err(ReadyError::Lexical(LexicalArtifactError::Drift));
        }
        search_graph::store::settle_ready_on(&mut tx, graph_target, &report)
            .await
            .map_err(|error| match error {
                search_graph::GraphError::Integrity(_) => ReadyError::Graph,
                other => other.into(),
            })?;
        let count = |n: u64| i64::try_from(n).map_err(|_| ReadyError::Mapping);
        sqlx::query(
            "INSERT INTO search_generation_receipt (source_id,generation_id,source_snapshot, \
             receipt_version,projection_digest,unit_manifest_digest,unit_count, \
             body_coverage_digest,body_item_count,lexical_digest,lexical_count, \
             lexical_schema_version,lexical_analyzer_version,graph_input_digest, \
             graph_input_count,profile_set_digest,composite_digest,graph_backend, \
             graph_schema_version,graph_mapping_digest,graph_content_digest, \
             graph_resource_count,graph_relation_count,vector_receipt,receipt_dto) \
             VALUES ($1,$2,$3,'v1',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18, \
             $19,$20,$21,$22,NULL,$23)",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(&manifest.source_snapshot)
        .bind(&manifest.digest)
        .bind(sha256_text(&receipt.unit_manifest.digest))
        .bind(count(receipt.unit_manifest.count)?)
        .bind(sha256_text(&receipt.body_coverage.digest))
        .bind(count(receipt.body_coverage.count)?)
        .bind(sha256_text(&receipt.lexical.digest))
        .bind(count(receipt.lexical.count)?)
        .bind(&seal.schema_version)
        .bind(&seal.analyzer_version)
        .bind(sha256_text(&receipt.graph.digest))
        .bind(count(receipt.graph.count)?)
        .bind(sha256_text(&receipt.profile_set_digest))
        .bind(sha256_text(&receipt.composite_digest))
        .bind(GRAPH_BACKEND)
        .bind(&report.graph_schema_version)
        .bind(&report.source_mapping_digest)
        .bind(&report.graph_content_digest)
        .bind(count(report.resource_count)?)
        .bind(count(report.relation_count)?)
        .bind(serde_json::json!({"dto_version": "v1", "bundle": &receipt}))
        .execute(&mut *tx)
        .await?;
        let settled = sqlx::query(
            "UPDATE search_generation SET state='READY', ready_at=clock_timestamp() \
             WHERE source_id=$1 AND generation_id=$2 AND state='BUILDING'",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .execute(&mut *tx)
        .await?;
        if settled.rows_affected() != 1 {
            return Err(ReadyError::Fence);
        }
        tx.commit().await.map_err(|_| ReadyError::StoreUnknown)?;
        Ok(VerifiedBundle {
            key,
            receipt,
            graph: report,
        })
    }
}
