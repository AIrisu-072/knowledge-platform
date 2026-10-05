//! P3-G02: one versioned, length-framed canonical encoder for typed n-ary
//! relations and Graph Resources, used by stage, validate and recover.
//!
//! Rules: domain separator `search-graph:typed-nary-v1`; every variable field
//! is length-framed; Optional values carry explicit tags; participant,
//! evidence and `TypedValue::Set` members are sorted, `TypedValue::List` keeps
//! its order; instants are signed epoch nanoseconds plus the offset in seconds,
//! so equal instants in different offsets stay distinct. Generation IDs and
//! build times are excluded. Floating point and TIMESTAMPTZ are never used.

use std::collections::{BTreeMap, BTreeSet};

use search_application::graph_generation::{GraphResourceRecord, GraphSourceMapping};
use search_core::id::{RelationId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use search_core::projection::TemporalProjection;
use search_core::relation::{RelationNamespace, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalDiscoveryProfile;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset};

use crate::GraphError;

/// Schema version recorded with every durable Graph generation.
pub const GRAPH_SCHEMA_VERSION: &str = "search-graph-v1";
const DOMAIN: &[u8] = b"search-graph:typed-nary-v1\0";

fn invalid(reason: &'static str) -> GraphError {
    GraphError::Invalid(reason)
}

/// An instant as stored: epoch nanoseconds and the offset in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    pub epoch_nanos: i128,
    pub offset_seconds: i32,
}

impl Instant {
    pub fn from_datetime(value: OffsetDateTime) -> Self {
        Self {
            epoch_nanos: value.unix_timestamp_nanos(),
            offset_seconds: value.offset().whole_seconds(),
        }
    }

    pub fn to_datetime(self) -> Result<OffsetDateTime, GraphError> {
        let offset = UtcOffset::from_whole_seconds(self.offset_seconds)
            .map_err(|_| invalid("offset out of range"))?;
        Ok(OffsetDateTime::from_unix_timestamp_nanos(self.epoch_nanos)
            .map_err(|_| invalid("instant out of range"))?
            .to_offset(offset))
    }
}

/// The five nullable instants and the nullable freshness basis of one Resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalColumns {
    pub resource_ref: ResourceId,
    pub valid_from: Option<Instant>,
    pub valid_to: Option<Instant>,
    pub freshness_anchor_at: Option<Instant>,
    pub freshness_basis: Option<String>,
    pub effective_from: Option<Instant>,
    pub effective_to: Option<Instant>,
}

fn half_open(from: Option<OffsetDateTime>, to: Option<OffsetDateTime>) -> Result<(), GraphError> {
    if let (Some(from), Some(to)) = (from, to)
        && from >= to
    {
        return Err(invalid("interval start must precede its end"));
    }
    Ok(())
}

pub fn encode_temporal(temporal: &TemporalProjection) -> Result<TemporalColumns, GraphError> {
    half_open(temporal.valid_from, temporal.valid_to)?;
    half_open(
        temporal.profile.effective_from,
        temporal.profile.effective_to,
    )?;
    Ok(TemporalColumns {
        resource_ref: temporal.resource_ref,
        valid_from: temporal.valid_from.map(Instant::from_datetime),
        valid_to: temporal.valid_to.map(Instant::from_datetime),
        freshness_anchor_at: temporal
            .profile
            .freshness_anchor_at
            .map(Instant::from_datetime),
        freshness_basis: temporal.profile.freshness_basis.clone(),
        effective_from: temporal.profile.effective_from.map(Instant::from_datetime),
        effective_to: temporal.profile.effective_to.map(Instant::from_datetime),
    })
}

pub fn decode_temporal(columns: &TemporalColumns) -> Result<TemporalProjection, GraphError> {
    let at = |value: Option<Instant>| value.map(Instant::to_datetime).transpose();
    let temporal = TemporalProjection {
        resource_ref: columns.resource_ref,
        valid_from: at(columns.valid_from)?,
        valid_to: at(columns.valid_to)?,
        profile: TemporalDiscoveryProfile {
            freshness_anchor_at: at(columns.freshness_anchor_at)?,
            freshness_basis: columns.freshness_basis.clone(),
            effective_from: at(columns.effective_from)?,
            effective_to: at(columns.effective_to)?,
        },
    };
    half_open(temporal.valid_from, temporal.valid_to)?;
    half_open(
        temporal.profile.effective_from,
        temporal.profile.effective_to,
    )?;
    Ok(temporal)
}

#[derive(Default)]
struct Canonical(Vec<u8>);

impl Canonical {
    fn tag(&mut self, tag: u8) {
        self.0.push(tag);
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn i32(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn i128(&mut self, value: i128) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }
    fn count(&mut self, count: usize) -> Result<(), GraphError> {
        self.u32(u32::try_from(count).map_err(|_| invalid("collection too large"))?);
        Ok(())
    }
    fn frame(&mut self, bytes: &[u8]) -> Result<(), GraphError> {
        self.count(bytes.len())?;
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), GraphError> {
        self.frame(text.as_bytes())
    }
    fn uuid(&mut self, value: uuid::Uuid) {
        self.0.extend_from_slice(value.as_bytes());
    }
    fn optional_text(&mut self, value: Option<&str>) -> Result<(), GraphError> {
        match value {
            None => self.tag(0),
            Some(text) => {
                self.tag(1);
                self.text(text)?;
            }
        }
        Ok(())
    }
    fn instant(&mut self, value: Option<OffsetDateTime>) {
        match value {
            None => self.tag(0),
            Some(value) => {
                let instant = Instant::from_datetime(value);
                self.tag(1);
                self.i128(instant.epoch_nanos);
                self.i32(instant.offset_seconds);
            }
        }
    }
    /// Sorted, duplicate-free members, each length-framed.
    fn set(&mut self, members: Vec<Vec<u8>>, what: &'static str) -> Result<(), GraphError> {
        let unique: BTreeSet<Vec<u8>> = members.iter().cloned().collect();
        if unique.len() != members.len() {
            return Err(invalid(what));
        }
        self.count(unique.len())?;
        for member in unique {
            self.frame(&member)?;
        }
        Ok(())
    }
}

fn namespace_tag(namespace: RelationNamespace) -> u8 {
    match namespace {
        RelationNamespace::Discovery => 1,
        RelationNamespace::Semantic => 2,
        RelationNamespace::Evidence => 3,
    }
}

pub(crate) fn kind_tag(kind: ResourceKind) -> u8 {
    match kind {
        ResourceKind::Knowledge => 1,
        ResourceKind::Document => 2,
        ResourceKind::FolderPlacement => 3,
        ResourceKind::Semantic => 4,
        ResourceKind::Capability => 5,
        ResourceKind::AgentSkill => 6,
        ResourceKind::Workflow => 7,
        ResourceKind::Policy => 8,
    }
}

fn typed_value(value: &TypedValue) -> Result<Vec<u8>, GraphError> {
    let mut out = Canonical::default();
    match value {
        TypedValue::Bool(value) => {
            out.tag(1);
            out.tag(u8::from(*value));
        }
        TypedValue::String(value) => {
            out.tag(2);
            out.text(value)?;
        }
        TypedValue::Integer(value) => {
            out.tag(3);
            out.i128(*value);
        }
        TypedValue::Decimal(value) => {
            out.tag(4);
            out.i128(value.coefficient);
            out.tag(value.scale);
        }
        TypedValue::Date(value) => {
            out.tag(5);
            out.i32(value.to_julian_day());
        }
        TypedValue::DateTime(value) => {
            out.tag(6);
            out.instant(Some(*value));
        }
        TypedValue::Duration(value) => {
            out.tag(7);
            out.i128(value.whole_nanoseconds());
        }
        TypedValue::ConceptRef(value) => {
            out.tag(8);
            out.text(value)?;
        }
        TypedValue::ResourceRef(value) => {
            out.tag(9);
            out.uuid(value.as_uuid());
        }
        TypedValue::List(items) => {
            out.tag(10);
            out.count(items.len())?;
            for item in items {
                out.frame(&typed_value(item)?)?;
            }
        }
        TypedValue::Set(items) => {
            out.tag(11);
            out.set(
                items.iter().map(typed_value).collect::<Result<_, _>>()?,
                "duplicate set member",
            )?;
        }
        TypedValue::Money(value) => {
            out.tag(12);
            out.i128(value.amount_minor);
            out.text(&value.currency)?;
        }
        TypedValue::Quantity(value) => {
            out.tag(13);
            out.i128(value.amount.coefficient);
            out.tag(value.amount.scale);
            out.text(&value.unit)?;
        }
    }
    Ok(out.0)
}

/// Canonical bytes of one n-ary relation. Participant `(role, resource)` pairs
/// and evidence refs are sets; qualifiers are keyed; the temporal scope is
/// half-open.
pub fn canonical_relation(relation: &TypedRelationInstance) -> Result<Vec<u8>, GraphError> {
    half_open(
        relation.temporal_scope.valid_from,
        relation.temporal_scope.valid_to,
    )?;
    if relation.relation_type.trim().is_empty() || relation.participants.is_empty() {
        return Err(invalid("relation type and participants are required"));
    }
    let mut out = Canonical::default();
    out.uuid(relation.relation_id.as_uuid());
    out.tag(namespace_tag(relation.namespace));
    out.text(&relation.relation_type)?;
    let mut participants = Vec::with_capacity(relation.participants.len());
    for participant in &relation.participants {
        if participant.role.trim().is_empty() {
            return Err(invalid("participant role is required"));
        }
        let mut member = Canonical::default();
        member.text(&participant.role)?;
        member.uuid(participant.resource_ref.as_uuid());
        participants.push(member.0);
    }
    out.set(participants, "duplicate participant role and resource")?;
    out.count(relation.qualifiers.len())?;
    for (key, value) in &relation.qualifiers {
        out.text(key)?;
        out.frame(&typed_value(value)?)?;
    }
    out.instant(relation.temporal_scope.valid_from);
    out.instant(relation.temporal_scope.valid_to);
    out.optional_text(relation.authority.as_deref())?;
    out.optional_text(relation.provenance.as_deref())?;
    out.set(
        relation
            .evidence_refs
            .iter()
            .map(|reference| reference.as_bytes().to_vec())
            .collect(),
        "duplicate evidence ref",
    )?;
    Ok(out.0)
}

fn mapping(out: &mut Canonical, mapping: &GraphSourceMapping) -> Result<(), GraphError> {
    match mapping {
        GraphSourceMapping::Document { document_id } => {
            out.tag(1);
            out.uuid(*document_id);
        }
        GraphSourceMapping::FolderPlacement {
            document_id,
            folder_id,
        } => {
            out.tag(2);
            out.uuid(*document_id);
            out.uuid(*folder_id);
        }
        GraphSourceMapping::Version {
            document_id,
            version_id,
        } => {
            out.tag(3);
            out.uuid(*document_id);
            out.uuid(*version_id);
        }
        GraphSourceMapping::Registered {
            adapter_id,
            native_id,
        } => {
            if adapter_id.trim().is_empty() || native_id.trim().is_empty() {
                return Err(invalid("registered mapping identifiers are required"));
            }
            out.tag(4);
            out.text(adapter_id)?;
            out.text(native_id)?;
        }
    }
    Ok(())
}

fn canonical_resource(record: &GraphResourceRecord) -> Result<Vec<u8>, GraphError> {
    if record.temporal.resource_ref != record.resource_ref {
        return Err(invalid("temporal projection belongs to another Resource"));
    }
    let columns = encode_temporal(&record.temporal)?;
    let mut out = Canonical::default();
    out.uuid(record.resource_ref.as_uuid());
    out.tag(kind_tag(record.kind));
    match record.resource_version_ref {
        None => out.tag(0),
        Some(version) => {
            out.tag(1);
            out.uuid(version.as_uuid());
        }
    }
    for instant in [
        columns.valid_from,
        columns.valid_to,
        columns.freshness_anchor_at,
    ] {
        out.instant(instant.map(Instant::to_datetime).transpose()?);
    }
    out.optional_text(columns.freshness_basis.as_deref())?;
    for instant in [columns.effective_from, columns.effective_to] {
        out.instant(instant.map(Instant::to_datetime).transpose()?);
    }
    mapping(&mut out, &record.mapping)?;
    let attached: Vec<Vec<u8>> = record
        .attached_relations
        .iter()
        .map(|relation| relation.relation_id.as_uuid().as_bytes().to_vec())
        .collect();
    out.set(attached, "relation attached twice to one Resource")?;
    Ok(out.0)
}

/// Every attachment of a relation must be byte-identical to the one canonical
/// relation, and each participant Resource present in the generation must
/// carry it.
fn validate_attachments(
    resources: &[GraphResourceRecord],
    relations: &BTreeMap<RelationId, Vec<u8>>,
    participants: &BTreeMap<RelationId, BTreeSet<ResourceId>>,
) -> Result<(), GraphError> {
    let present: BTreeSet<ResourceId> =
        resources.iter().map(|record| record.resource_ref).collect();
    for record in resources {
        for relation in &record.attached_relations {
            let Some(canonical) = relations.get(&relation.relation_id) else {
                return Err(invalid("attached relation is missing from the generation"));
            };
            if &canonical_relation(relation)? != canonical
                || !relation
                    .participants
                    .iter()
                    .any(|participant| participant.resource_ref == record.resource_ref)
            {
                return Err(invalid("attachment differs from the canonical relation"));
            }
        }
    }
    for (relation_id, members) in participants {
        for resource in members.intersection(&present) {
            let record = resources
                .iter()
                .find(|record| record.resource_ref == *resource)
                .ok_or_else(|| invalid("participant Resource missing"))?;
            if !record
                .attached_relations
                .iter()
                .any(|relation| relation.relation_id == *relation_id)
            {
                return Err(invalid(
                    "participant Resource lacks its relation attachment",
                ));
            }
        }
    }
    Ok(())
}

fn sha256_text(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// `sha256:` digest of the canonical relation bytes stored with each row.
pub fn relation_digest(relation: &TypedRelationInstance) -> Result<String, GraphError> {
    Ok(sha256_text(
        b"search-graph:relation-v1\0",
        &canonical_relation(relation)?,
    ))
}

/// `sha256:` Source mapping commitment of one snapshot: every Resource's kind,
/// Version reference and Source-native mapping, in Resource order. The Source
/// adapter issues it at registration; stage and recover recompute it from rows.
pub fn canonical_mapping_digest(
    source: SourceId,
    source_snapshot: &str,
    resources: &[GraphResourceRecord],
) -> Result<String, GraphError> {
    let mut ordered: Vec<&GraphResourceRecord> = resources.iter().collect();
    ordered.sort_by_key(|record| record.resource_ref);
    if ordered
        .windows(2)
        .any(|pair| pair[0].resource_ref == pair[1].resource_ref)
    {
        return Err(invalid("duplicate Resource"));
    }
    let mut out = Canonical::default();
    out.uuid(source.as_uuid());
    out.text(source_snapshot)?;
    out.count(ordered.len())?;
    for record in ordered {
        out.uuid(record.resource_ref.as_uuid());
        out.tag(kind_tag(record.kind));
        match record.resource_version_ref {
            None => out.tag(0),
            Some(version) => {
                out.tag(1);
                out.uuid(version.as_uuid());
            }
        }
        mapping(&mut out, &record.mapping)?;
    }
    Ok(sha256_text(b"search-graph:source-mapping-v1\0", &out.0))
}

/// `sha256:` digest of one Source's Graph content: schema, every Resource and
/// every relation, in key order. Generation identity is excluded.
pub fn canonical_graph_digest(
    source: SourceId,
    schema: &str,
    resources: &[GraphResourceRecord],
    relations: &[TypedRelationInstance],
) -> Result<String, GraphError> {
    let mut canonical_relations = BTreeMap::new();
    let mut participants = BTreeMap::new();
    for relation in relations {
        let bytes = canonical_relation(relation)?;
        if canonical_relations
            .insert(relation.relation_id, bytes)
            .is_some()
        {
            return Err(invalid("duplicate relation ID"));
        }
        participants.insert(
            relation.relation_id,
            relation
                .participants
                .iter()
                .map(|participant| participant.resource_ref)
                .collect::<BTreeSet<_>>(),
        );
    }
    let mut canonical_resources = BTreeMap::new();
    for record in resources {
        if canonical_resources
            .insert(record.resource_ref, canonical_resource(record)?)
            .is_some()
        {
            return Err(invalid("duplicate Resource"));
        }
    }
    validate_attachments(resources, &canonical_relations, &participants)?;
    let mut out = Canonical::default();
    out.uuid(source.as_uuid());
    out.text(schema)?;
    out.count(canonical_resources.len())?;
    for bytes in canonical_resources.values() {
        out.frame(bytes)?;
    }
    out.count(canonical_relations.len())?;
    for bytes in canonical_relations.values() {
        out.frame(bytes)?;
    }
    Ok(sha256_text(DOMAIN, &out.0))
}
