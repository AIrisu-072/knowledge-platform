//! Closed request DTOs and the private allow-list mapping to public JSON.
//!
//! Requests deny unknown fields; nothing in them names an actor, tenant,
//! selector, route, provider URL or access context. Responses are built
//! field by field from the application's allow-listed views inside the
//! disclosure gate; no core type is serialized directly.

use search_application::api_cursor::CursorHandle;
use search_application::discover_route::{
    DiscoverGraphInput, DiscoverInput, MAX_GRAPH_HOPS, MAX_GRAPH_SEEDS, graph_token_ok,
};
use search_application::discovery_service::MatchedField;
use search_application::public_projection::{
    Completeness, CompletenessReason, DiscoveryEvaluationView, PublicApplicability,
    PublicClaimState, PublicEvidenceRole, PublicEvidenceValue, PublicSufficiency, TraceOutcome,
    TraceStage,
};
use search_application::resource_read::{ResourceCoverage, ResourceView};
use search_application::search_query::{
    PublicGapCode, PublicGapView, SearchCoverage, SearchInput, SearchResultView,
};
use search_application::source_browse::{SourceCoverageKind, SourceView};
use search_application::source_registration::SourceKind;
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::limits::DEFAULT_PAGE_SIZE;
use crate::problem::FieldError;

const MAX_QUERY_BYTES: usize = 2_048;
const MAX_PURPOSE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceTypeDto {
    Knowledge,
    Document,
    FolderPlacement,
    Semantic,
    Capability,
    AgentSkill,
    Workflow,
    Policy,
}

impl From<ResourceTypeDto> for ResourceKind {
    fn from(value: ResourceTypeDto) -> Self {
        match value {
            ResourceTypeDto::Knowledge => Self::Knowledge,
            ResourceTypeDto::Document => Self::Document,
            ResourceTypeDto::FolderPlacement => Self::FolderPlacement,
            ResourceTypeDto::Semantic => Self::Semantic,
            ResourceTypeDto::Capability => Self::Capability,
            ResourceTypeDto::AgentSkill => Self::AgentSkill,
            ResourceTypeDto::Workflow => Self::Workflow,
            ResourceTypeDto::Policy => Self::Policy,
        }
    }
}

fn resource_type(kind: ResourceKind) -> ResourceTypeDto {
    match kind {
        ResourceKind::Knowledge => ResourceTypeDto::Knowledge,
        ResourceKind::Document => ResourceTypeDto::Document,
        ResourceKind::FolderPlacement => ResourceTypeDto::FolderPlacement,
        ResourceKind::Semantic => ResourceTypeDto::Semantic,
        ResourceKind::Capability => ResourceTypeDto::Capability,
        ResourceKind::AgentSkill => ResourceTypeDto::AgentSkill,
        ResourceKind::Workflow => ResourceTypeDto::Workflow,
        ResourceKind::Policy => ResourceTypeDto::Policy,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestCoverageDto {
    TitleAndPermittedMetadata,
    BodyRequired,
}

impl From<RequestCoverageDto> for SearchCoverage {
    fn from(value: RequestCoverageDto) -> Self {
        match value {
            RequestCoverageDto::TitleAndPermittedMetadata => Self::TitleAndPermittedMetadata,
            RequestCoverageDto::BodyRequired => Self::BodyRequired,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchQueryDto {
    query: String,
    #[serde(default)]
    resource_types: Option<Vec<ResourceTypeDto>>,
    #[serde(default)]
    source_ids: Option<Vec<Uuid>>,
    coverage: RequestCoverageDto,
    #[serde(default)]
    page_size: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

fn field(pointer: &'static str, code: &'static str) -> FieldError {
    FieldError { pointer, code }
}

fn query_ok(query: &str) -> bool {
    !query.trim().is_empty() && query.len() <= MAX_QUERY_BYTES
}

impl SearchQueryDto {
    pub fn into_input(self) -> Result<SearchInput, Vec<FieldError>> {
        let mut errors = Vec::new();
        if !query_ok(&self.query) {
            errors.push(field("/query", "OUT_OF_RANGE"));
        }
        if self
            .resource_types
            .as_ref()
            .is_some_and(|types| types.is_empty() || types.len() > 8)
        {
            errors.push(field("/resourceTypes", "OUT_OF_RANGE"));
        }
        if self
            .source_ids
            .as_ref()
            .is_some_and(|ids| ids.is_empty() || ids.len() > 16)
        {
            errors.push(field("/sourceIds", "OUT_OF_RANGE"));
        }
        let page_size = self.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=100).contains(&page_size) {
            errors.push(field("/pageSize", "OUT_OF_RANGE"));
        }
        let cursor = match self.cursor.as_deref().map(CursorHandle::parse) {
            None => None,
            Some(Some(cursor)) => Some(cursor),
            Some(None) => {
                errors.push(field("/cursor", "INVALID_FORMAT"));
                None
            }
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(SearchInput {
            query: self.query,
            resource_types: self
                .resource_types
                .unwrap_or_default()
                .into_iter()
                .map(ResourceKind::from)
                .collect(),
            source_ids: self
                .source_ids
                .unwrap_or_default()
                .into_iter()
                .map(SourceId::from_uuid)
                .collect(),
            coverage: self.coverage.into(),
            page_size,
            cursor,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DiscoveryNeedDto {
    purpose: String,
    required_resource_types: Vec<ResourceTypeDto>,
    required_claim_ids: Vec<Uuid>,
    #[serde(default)]
    temporal_target: Option<String>,
    #[serde(default)]
    business_timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DiscoveryGraphDto {
    seed_resource_ids: Vec<Uuid>,
    relation_type: String,
    from_role: String,
    to_role: String,
    #[serde(default)]
    max_hops: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DiscoveryInputDto {
    need: DiscoveryNeedDto,
    #[serde(default)]
    query: Option<String>,
    coverage: RequestCoverageDto,
    #[serde(default)]
    graph: Option<DiscoveryGraphDto>,
}

impl DiscoveryInputDto {
    pub fn into_input(self) -> Result<DiscoverInput, Vec<FieldError>> {
        let mut errors = Vec::new();
        let need = self.need;
        if need.purpose.trim().is_empty() || need.purpose.len() > MAX_PURPOSE_BYTES {
            errors.push(field("/need/purpose", "OUT_OF_RANGE"));
        }
        if need.required_resource_types.is_empty() || need.required_resource_types.len() > 8 {
            errors.push(field("/need/requiredResourceTypes", "OUT_OF_RANGE"));
        }
        let mut claims = need.required_claim_ids.clone();
        claims.sort();
        claims.dedup();
        if need.required_claim_ids.is_empty()
            || need.required_claim_ids.len() > 16
            || claims.len() != need.required_claim_ids.len()
        {
            errors.push(field("/need/requiredClaimIds", "OUT_OF_RANGE"));
        }
        let temporal_target = match need.temporal_target.as_deref() {
            None => None,
            Some(value) => match OffsetDateTime::parse(value, &Rfc3339) {
                Ok(parsed) => Some(parsed),
                Err(_) => {
                    errors.push(field("/need/temporalTarget", "INVALID_FORMAT"));
                    None
                }
            },
        };
        if need
            .business_timezone
            .as_ref()
            .is_some_and(|zone| zone.is_empty() || zone.len() > 255)
        {
            errors.push(field("/need/businessTimezone", "OUT_OF_RANGE"));
        }
        if self.query.as_deref().is_some_and(|query| !query_ok(query)) {
            errors.push(field("/query", "OUT_OF_RANGE"));
        }
        if self.coverage == RequestCoverageDto::BodyRequired && self.query.is_none() {
            errors.push(field("/query", "REQUIRED_FIELD"));
        }
        let graph = self.graph.map(|graph| {
            let mut seeds = graph.seed_resource_ids.clone();
            seeds.sort();
            seeds.dedup();
            if graph.seed_resource_ids.is_empty()
                || graph.seed_resource_ids.len() > MAX_GRAPH_SEEDS
                || seeds.len() != graph.seed_resource_ids.len()
            {
                errors.push(field("/graph/seedResourceIds", "OUT_OF_RANGE"));
            }
            for (pointer, value) in [
                ("/graph/relationType", &graph.relation_type),
                ("/graph/fromRole", &graph.from_role),
                ("/graph/toRole", &graph.to_role),
            ] {
                if !graph_token_ok(value) {
                    errors.push(field(pointer, "INVALID_FORMAT"));
                }
            }
            let max_hops = graph.max_hops.unwrap_or(2) as usize;
            if !(1..=MAX_GRAPH_HOPS).contains(&max_hops) {
                errors.push(field("/graph/maxHops", "OUT_OF_RANGE"));
            }
            DiscoverGraphInput {
                seed_resource_ids: graph
                    .seed_resource_ids
                    .into_iter()
                    .map(ResourceId::from_uuid)
                    .collect(),
                relation_type: graph.relation_type,
                from_role: graph.from_role,
                to_role: graph.to_role,
                max_hops,
            }
        });
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(DiscoverInput {
            purpose: need.purpose,
            required_resource_types: need
                .required_resource_types
                .into_iter()
                .map(ResourceKind::from)
                .collect(),
            required_claim_ids: need
                .required_claim_ids
                .into_iter()
                .map(ClaimId::from_uuid)
                .collect(),
            temporal_target,
            business_timezone: need.business_timezone,
            query: self.query,
            coverage: self.coverage.into(),
            graph,
        })
    }
}

fn id(value: impl Into<Uuid>) -> String {
    value.into().hyphenated().to_string()
}

fn resource(value: ResourceId) -> String {
    id(value.as_uuid())
}

fn source(value: SourceId) -> String {
    id(value.as_uuid())
}

const fn gap_code(code: PublicGapCode) -> &'static str {
    match code {
        PublicGapCode::MissingFact => "MISSING_FACT",
        PublicGapCode::InsufficientEvidenceClass => "INSUFFICIENT_EVIDENCE_CLASS",
        PublicGapCode::Authority => "AUTHORITY",
        PublicGapCode::Freshness => "FRESHNESS",
        PublicGapCode::Corroboration => "CORROBORATION",
        PublicGapCode::Conflict => "CONFLICT",
        PublicGapCode::Availability => "AVAILABILITY",
        PublicGapCode::UnsupportedCoverage => "UNSUPPORTED_COVERAGE",
        PublicGapCode::PaginationUnavailable => "PAGINATION_UNAVAILABLE",
        PublicGapCode::RequiredClaimUnresolved => "REQUIRED_CLAIM_UNRESOLVED",
        PublicGapCode::RequiredSourceUnavailable => "REQUIRED_SOURCE_UNAVAILABLE",
        PublicGapCode::BudgetExhausted => "BUDGET_EXHAUSTED",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GapDto {
    reason_code: &'static str,
    blocking: bool,
}

fn gaps(values: &[PublicGapView]) -> Vec<GapDto> {
    values
        .iter()
        .take(64)
        .map(|gap| GapDto {
            reason_code: gap_code(gap.code),
            blocking: gap.blocking,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceDto {
    source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_version_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchItemDto {
    resource_id: String,
    source_id: String,
    resource_type: ResourceTypeDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    rank: usize,
    matched_fields: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<SnippetDto>,
    provenance: ProvenanceDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnippetDto {
    text: String,
    field: &'static str,
    coverage: &'static str,
}

/// A body snippet: plain text, at most 320 code points, never empty.
fn body_snippet(value: &Option<String>) -> Option<SnippetDto> {
    value
        .as_ref()
        .filter(|text| !text.trim().is_empty() && text.chars().count() <= 320)
        .map(|text| SnippetDto {
            text: text.clone(),
            field: "body",
            coverage: "body",
        })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchCoverageDto {
    source_id: String,
    kind: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchPageDto {
    items: Vec<SearchItemDto>,
    next_cursor: Option<String>,
    partial: bool,
    coverage: Vec<SearchCoverageDto>,
    gaps: Vec<GapDto>,
    trace_id: String,
}

fn title(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .filter(|title| title.chars().count() <= 512)
        .cloned()
}

pub fn search_page(view: &SearchResultView, trace_id: Uuid) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&SearchPageDto {
        items: view
            .items()
            .iter()
            .filter_map(|item| {
                Some(SearchItemDto {
                    resource_id: resource(item.resource_id),
                    source_id: source(item.source_id),
                    resource_type: resource_type(item.resource_type?),
                    resource_version_id: item.resource_version.map(|version| id(version.as_uuid())),
                    title: title(&item.title),
                    rank: item.rank,
                    matched_fields: item
                        .matched
                        .iter()
                        .map(|matched| match matched {
                            MatchedField::Title => "title",
                            MatchedField::Metadata => "metadata",
                            MatchedField::Body => "body",
                        })
                        .collect(),
                    snippet: body_snippet(&item.snippet),
                    provenance: ProvenanceDto {
                        source_id: source(item.source_id),
                        resource_version_id: item
                            .resource_version
                            .map(|version| id(version.as_uuid())),
                    },
                })
            })
            .collect(),
        next_cursor: view.next_cursor().map(CursorHandle::to_wire),
        partial: view.partial(),
        coverage: view
            .coverage()
            .iter()
            .map(|coverage| SearchCoverageDto {
                source_id: source(coverage.source_id),
                kind: if coverage.body {
                    "bodySearchWithPerItemCoverage"
                } else {
                    "titleAndPermittedMetadata"
                },
            })
            .collect(),
        gaps: gaps(view.gaps()),
        trace_id: id(trace_id),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QualifiedDto {
    resource_id: String,
    source_id: String,
    applicability: &'static str,
    matched_condition_codes: Vec<String>,
    resolved_discriminator_codes: Vec<String>,
    evidence_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ValueDto {
    Text(String),
    Number(i64),
    Bool(bool),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceDto {
    id: String,
    claim_id: String,
    state: &'static str,
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<ValueDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RejectedDto {
    reason_code: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceDto {
    stage: &'static str,
    outcome_code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible_source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryEvaluationDto {
    need_id: String,
    discovery_evaluation_id: String,
    qualified_resources: Vec<QualifiedDto>,
    evidence_sufficiency: &'static str,
    evidence: Vec<EvidenceDto>,
    gaps: Vec<GapDto>,
    rejected_candidates: Vec<RejectedDto>,
    evaluation_completeness: &'static str,
    completeness_reason_codes: Vec<&'static str>,
    trace: Vec<TraceDto>,
    trace_id: String,
}

pub fn discovery_evaluation(view: &DiscoveryEvaluationView) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&DiscoveryEvaluationDto {
        need_id: id(view.need_id.as_uuid()),
        discovery_evaluation_id: id(view.evaluation_id.as_uuid()),
        qualified_resources: view
            .qualified
            .iter()
            .map(|item| QualifiedDto {
                resource_id: resource(item.resource_id),
                source_id: source(item.source_id),
                applicability: match item.applicability {
                    PublicApplicability::Applicable => "applicable",
                    PublicApplicability::Excluded => "excluded",
                    PublicApplicability::Unresolved => "unresolved",
                    PublicApplicability::Invalid => "invalid",
                },
                matched_condition_codes: item.matched_condition_codes.clone(),
                resolved_discriminator_codes: item.resolved_discriminator_codes.clone(),
                evidence_ids: item.evidence_ids.iter().map(|value| id(*value)).collect(),
            })
            .collect(),
        evidence_sufficiency: match view.sufficiency {
            PublicSufficiency::Sufficient => "sufficient",
            PublicSufficiency::Insufficient => "insufficient",
            PublicSufficiency::Unresolved => "unresolved",
            PublicSufficiency::Conflicted => "conflicted",
            PublicSufficiency::Invalid => "invalid",
        },
        evidence: view
            .evidence
            .iter()
            .map(|evidence| EvidenceDto {
                id: id(evidence.id),
                claim_id: id(evidence.claim_id.as_uuid()),
                state: match evidence.state {
                    PublicClaimState::Supported => "supported",
                    PublicClaimState::Absent => "absent",
                    PublicClaimState::Unknown => "unknown",
                    PublicClaimState::Conflicted => "conflicted",
                    PublicClaimState::Invalid => "invalid",
                },
                role: match evidence.role {
                    PublicEvidenceRole::Primary => "primary",
                    PublicEvidenceRole::Corroborating => "corroborating",
                    PublicEvidenceRole::Contradicting => "contradicting",
                    PublicEvidenceRole::Contextual => "contextual",
                    PublicEvidenceRole::Derived => "derived",
                },
                source_id: evidence.source_id.map(source),
                value: evidence.value.as_ref().map(|value| match value {
                    PublicEvidenceValue::Text(text) => ValueDto::Text(text.clone()),
                    PublicEvidenceValue::Integer(number) => ValueDto::Number(*number),
                    PublicEvidenceValue::Bool(flag) => ValueDto::Bool(*flag),
                }),
            })
            .collect(),
        gaps: gaps(&view.gaps),
        rejected_candidates: view
            .rejected_reason_codes
            .iter()
            .map(|code| RejectedDto { reason_code: code })
            .collect(),
        evaluation_completeness: match view.completeness {
            Completeness::Complete => "complete",
            Completeness::Bounded => "bounded",
            Completeness::Interrupted => "interrupted",
        },
        completeness_reason_codes: view
            .completeness_reasons
            .iter()
            .map(|reason| match reason {
                CompletenessReason::BudgetExhausted => "BUDGET_EXHAUSTED",
                CompletenessReason::SourceInterrupted => "SOURCE_INTERRUPTED",
                CompletenessReason::RequiredEvidenceUnevaluated => "REQUIRED_EVIDENCE_UNEVALUATED",
                CompletenessReason::BodyCoverageIncomplete => "BODY_COVERAGE_INCOMPLETE",
            })
            .collect(),
        trace: view
            .trace
            .iter()
            .map(|entry| TraceDto {
                stage: match entry.stage {
                    TraceStage::Routing => "routing",
                    TraceStage::Retrieval => "retrieval",
                    TraceStage::Qualification => "qualification",
                    TraceStage::Evidence => "evidence",
                    TraceStage::Disclosure => "disclosure",
                },
                outcome_code: match entry.outcome {
                    TraceOutcome::Completed => "completed",
                    TraceOutcome::Bounded => "bounded",
                    TraceOutcome::Interrupted => "interrupted",
                    TraceOutcome::Skipped => "skipped",
                    TraceOutcome::Unavailable => "unavailable",
                },
                visible_source_id: entry.visible_source_id.map(source),
                count: entry.count.map(|count| count.min(200)),
            })
            .collect(),
        trace_id: id(view.trace_id),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceDetailDto {
    resource_id: String,
    source_id: String,
    resource_type: ResourceTypeDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    coverage: &'static str,
    provenance: ProvenanceDto,
    trace_id: String,
}

pub fn resource_detail(view: &ResourceView, trace_id: Uuid) -> serde_json::Result<Vec<u8>> {
    let snapshot = view.snapshot();
    let version = snapshot
        .resource_version
        .map(|version| id(version.as_uuid()));
    serde_json::to_vec(&ResourceDetailDto {
        resource_id: resource(snapshot.resource_id),
        source_id: source(snapshot.source_id),
        resource_type: resource_type(snapshot.resource_type),
        resource_version_id: version.clone(),
        title: title(&snapshot.title),
        coverage: match snapshot.coverage {
            ResourceCoverage::TitleAndPermittedMetadata => "titleAndPermittedMetadata",
            ResourceCoverage::BodySupported => "bodySupported",
            ResourceCoverage::BodyPartial => "bodyPartial",
            ResourceCoverage::BodyUnsupported => "bodyUnsupported",
            ResourceCoverage::BodyUnknown => "bodyUnknown",
        },
        provenance: ProvenanceDto {
            source_id: source(snapshot.source_id),
            resource_version_id: version,
        },
        trace_id: id(trace_id),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceItemDto {
    source_id: String,
    source_type: &'static str,
    resource_types: Vec<ResourceTypeDto>,
    discovery_modes: Vec<&'static str>,
    enumeration_semantics: &'static str,
    coverage: Vec<&'static str>,
    availability_code: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourcePageDto {
    items: Vec<SourceItemDto>,
    next_cursor: Option<String>,
    trace_id: String,
}

pub fn source_page(view: &SourceView, trace_id: Uuid) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&SourcePageDto {
        items: view
            .items()
            .iter()
            .map(|item| SourceItemDto {
                source_id: source(item.source_id),
                source_type: match item.source_type {
                    SourceKind::Document => "document",
                    SourceKind::Remote => "remote",
                },
                resource_types: item
                    .resource_types
                    .iter()
                    .copied()
                    .map(resource_type)
                    .take(8)
                    .collect(),
                discovery_modes: item
                    .discovery_modes
                    .iter()
                    .map(|mode| match mode {
                        DiscoveryMode::LocalDirectory => "localDirectory",
                        DiscoveryMode::LocalContentSearch => "localContentSearch",
                        DiscoveryMode::RemoteEnumeration => "remoteEnumeration",
                        DiscoveryMode::RemoteQuery => "remoteQuery",
                        DiscoveryMode::DirectAddress => "directAddress",
                        DiscoveryMode::LiveOnly => "liveOnly",
                    })
                    .collect(),
                enumeration_semantics: match item.enumeration_semantics {
                    EnumerationSemantics::Complete => "complete",
                    EnumerationSemantics::Partial => "partial",
                    EnumerationSemantics::QueryOnly => "queryOnly",
                    EnumerationSemantics::None => "none",
                },
                coverage: item
                    .coverage
                    .iter()
                    .map(|coverage| match coverage {
                        SourceCoverageKind::TitleAndPermittedMetadata => {
                            "titleAndPermittedMetadata"
                        }
                        SourceCoverageKind::BodySearchWithPerItemCoverage => {
                            "bodySearchWithPerItemCoverage"
                        }
                    })
                    .collect(),
                availability_code: "available",
            })
            .collect(),
        next_cursor: view.next_cursor().map(CursorHandle::to_wire),
        trace_id: id(trace_id),
    })
}
