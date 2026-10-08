//! Stage 4 (SD-T11 5): Graph rows as content-addressed segments. A segment
//! holds one document's Resource rows and the relations among them; Resources
//! owned by no single document and relations across documents form one
//! remainder segment. A generation lists its segments in order, and segments
//! are shared between generations. The logical Graph digest is unchanged: it
//! is computed from the assembled records exactly as from the row tables.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use search_application::graph_generation::{GraphResourceRecord, GraphSourceMapping};
use search_core::id::{ResourceId, ResourceVersionId, SourceId};
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::TypedRelationInstance;
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::GraphError;
use crate::canonical::{
    Instant, TemporalColumns, decode_temporal, encode_temporal, segment_digest,
};
use crate::store::{kind_from, kind_text, load_rows};

const SEGMENT_DTO_VERSION: &str = "v1";
/// Verified segments kept per process; above this only the segments of the
/// generation being read are kept.
const SEGMENT_CACHE_ITEMS: usize = 200_000;
/// Segments read per query.
const SEGMENT_FETCH_BATCH: usize = 1_000;

fn integrity(what: &'static str) -> GraphError {
    GraphError::Integrity(what)
}

/// One verified segment: Resource rows without attachments and relations.
pub(crate) struct Segment {
    resources: Vec<GraphResourceRecord>,
    relations: Vec<TypedRelationInstance>,
}

/// One segment ready to store.
pub(crate) struct BuiltSegment {
    pub(crate) digest: String,
    pub(crate) resource_count: i32,
    pub(crate) relation_count: i32,
    pub(crate) payload: Value,
}

fn instant_json(value: Option<Instant>) -> Value {
    match value {
        None => Value::Null,
        Some(instant) => json!([instant.epoch_nanos.to_string(), instant.offset_seconds]),
    }
}

fn instant_from(value: &Value) -> Result<Option<Instant>, GraphError> {
    match value {
        Value::Null => Ok(None),
        Value::Array(pair) if pair.len() == 2 => Ok(Some(Instant {
            epoch_nanos: pair[0]
                .as_str()
                .and_then(|nanos| nanos.parse().ok())
                .ok_or_else(|| integrity("segment instant"))?,
            offset_seconds: pair[1]
                .as_i64()
                .and_then(|offset| i32::try_from(offset).ok())
                .ok_or_else(|| integrity("segment instant"))?,
        })),
        _ => Err(integrity("segment instant")),
    }
}

fn resource_json(record: &GraphResourceRecord) -> Result<Value, GraphError> {
    let columns = encode_temporal(&record.temporal)?;
    let mapping = match &record.mapping {
        GraphSourceMapping::Document { document_id } => {
            json!({"kind": "DOCUMENT", "document_id": document_id})
        }
        GraphSourceMapping::FolderPlacement {
            document_id,
            folder_id,
        } => json!({"kind": "FOLDER_PLACEMENT", "document_id": document_id,
                    "folder_id": folder_id}),
        GraphSourceMapping::Version {
            document_id,
            version_id,
        } => json!({"kind": "VERSION", "document_id": document_id, "version_id": version_id}),
        GraphSourceMapping::Registered {
            adapter_id,
            native_id,
        } => json!({"kind": "REGISTERED", "adapter_id": adapter_id, "native_id": native_id}),
    };
    Ok(json!({
        "id": record.resource_ref.as_uuid(),
        "kind": kind_text(record.kind),
        "version": record.resource_version_ref.map(|version| version.as_uuid()),
        "mapping": mapping,
        "valid_from": instant_json(columns.valid_from),
        "valid_to": instant_json(columns.valid_to),
        "freshness_anchor": instant_json(columns.freshness_anchor_at),
        "freshness_basis": columns.freshness_basis,
        "effective_from": instant_json(columns.effective_from),
        "effective_to": instant_json(columns.effective_to),
    }))
}

fn uuid_at(value: &Value, field: &str) -> Result<Uuid, GraphError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(|text| Uuid::parse_str(text).ok())
        .ok_or_else(|| integrity("segment identifier"))
}

fn text_at(value: &Value, field: &str) -> Result<String, GraphError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| integrity("segment text"))
}

fn resource_from_json(value: &Value) -> Result<GraphResourceRecord, GraphError> {
    let object = value
        .as_object()
        .ok_or_else(|| integrity("segment Resource"))?;
    if object.len() != 10 {
        return Err(integrity("segment Resource fields"));
    }
    let resource_ref = ResourceId::from_uuid(uuid_at(value, "id")?);
    let mapping_value = &value["mapping"];
    let mapping = match mapping_value.get("kind").and_then(Value::as_str) {
        Some("DOCUMENT") => GraphSourceMapping::Document {
            document_id: uuid_at(mapping_value, "document_id")?,
        },
        Some("FOLDER_PLACEMENT") => GraphSourceMapping::FolderPlacement {
            document_id: uuid_at(mapping_value, "document_id")?,
            folder_id: uuid_at(mapping_value, "folder_id")?,
        },
        Some("VERSION") => GraphSourceMapping::Version {
            document_id: uuid_at(mapping_value, "document_id")?,
            version_id: uuid_at(mapping_value, "version_id")?,
        },
        Some("REGISTERED") => GraphSourceMapping::Registered {
            adapter_id: text_at(mapping_value, "adapter_id")?,
            native_id: text_at(mapping_value, "native_id")?,
        },
        _ => return Err(integrity("segment mapping kind")),
    };
    let version = match &value["version"] {
        Value::Null => None,
        _ => Some(ResourceVersionId::from_uuid(uuid_at(value, "version")?)),
    };
    let freshness_basis = match &value["freshness_basis"] {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        _ => return Err(integrity("segment freshness basis")),
    };
    let columns = TemporalColumns {
        resource_ref,
        valid_from: instant_from(&value["valid_from"])?,
        valid_to: instant_from(&value["valid_to"])?,
        freshness_anchor_at: instant_from(&value["freshness_anchor"])?,
        freshness_basis,
        effective_from: instant_from(&value["effective_from"])?,
        effective_to: instant_from(&value["effective_to"])?,
    };
    Ok(GraphResourceRecord {
        resource_ref,
        kind: kind_from(
            value["kind"]
                .as_str()
                .ok_or_else(|| integrity("segment Resource kind"))?,
        )?,
        resource_version_ref: version,
        temporal: decode_temporal(&columns)?,
        mapping,
        attached_relations: Vec::new(),
    })
}

fn owner(record: &GraphResourceRecord) -> Option<Uuid> {
    match &record.mapping {
        GraphSourceMapping::Document { document_id }
        | GraphSourceMapping::FolderPlacement { document_id, .. }
        | GraphSourceMapping::Version { document_id, .. } => Some(*document_id),
        GraphSourceMapping::Registered { .. } => None,
    }
}

/// Splits a whole Graph into its segments, in list order: documents by ID,
/// then the remainder.
pub(crate) fn partition(
    source: SourceId,
    resources: &[GraphResourceRecord],
    relations: &[TypedRelationInstance],
) -> Result<Vec<BuiltSegment>, GraphError> {
    // (0, document) for a document segment, (1, nil) for the remainder.
    type Key = (u8, Uuid);
    let remainder: Key = (1, Uuid::nil());
    let mut groups: BTreeMap<Key, (Vec<GraphResourceRecord>, Vec<TypedRelationInstance>)> =
        BTreeMap::new();
    let mut owner_of: HashMap<ResourceId, Key> = HashMap::with_capacity(resources.len());
    for record in resources {
        let key = owner(record).map_or(remainder, |document| (0, document));
        owner_of.insert(record.resource_ref, key);
        groups.entry(key).or_default().0.push(GraphResourceRecord {
            attached_relations: Vec::new(),
            ..record.clone()
        });
    }
    for relation in relations {
        let keys: BTreeSet<Option<&Key>> = relation
            .participants
            .iter()
            .map(|participant| owner_of.get(&participant.resource_ref))
            .collect();
        let key = match keys.into_iter().collect::<Vec<_>>().as_slice() {
            [Some(key)] => **key,
            _ => remainder,
        };
        groups.entry(key).or_default().1.push(relation.clone());
    }
    groups
        .into_values()
        .map(|(mut resources, mut relations)| {
            resources.sort_by_key(|record| record.resource_ref);
            relations.sort_by_key(|relation| relation.relation_id);
            let digest = segment_digest(source, &resources, &relations)?;
            let payload = json!({
                "dto_version": SEGMENT_DTO_VERSION,
                "source_id": source.as_uuid(),
                "resources": resources.iter().map(resource_json).collect::<Result<Vec<_>, _>>()?,
                "relations": relations,
            });
            Ok(BuiltSegment {
                digest,
                resource_count: i32::try_from(resources.len())
                    .map_err(|_| GraphError::Invalid("segment size"))?,
                relation_count: i32::try_from(relations.len())
                    .map_err(|_| GraphError::Invalid("segment size"))?,
                payload,
            })
        })
        .collect()
}

/// Decodes a stored segment and checks it against its digest and counts.
fn verify(
    digest: &str,
    source: SourceId,
    resource_count: i32,
    relation_count: i32,
    payload: &Value,
) -> Result<Segment, GraphError> {
    if payload.get("dto_version").and_then(Value::as_str) != Some(SEGMENT_DTO_VERSION)
        || payload.as_object().is_none_or(|object| object.len() != 4)
        || uuid_at(payload, "source_id")? != source.as_uuid()
    {
        return Err(integrity("segment payload"));
    }
    let resources = payload["resources"]
        .as_array()
        .ok_or_else(|| integrity("segment Resources"))?
        .iter()
        .map(resource_from_json)
        .collect::<Result<Vec<_>, _>>()?;
    let relations: Vec<TypedRelationInstance> =
        serde_json::from_value(payload["relations"].clone())
            .map_err(|_| integrity("segment relations"))?;
    for relation in &relations {
        relation
            .validate()
            .map_err(|_| integrity("segment relation"))?;
    }
    if usize::try_from(resource_count).ok() != Some(resources.len())
        || usize::try_from(relation_count).ok() != Some(relations.len())
        || segment_digest(source, &resources, &relations)? != digest
    {
        return Err(integrity("segment digest"));
    }
    Ok(Segment {
        resources,
        relations,
    })
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Segment>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Segment>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Forgets every verified segment, as a restarted process would.
pub fn forget_verified_segments() {
    if let Ok(mut cache) = cache().lock() {
        cache.clear();
    }
}

/// The ordered segment list of a generation; empty for one stored in rows.
pub(crate) async fn segment_list(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
) -> Result<Vec<String>, GraphError> {
    let rows: Vec<(i32, String)> = sqlx::query_as(
        "SELECT ordinal, segment_digest FROM search_graph.generation_segment \
         WHERE source_id=$1 AND generation_id=$2 ORDER BY ordinal",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(&mut *connection)
    .await?;
    if rows
        .iter()
        .enumerate()
        .any(|(at, (ordinal, _))| usize::try_from(*ordinal).ok() != Some(at))
    {
        return Err(integrity("segment list order"));
    }
    Ok(rows.into_iter().map(|(_, digest)| digest).collect())
}

/// The listed segments, each verified when this process first reads it.
async fn segments(
    connection: &mut PgConnection,
    source: SourceId,
    list: &[String],
) -> Result<Vec<Arc<Segment>>, GraphError> {
    let missing: Vec<String> = {
        let cache = cache().lock().map_err(|_| GraphError::Store)?;
        let mut missing: Vec<String> = list
            .iter()
            .filter(|digest| !cache.contains_key(*digest))
            .cloned()
            .collect();
        missing.sort();
        missing.dedup();
        missing
    };
    let mut fetched = HashMap::with_capacity(missing.len());
    for batch in missing.chunks(SEGMENT_FETCH_BATCH) {
        let rows: Vec<(String, Uuid, i32, i32, Value)> = sqlx::query_as(
            "SELECT segment_digest, source_id, resource_count, relation_count, payload \
             FROM search_graph.segment WHERE segment_digest = ANY($1)",
        )
        .bind(batch)
        .fetch_all(&mut *connection)
        .await?;
        if rows.len() != batch.len() {
            return Err(integrity("missing segment"));
        }
        for (digest, row_source, resources, relations, payload) in rows {
            if row_source != source.as_uuid() {
                return Err(integrity("segment Source"));
            }
            let segment = verify(&digest, source, resources, relations, &payload)?;
            fetched.insert(digest, Arc::new(segment));
        }
    }
    let mut cache = cache().lock().map_err(|_| GraphError::Store)?;
    cache.extend(fetched);
    let out = list
        .iter()
        .map(|digest| cache.get(digest).cloned().ok_or(GraphError::Store))
        .collect::<Result<Vec<_>, _>>()?;
    if cache.len() > SEGMENT_CACHE_ITEMS {
        let listed: BTreeSet<&String> = list.iter().collect();
        cache.retain(|digest, _| listed.contains(digest));
    }
    Ok(out)
}

/// Every Resource with its attached relations and every relation, in key
/// order, as [`load_rows`] returns them.
fn assemble(
    segments: &[Arc<Segment>],
) -> Result<(Vec<GraphResourceRecord>, Vec<TypedRelationInstance>), GraphError> {
    let mut relations: BTreeMap<_, &TypedRelationInstance> = BTreeMap::new();
    for relation in segments.iter().flat_map(|segment| &segment.relations) {
        if relations.insert(relation.relation_id, relation).is_some() {
            return Err(integrity("relation in two segments"));
        }
    }
    let mut attached: HashMap<ResourceId, Vec<TypedRelationInstance>> = HashMap::new();
    for relation in relations.values() {
        let members: BTreeSet<ResourceId> = relation
            .participants
            .iter()
            .map(|participant| participant.resource_ref)
            .collect();
        for member in members {
            attached
                .entry(member)
                .or_default()
                .push((*relation).clone());
        }
    }
    let mut resources: BTreeMap<ResourceId, GraphResourceRecord> = BTreeMap::new();
    for record in segments.iter().flat_map(|segment| &segment.resources) {
        let mut record = record.clone();
        record.attached_relations = attached.remove(&record.resource_ref).unwrap_or_default();
        record
            .attached_relations
            .sort_by_key(|relation| relation.relation_id);
        if resources.insert(record.resource_ref, record).is_some() {
            return Err(integrity("Resource in two segments"));
        }
    }
    Ok((
        resources.into_values().collect(),
        relations.into_values().cloned().collect(),
    ))
}

/// Every Resource and relation of a generation: from its segments when it
/// lists any, otherwise from the row tables. A generation with both is an
/// integrity failure.
pub(crate) async fn load_generation(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
) -> Result<(Vec<GraphResourceRecord>, Vec<TypedRelationInstance>), GraphError> {
    let list = segment_list(&mut *connection, key).await?;
    if list.is_empty() {
        return load_rows(connection, key).await;
    }
    let rows: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM search_graph.resource \
         WHERE source_id=$1 AND generation_id=$2) \
         OR EXISTS (SELECT 1 FROM search_graph.relation \
         WHERE source_id=$1 AND generation_id=$2)",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_one(&mut *connection)
    .await?;
    if rows {
        return Err(integrity("generation has both rows and segments"));
    }
    let segments = segments(connection, key.source_id, &list).await?;
    assemble(&segments)
}

/// Writes the segments that are not stored yet and the generation's list.
/// The list rows are generation children, so the parent's guard applies.
pub(crate) async fn write_segments(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    built: &[BuiltSegment],
) -> Result<(), GraphError> {
    let digests: Vec<String> = built.iter().map(|segment| segment.digest.clone()).collect();
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT segment_digest FROM search_graph.segment WHERE segment_digest = ANY($1)",
    )
    .bind(&digests)
    .fetch_all(&mut *connection)
    .await?;
    let existing: BTreeSet<String> = existing.into_iter().collect();
    let new: Vec<&BuiltSegment> = built
        .iter()
        .filter(|segment| !existing.contains(&segment.digest))
        .collect();
    for batch in new.chunks(SEGMENT_FETCH_BATCH) {
        sqlx::query(
            "INSERT INTO search_graph.segment (segment_digest,source_id,resource_count, \
             relation_count,payload) SELECT d, $1, r, l, p \
             FROM UNNEST($2::text[], $3::int4[], $4::int4[], $5::jsonb[]) AS t(d, r, l, p) \
             ON CONFLICT (segment_digest) DO NOTHING",
        )
        .bind(key.source_id.as_uuid())
        .bind(batch.iter().map(|s| s.digest.clone()).collect::<Vec<_>>())
        .bind(batch.iter().map(|s| s.resource_count).collect::<Vec<_>>())
        .bind(batch.iter().map(|s| s.relation_count).collect::<Vec<_>>())
        .bind(batch.iter().map(|s| s.payload.clone()).collect::<Vec<_>>())
        .execute(&mut *connection)
        .await?;
    }
    let ordinals: Vec<i32> = (0..built.len())
        .map(|ordinal| i32::try_from(ordinal).map_err(|_| GraphError::Invalid("ordinal")))
        .collect::<Result<_, _>>()?;
    sqlx::query(
        "INSERT INTO search_graph.generation_segment (source_id,generation_id,ordinal, \
         segment_digest) SELECT $1, $2, o, d FROM UNNEST($3::int4[], $4::text[]) AS t(o, d)",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(&ordinals)
    .bind(&digests)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Deletes segments no list names, older than ten minutes: a build may have
/// written them and not yet committed its list.
pub async fn collect_unlisted(pool: &sqlx::PgPool) -> Result<u64, GraphError> {
    Ok(sqlx::query(
        "DELETE FROM search_graph.segment s \
         WHERE s.created_at < clock_timestamp() - interval '10 minutes' \
         AND NOT EXISTS (SELECT 1 FROM search_graph.generation_segment g \
         WHERE g.segment_digest = s.segment_digest)",
    )
    .execute(pool)
    .await?
    .rows_affected())
}

/// Resource rows of the segmented READY generations read last, by Resource.
type ResourceIndex = Arc<HashMap<ResourceId, GraphResourceRecord>>;

fn resource_indexes() -> &'static Mutex<Vec<(ProjectionGenerationKey, ResourceIndex)>> {
    static INDEXES: OnceLock<Mutex<Vec<(ProjectionGenerationKey, ResourceIndex)>>> =
        OnceLock::new();
    INDEXES.get_or_init(Default::default)
}

/// Generations whose Resource index is kept per process.
const RESOURCE_INDEXES: usize = 2;

/// For a segmented generation, `Some` with the READY Resource row (without
/// attachments) if it exists; `None` when the generation lists no segments.
pub(crate) async fn ready_segment_resource(
    pool: &sqlx::PgPool,
    key: ProjectionGenerationKey,
    resource_ref: ResourceId,
) -> Result<Option<Option<GraphResourceRecord>>, GraphError> {
    let cached = resource_indexes()
        .lock()
        .map_err(|_| GraphError::Store)?
        .iter()
        .find(|(at, _)| *at == key)
        .map(|(_, index)| index.clone());
    let index = match cached {
        Some(index) => index,
        None => {
            let mut connection = pool.acquire().await?;
            let state: Option<String> = sqlx::query_scalar(
                "SELECT state FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2",
            )
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .fetch_optional(&mut *connection)
            .await?;
            let list = segment_list(&mut connection, key).await?;
            if list.is_empty() {
                return Ok(None);
            }
            if state.as_deref() != Some("READY") {
                return Ok(Some(None));
            }
            let segments = segments(&mut connection, key.source_id, &list).await?;
            let index: ResourceIndex = Arc::new(
                segments
                    .iter()
                    .flat_map(|segment| &segment.resources)
                    .map(|record| (record.resource_ref, record.clone()))
                    .collect(),
            );
            let mut indexes = resource_indexes().lock().map_err(|_| GraphError::Store)?;
            indexes.push((key, index.clone()));
            if indexes.len() > RESOURCE_INDEXES {
                indexes.remove(0);
            }
            index
        }
    };
    Ok(Some(index.get(&resource_ref).cloned()))
}
