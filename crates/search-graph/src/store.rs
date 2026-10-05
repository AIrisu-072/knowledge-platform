//! P3-G04 (with P7-07 and the G08 READY recovery): the PostgreSQL Graph store
//! behind the application ports. Every write locks its BUILDING parent first;
//! the database trigger independently requires the live guard. Validation and
//! recovery rebuild every typed relation from rows and recompute all digests.

use std::collections::{BTreeMap, BTreeSet};

use search_application::SearchError;
use search_application::graph_generation::{
    BuildGuardHandle, DurableGraphGenerationPort, GraphBatchCursor, GraphBuildRef,
    GraphGenerationReceipt, GraphIncrementalDelta, GraphResourceRecord, GraphSourceMapping,
    GraphStage, GraphStageReport, RegisteredFullBuildHandle,
};
use search_application::ports::BoxFuture;
use search_core::id::{ResourceId, ResourceVersionId};
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::{RelationNamespace, TypedRelationInstance};
use search_core::resource::ResourceKind;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::GraphError;
use crate::canonical::{
    GRAPH_SCHEMA_VERSION, Instant, TemporalColumns, canonical_graph_digest,
    canonical_mapping_digest, decode_temporal, encode_temporal, relation_digest,
};

const RELATION_DTO_VERSION: &str = "v1";

fn kind_text(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Knowledge => "KNOWLEDGE",
        ResourceKind::Document => "DOCUMENT",
        ResourceKind::FolderPlacement => "FOLDER_PLACEMENT",
        ResourceKind::Semantic => "SEMANTIC",
        ResourceKind::Capability => "CAPABILITY",
        ResourceKind::AgentSkill => "AGENT_SKILL",
        ResourceKind::Workflow => "WORKFLOW",
        ResourceKind::Policy => "POLICY",
    }
}

fn kind_from(text: &str) -> Result<ResourceKind, GraphError> {
    Ok(match text {
        "KNOWLEDGE" => ResourceKind::Knowledge,
        "DOCUMENT" => ResourceKind::Document,
        "FOLDER_PLACEMENT" => ResourceKind::FolderPlacement,
        "SEMANTIC" => ResourceKind::Semantic,
        "CAPABILITY" => ResourceKind::Capability,
        "AGENT_SKILL" => ResourceKind::AgentSkill,
        "WORKFLOW" => ResourceKind::Workflow,
        "POLICY" => ResourceKind::Policy,
        _ => return Err(GraphError::Integrity("resource kind")),
    })
}

fn namespace_text(namespace: RelationNamespace) -> &'static str {
    match namespace {
        RelationNamespace::Discovery => "DISCOVERY",
        RelationNamespace::Semantic => "SEMANTIC",
        RelationNamespace::Evidence => "EVIDENCE",
    }
}

/// Registers the Graph parent inside the caller's P7 registration
/// transaction. The database admits it only for the BUILDING P7 FULL target
/// with the same Source snapshot and manifest digest.
#[allow(clippy::too_many_arguments)]
pub async fn register_full_on(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    guard_token: Uuid,
    build_fence: i64,
    source_snapshot: &str,
    projection_manifest_digest: &str,
    source_mapping_digest: &str,
) -> Result<RegisteredFullBuildHandle, GraphError> {
    sqlx::query(
        "INSERT INTO search_graph.generation (source_id,generation_id,graph_schema_version, \
         build_kind,full_guard_token,full_build_fence,source_snapshot, \
         projection_manifest_digest,source_mapping_digest,state) \
         VALUES ($1,$2,$3,'FULL',$4,$5,$6,$7,$8,'BUILDING')",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(GRAPH_SCHEMA_VERSION)
    .bind(guard_token)
    .bind(build_fence)
    .bind(source_snapshot)
    .bind(projection_manifest_digest)
    .bind(source_mapping_digest)
    .execute(connection)
    .await?;
    Ok(RegisteredFullBuildHandle::from_identifiers(
        key,
        guard_token,
        build_fence,
    ))
}

struct Parent {
    state: String,
    build_kind: String,
    full_guard_token: Option<Uuid>,
    full_build_fence: Option<i64>,
    base: Option<Uuid>,
    build_guard_token: Option<Uuid>,
    build_fence: Option<i64>,
    source_snapshot: String,
    projection_manifest_digest: String,
    source_mapping_digest: String,
    graph_content_digest: Option<String>,
    resource_count: Option<i64>,
    relation_count: Option<i64>,
}

async fn parent(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    lock: &str,
) -> Result<Option<Parent>, GraphError> {
    let statement = format!(
        "SELECT state,build_kind,full_guard_token,full_build_fence, \
         incremental_base_generation_id,build_guard_token,build_fence,source_snapshot, \
         projection_manifest_digest,source_mapping_digest,graph_content_digest, \
         resource_count,relation_count FROM search_graph.generation \
         WHERE source_id=$1 AND generation_id=$2 {lock}"
    );
    let Some(row) = sqlx::query(sqlx::AssertSqlSafe(statement))
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(connection)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Parent {
        state: row.try_get("state")?,
        build_kind: row.try_get("build_kind")?,
        full_guard_token: row.try_get("full_guard_token")?,
        full_build_fence: row.try_get("full_build_fence")?,
        base: row.try_get("incremental_base_generation_id")?,
        build_guard_token: row.try_get("build_guard_token")?,
        build_fence: row.try_get("build_fence")?,
        source_snapshot: row.try_get("source_snapshot")?,
        projection_manifest_digest: row.try_get("projection_manifest_digest")?,
        source_mapping_digest: row.try_get("source_mapping_digest")?,
        graph_content_digest: row.try_get("graph_content_digest")?,
        resource_count: row.try_get("resource_count")?,
        relation_count: row.try_get("relation_count")?,
    }))
}

/// The parent must be BUILDING and bound to exactly this reference.
fn building_for(parent: Option<Parent>, target: &GraphBuildRef) -> Result<Parent, GraphError> {
    let parent = parent.ok_or(GraphError::FenceLost)?;
    let bound = match target {
        GraphBuildRef::Full(handle) => {
            parent.build_kind == "FULL"
                && parent.full_guard_token == Some(handle.guard_token())
                && parent.full_build_fence == Some(handle.build_fence())
        }
        GraphBuildRef::Incremental(handle) => {
            parent.build_kind == "INCREMENTAL"
                && parent.base == Some(handle.base_key().generation_id.as_uuid())
                && parent.build_guard_token == Some(handle.guard_token())
                && parent.build_fence == Some(handle.build_fence())
        }
    };
    if parent.state != "BUILDING" || !bound {
        return Err(GraphError::FenceLost);
    }
    Ok(parent)
}

fn nanos(value: Option<Instant>) -> (Option<String>, Option<i32>) {
    (
        value.map(|instant| instant.epoch_nanos.to_string()),
        value.map(|instant| instant.offset_seconds),
    )
}

fn instant(nanos: Option<String>, offset: Option<i32>) -> Result<Option<Instant>, GraphError> {
    match (nanos, offset) {
        (None, None) => Ok(None),
        (Some(nanos), Some(offset_seconds)) => Ok(Some(Instant {
            epoch_nanos: nanos
                .parse()
                .map_err(|_| GraphError::Integrity("stored instant"))?,
            offset_seconds,
        })),
        _ => Err(GraphError::Integrity("stored instant pair")),
    }
}

async fn insert_rows(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    resources: &[GraphResourceRecord],
    relations: &[TypedRelationInstance],
) -> Result<(), GraphError> {
    for record in resources {
        let columns = encode_temporal(&record.temporal)?;
        let (owner, folder, version, adapter, native, mapping_kind) = match &record.mapping {
            GraphSourceMapping::Document { document_id } => {
                (Some(*document_id), None, None, None, None, "DOCUMENT")
            }
            GraphSourceMapping::FolderPlacement {
                document_id,
                folder_id,
            } => (
                Some(*document_id),
                Some(*folder_id),
                None,
                None,
                None,
                "FOLDER_PLACEMENT",
            ),
            GraphSourceMapping::Version {
                document_id,
                version_id,
            } => (
                Some(*document_id),
                None,
                Some(*version_id),
                None,
                None,
                "VERSION",
            ),
            GraphSourceMapping::Registered {
                adapter_id,
                native_id,
            } => (
                None,
                None,
                None,
                Some(adapter_id.clone()),
                Some(native_id.clone()),
                "REGISTERED",
            ),
        };
        let (valid_from, valid_from_offset) = nanos(columns.valid_from);
        let (valid_to, valid_to_offset) = nanos(columns.valid_to);
        let (anchor, anchor_offset) = nanos(columns.freshness_anchor_at);
        let (effective_from, effective_from_offset) = nanos(columns.effective_from);
        let (effective_to, effective_to_offset) = nanos(columns.effective_to);
        sqlx::query(
            "INSERT INTO search_graph.resource (source_id,generation_id,resource_id, \
             resource_kind,resource_version_id,mapping_kind,owner_document_id,folder_id, \
             version_id,adapter_id,native_id,valid_from_nanos,valid_from_offset, \
             valid_to_nanos,valid_to_offset,freshness_anchor_nanos,freshness_anchor_offset, \
             freshness_basis,effective_from_nanos,effective_from_offset,effective_to_nanos, \
             effective_to_offset) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11, \
             $12::numeric,$13,$14::numeric,$15,$16::numeric,$17,$18,$19::numeric,$20, \
             $21::numeric,$22)",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(record.resource_ref.as_uuid())
        .bind(kind_text(record.kind))
        .bind(record.resource_version_ref.map(|version| version.as_uuid()))
        .bind(mapping_kind)
        .bind(owner)
        .bind(folder)
        .bind(version)
        .bind(adapter)
        .bind(native)
        .bind(valid_from)
        .bind(valid_from_offset)
        .bind(valid_to)
        .bind(valid_to_offset)
        .bind(anchor)
        .bind(anchor_offset)
        .bind(columns.freshness_basis)
        .bind(effective_from)
        .bind(effective_from_offset)
        .bind(effective_to)
        .bind(effective_to_offset)
        .execute(&mut *connection)
        .await?;
    }
    for relation in relations {
        let payload = serde_json::json!({
            "dto_version": RELATION_DTO_VERSION,
            "relation": relation,
        });
        sqlx::query(
            "INSERT INTO search_graph.relation (source_id,generation_id,relation_id,namespace, \
             relation_type,payload,canonical_digest) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(relation.relation_id.as_uuid())
        .bind(namespace_text(relation.namespace))
        .bind(&relation.relation_type)
        .bind(payload)
        .bind(relation_digest(relation)?)
        .execute(&mut *connection)
        .await?;
        for (ordinal, participant) in relation.participants.iter().enumerate() {
            sqlx::query(
                "INSERT INTO search_graph.participant (source_id,generation_id,relation_id, \
                 ordinal,role,resource_id) VALUES ($1,$2,$3,$4,$5,$6)",
            )
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .bind(relation.relation_id.as_uuid())
            .bind(i32::try_from(ordinal).map_err(|_| GraphError::Invalid("participants"))?)
            .bind(&participant.role)
            .bind(participant.resource_ref.as_uuid())
            .execute(&mut *connection)
            .await?;
        }
    }
    Ok(())
}

/// Rebuilds every Resource and typed relation of one generation from rows,
/// checking stored relation digests and the participant incidence rows.
pub(crate) async fn load_rows(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
) -> Result<(Vec<GraphResourceRecord>, Vec<TypedRelationInstance>), GraphError> {
    let relation_rows = sqlx::query(
        "SELECT relation_id, payload, canonical_digest FROM search_graph.relation \
         WHERE source_id=$1 AND generation_id=$2 ORDER BY relation_id",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(&mut *connection)
    .await?;
    let mut relations = Vec::with_capacity(relation_rows.len());
    for row in &relation_rows {
        relations.push(relation_from_row(row)?);
    }
    let participant_rows = sqlx::query(
        "SELECT relation_id, ordinal, role, resource_id FROM search_graph.participant \
         WHERE source_id=$1 AND generation_id=$2 ORDER BY relation_id, ordinal",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(&mut *connection)
    .await?;
    let mut incidence: BTreeMap<Uuid, Vec<(String, Uuid)>> = BTreeMap::new();
    for row in participant_rows {
        incidence
            .entry(row.try_get("relation_id")?)
            .or_default()
            .push((row.try_get("role")?, row.try_get("resource_id")?));
    }
    for relation in &relations {
        let stored = incidence
            .remove(&relation.relation_id.as_uuid())
            .unwrap_or_default();
        let expected: Vec<(String, Uuid)> = relation
            .participants
            .iter()
            .map(|participant| (participant.role.clone(), participant.resource_ref.as_uuid()))
            .collect();
        if stored != expected {
            return Err(GraphError::Integrity("participant incidence"));
        }
    }
    if !incidence.is_empty() {
        return Err(GraphError::Integrity("orphan participant rows"));
    }

    let resource_rows = sqlx::query(concat!(
        "SELECT ",
        resource_columns!(),
        " FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2 \
         ORDER BY resource_id"
    ))
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(&mut *connection)
    .await?;
    let mut attached: BTreeMap<Uuid, Vec<TypedRelationInstance>> = BTreeMap::new();
    for relation in &relations {
        let members: BTreeSet<Uuid> = relation
            .participants
            .iter()
            .map(|participant| participant.resource_ref.as_uuid())
            .collect();
        for member in members {
            attached.entry(member).or_default().push(relation.clone());
        }
    }
    let mut resources = Vec::with_capacity(resource_rows.len());
    for row in &resource_rows {
        let resource_id: Uuid = row.try_get("resource_id")?;
        let mut relations_here = attached.remove(&resource_id).unwrap_or_default();
        relations_here.sort_by_key(|relation| relation.relation_id);
        resources.push(resource_from_row(row, relations_here)?);
    }
    Ok((resources, relations))
}

/// Column list decoded by [`resource_from_row`]; instants travel as text.
macro_rules! resource_columns {
    () => {
        "resource_id,resource_kind,resource_version_id, \
     mapping_kind,owner_document_id,folder_id,version_id,adapter_id,native_id, \
     valid_from_nanos::text AS valid_from_nanos,valid_from_offset, \
     valid_to_nanos::text AS valid_to_nanos,valid_to_offset, \
     freshness_anchor_nanos::text AS freshness_anchor_nanos,freshness_anchor_offset, \
     freshness_basis,effective_from_nanos::text AS effective_from_nanos, \
     effective_from_offset,effective_to_nanos::text AS effective_to_nanos,effective_to_offset"
    };
}
pub(crate) use resource_columns;

/// Rebuilds one typed relation from its versioned payload and checks the
/// stored ID and canonical digest.
pub(crate) fn relation_from_row(row: &PgRow) -> Result<TypedRelationInstance, GraphError> {
    let payload: Value = row.try_get("payload")?;
    if payload.get("dto_version").and_then(Value::as_str) != Some(RELATION_DTO_VERSION)
        || payload.as_object().is_none_or(|object| object.len() != 2)
    {
        return Err(GraphError::Integrity("relation payload version"));
    }
    let relation: TypedRelationInstance = serde_json::from_value(
        payload
            .get("relation")
            .cloned()
            .ok_or(GraphError::Integrity("relation payload"))?,
    )
    .map_err(|_| GraphError::Integrity("relation payload"))?;
    if relation.relation_id.as_uuid() != row.try_get::<Uuid, _>("relation_id")?
        || relation_digest(&relation)? != row.try_get::<String, _>("canonical_digest")?
    {
        return Err(GraphError::Integrity("relation digest"));
    }
    Ok(relation)
}

/// Rebuilds one resource record from a [`RESOURCE_COLUMNS`] row.
pub(crate) fn resource_from_row(
    row: &PgRow,
    attached_relations: Vec<TypedRelationInstance>,
) -> Result<GraphResourceRecord, GraphError> {
    let resource_id: Uuid = row.try_get("resource_id")?;
    let mapping = match row.try_get::<String, _>("mapping_kind")?.as_str() {
        "DOCUMENT" => GraphSourceMapping::Document {
            document_id: row
                .try_get::<Option<Uuid>, _>("owner_document_id")?
                .ok_or(GraphError::Integrity("owner"))?,
        },
        "FOLDER_PLACEMENT" => GraphSourceMapping::FolderPlacement {
            document_id: row
                .try_get::<Option<Uuid>, _>("owner_document_id")?
                .ok_or(GraphError::Integrity("owner"))?,
            folder_id: row
                .try_get::<Option<Uuid>, _>("folder_id")?
                .ok_or(GraphError::Integrity("folder"))?,
        },
        "VERSION" => GraphSourceMapping::Version {
            document_id: row
                .try_get::<Option<Uuid>, _>("owner_document_id")?
                .ok_or(GraphError::Integrity("owner"))?,
            version_id: row
                .try_get::<Option<Uuid>, _>("version_id")?
                .ok_or(GraphError::Integrity("version"))?,
        },
        "REGISTERED" => GraphSourceMapping::Registered {
            adapter_id: row
                .try_get::<Option<String>, _>("adapter_id")?
                .ok_or(GraphError::Integrity("adapter"))?,
            native_id: row
                .try_get::<Option<String>, _>("native_id")?
                .ok_or(GraphError::Integrity("native"))?,
        },
        _ => return Err(GraphError::Integrity("mapping kind")),
    };
    let columns = TemporalColumns {
        resource_ref: ResourceId::from_uuid(resource_id),
        valid_from: instant(
            row.try_get("valid_from_nanos")?,
            row.try_get("valid_from_offset")?,
        )?,
        valid_to: instant(
            row.try_get("valid_to_nanos")?,
            row.try_get("valid_to_offset")?,
        )?,
        freshness_anchor_at: instant(
            row.try_get("freshness_anchor_nanos")?,
            row.try_get("freshness_anchor_offset")?,
        )?,
        freshness_basis: row.try_get("freshness_basis")?,
        effective_from: instant(
            row.try_get("effective_from_nanos")?,
            row.try_get("effective_from_offset")?,
        )?,
        effective_to: instant(
            row.try_get("effective_to_nanos")?,
            row.try_get("effective_to_offset")?,
        )?,
    };
    Ok(GraphResourceRecord {
        resource_ref: ResourceId::from_uuid(resource_id),
        kind: kind_from(&row.try_get::<String, _>("resource_kind")?)?,
        resource_version_ref: row
            .try_get::<Option<Uuid>, _>("resource_version_id")?
            .map(ResourceVersionId::from_uuid),
        temporal: decode_temporal(&columns)?,
        mapping,
        attached_relations,
    })
}

/// Digest, mapping and counts recomputed from rows, checked against the parent.
fn recompute(
    key: ProjectionGenerationKey,
    parent: &Parent,
    resources: &[GraphResourceRecord],
    relations: &[TypedRelationInstance],
) -> Result<(String, u64, u64), GraphError> {
    let content = canonical_graph_digest(key.source_id, GRAPH_SCHEMA_VERSION, resources, relations)
        .map_err(|_| GraphError::Integrity("graph content"))?;
    let mapping = canonical_mapping_digest(key.source_id, &parent.source_snapshot, resources)
        .map_err(|_| GraphError::Integrity("source mapping"))?;
    if mapping != parent.source_mapping_digest {
        return Err(GraphError::Integrity("source mapping commitment"));
    }
    let count = |n: usize| u64::try_from(n).map_err(|_| GraphError::Integrity("count"));
    Ok((content, count(resources.len())?, count(relations.len())?))
}

/// P7-08: revalidates the staged generation on the caller's connection, under
/// its parent lock, returning the report and the rebuilt rows.
pub async fn validate_on(
    connection: &mut PgConnection,
    target: &GraphBuildRef,
) -> Result<
    (
        GraphStageReport,
        Vec<GraphResourceRecord>,
        Vec<TypedRelationInstance>,
    ),
    GraphError,
> {
    let key = target.target_key();
    let parent = building_for(parent(&mut *connection, key, "FOR UPDATE").await?, target)?;
    let (resources, relations) = load_rows(&mut *connection, key).await?;
    let (graph_content_digest, resource_count, relation_count) =
        recompute(key, &parent, &resources, &relations)?;
    Ok((
        GraphStageReport {
            key,
            source_snapshot: parent.source_snapshot,
            projection_manifest_digest: parent.projection_manifest_digest,
            source_mapping_digest: parent.source_mapping_digest,
            graph_content_digest,
            resource_count,
            relation_count,
            graph_schema_version: GRAPH_SCHEMA_VERSION.into(),
        },
        resources,
        relations,
    ))
}

/// P7-08: marks the Graph READY with the recomputed digest and counts, on the
/// caller's connection and inside its READY transaction.
pub async fn settle_ready_on(
    connection: &mut PgConnection,
    report: &GraphStageReport,
) -> Result<(), GraphError> {
    let count = |n: u64| i64::try_from(n).map_err(|_| GraphError::Integrity("count"));
    let updated = sqlx::query(
        "UPDATE search_graph.generation SET state='READY', graph_content_digest=$3, \
         resource_count=$4, relation_count=$5, ready_at=clock_timestamp() \
         WHERE source_id=$1 AND generation_id=$2 AND state='BUILDING'",
    )
    .bind(report.key.source_id.as_uuid())
    .bind(report.key.generation_id.as_uuid())
    .bind(&report.graph_content_digest)
    .bind(count(report.resource_count)?)
    .bind(count(report.relation_count)?)
    .execute(connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(GraphError::FenceLost);
    }
    Ok(())
}

/// The PostgreSQL Graph store. Holds only a pool; every reference is checked
/// against stored rows on each use.
#[derive(Clone)]
pub struct PostgresGraphStore {
    pool: PgPool,
}

impl PostgresGraphStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Writes all children of a registered FULL target in one transaction.
    pub async fn stage_full(
        &self,
        target: &RegisteredFullBuildHandle,
        resources: &[GraphResourceRecord],
        relations: &[TypedRelationInstance],
    ) -> Result<GraphStage, GraphError> {
        let key = target.key();
        for relation in relations {
            relation
                .validate()
                .map_err(|_| GraphError::Invalid("relation"))?;
        }
        canonical_graph_digest(key.source_id, GRAPH_SCHEMA_VERSION, resources, relations)?;
        let mut tx = self.pool.begin().await?;
        let parent = building_for(
            parent(&mut tx, key, "FOR UPDATE").await?,
            &GraphBuildRef::Full(*target),
        )?;
        if canonical_mapping_digest(key.source_id, &parent.source_snapshot, resources)?
            != parent.source_mapping_digest
        {
            return Err(GraphError::Integrity("source mapping commitment"));
        }
        insert_rows(&mut tx, key, resources, relations).await?;
        tx.commit().await?;
        Ok(GraphStage { key })
    }

    /// Recomputes the staged generation under its parent lock. No READY.
    pub async fn validate(&self, target: &GraphBuildRef) -> Result<GraphStageReport, GraphError> {
        let key = target.target_key();
        let mut tx = self.pool.begin().await?;
        let parent = building_for(parent(&mut tx, key, "FOR UPDATE").await?, target)?;
        let (resources, relations) = load_rows(&mut tx, key).await?;
        let (graph_content_digest, resource_count, relation_count) =
            recompute(key, &parent, &resources, &relations)?;
        tx.commit().await?;
        Ok(GraphStageReport {
            key,
            source_snapshot: parent.source_snapshot,
            projection_manifest_digest: parent.projection_manifest_digest,
            source_mapping_digest: parent.source_mapping_digest,
            graph_content_digest,
            resource_count,
            relation_count,
            graph_schema_version: GRAPH_SCHEMA_VERSION.into(),
        })
    }

    /// Read-only recovery of a READY generation: every row is rebuilt and the
    /// stored digest, counts and mapping commitment must still hold.
    pub async fn recover(
        &self,
        key: ProjectionGenerationKey,
        expected_manifest_digest: &str,
    ) -> Result<GraphGenerationReceipt, GraphError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let parent = parent(&mut tx, key, "")
            .await?
            .ok_or(GraphError::Integrity("missing generation"))?;
        if parent.state != "READY" || parent.projection_manifest_digest != expected_manifest_digest
        {
            return Err(GraphError::Integrity("not the expected READY generation"));
        }
        let (resources, relations) = load_rows(&mut tx, key).await?;
        let (content, resource_count, relation_count) =
            recompute(key, &parent, &resources, &relations)?;
        if Some(&content) != parent.graph_content_digest.as_ref()
            || parent.resource_count.and_then(|n| u64::try_from(n).ok()) != Some(resource_count)
            || parent.relation_count.and_then(|n| u64::try_from(n).ok()) != Some(relation_count)
        {
            return Err(GraphError::Integrity("READY receipt"));
        }
        tx.commit().await?;
        Ok(GraphGenerationReceipt {
            key,
            projection_manifest_digest: parent.projection_manifest_digest,
            source_snapshot: parent.source_snapshot,
            source_mapping_digest: parent.source_mapping_digest,
            graph_content_digest: content,
            resource_count,
            relation_count,
            graph_schema_version: GRAPH_SCHEMA_VERSION.into(),
        })
    }
}

impl DurableGraphGenerationPort for PostgresGraphStore {
    fn stage_full_registered<'a>(
        &'a self,
        target: &'a RegisteredFullBuildHandle,
        resources: &'a [GraphResourceRecord],
        relations: &'a [TypedRelationInstance],
    ) -> BoxFuture<'a, GraphStage> {
        Box::pin(async move { Ok(self.stage_full(target, resources, relations).await?) })
    }

    fn copy_batch<'a>(
        &'a self,
        _target: &'a BuildGuardHandle,
        _expected: &'a GraphBatchCursor,
        _limit: u32,
    ) -> BoxFuture<'a, GraphBatchCursor> {
        Box::pin(async {
            Err(SearchError::OperationFailed(
                "incremental Graph build is not available".into(),
            ))
        })
    }

    fn verify_copy<'a>(&'a self, _target: &'a BuildGuardHandle) -> BoxFuture<'a, ()> {
        Box::pin(async {
            Err(SearchError::OperationFailed(
                "incremental Graph build is not available".into(),
            ))
        })
    }

    fn apply_delta_batch<'a>(
        &'a self,
        _target: &'a BuildGuardHandle,
        _delta: &'a GraphIncrementalDelta,
        _expected: &'a GraphBatchCursor,
        _limit: u32,
    ) -> BoxFuture<'a, GraphBatchCursor> {
        Box::pin(async {
            Err(SearchError::OperationFailed(
                "incremental Graph build is not available".into(),
            ))
        })
    }

    fn validate_staged<'a>(&'a self, target: &'a GraphBuildRef) -> BoxFuture<'a, GraphStageReport> {
        Box::pin(async move { Ok(self.validate(target).await?) })
    }

    fn recover_ready<'a>(
        &'a self,
        key: &'a ProjectionGenerationKey,
        expected_manifest_digest: &'a str,
    ) -> BoxFuture<'a, GraphGenerationReceipt> {
        Box::pin(async move { Ok(self.recover(*key, expected_manifest_digest).await?) })
    }
}
