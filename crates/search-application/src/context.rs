//! Task-scoped context assembly. Source content remains data, never authority.

use std::collections::{BTreeMap, BTreeSet};

use search_core::binding::{RepresentationBinding, RevalidationMarker};
use search_core::id::SourceId;
use search_core::projection::ProjectionGenerationKey;

use crate::error::SearchError;
use crate::session::{BoundResourceKey, SessionWorkingSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextSegmentType {
    Skill,
    WorkflowFragment,
    KnowledgeFragment,
    EvidenceReference,
    SemanticResource,
    CapabilityContract,
    ConcreteToolSchema,
    PolicyGuidance,
    Fact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTrustClass {
    TrustedLocal,
    UntrustedContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextRole {
    Instruction,
    Data,
}

/// These instructions are compiled into the application. A Source cannot
/// nominate an instruction by supplying a label or its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovedLocalInstruction {
    BoundResourceSkill,
}

impl ApprovedLocalInstruction {
    const fn segment_id(self) -> &'static str {
        match self {
            Self::BoundResourceSkill => "local-skill",
        }
    }

    const fn purpose(self) -> &'static str {
        match self {
            Self::BoundResourceSkill => "session-bound resource guidance",
        }
    }

    const fn content(self) -> &'static str {
        match self {
            Self::BoundResourceSkill => {
                "Use only task-selected resources with their session binding; revalidate before execution."
            }
        }
    }

    const fn digest(self) -> &'static str {
        match self {
            Self::BoundResourceSkill => "approved-local-bound-resource-skill-v1",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextProvenance {
    ApprovedLocal(ApprovedLocalInstruction),
    PublicToolSchema,
    Remote(SourceId),
}

/// Full identity attached to any resource-backed segment. An Execution
/// manifest checks both this value and the original generation against its
/// Session Working Set before the segment can be included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextResourceBinding {
    key: BoundResourceKey,
    generation: ProjectionGenerationKey,
    binding: RepresentationBinding,
}

impl ContextResourceBinding {
    pub fn new(
        key: BoundResourceKey,
        generation: ProjectionGenerationKey,
        binding: RepresentationBinding,
    ) -> Result<Self, SearchError> {
        if key.source_ref != generation.source_id || key.source_ref != binding.source_ref {
            return Err(SearchError::InvalidRequest(
                "context resource Source differs from its binding or generation".into(),
            ));
        }
        binding
            .validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        Ok(Self {
            key,
            generation,
            binding,
        })
    }

    pub const fn key(&self) -> BoundResourceKey {
        self.key
    }

    pub const fn generation(&self) -> ProjectionGenerationKey {
        self.generation
    }

    pub fn binding(&self) -> &RepresentationBinding {
        &self.binding
    }
}

/// Only public input shapes are representable. Runtime credentials, parameter
/// values, defaults and examples have no place in this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicToolParameterType {
    String,
    Integer,
}

impl PublicToolParameterType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
        }
    }
}

/// Opaque outside this module: only the compiled catalog can define inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicToolParameter {
    name: &'static str,
    kind: PublicToolParameterType,
    required: bool,
}

const SEARCH_KNOWLEDGE_INPUTS: &[PublicToolParameter] = &[
    PublicToolParameter {
        name: "query",
        kind: PublicToolParameterType::String,
        required: true,
    },
    PublicToolParameter {
        name: "limit",
        kind: PublicToolParameterType::Integer,
        required: false,
    },
];

/// Local tool schemas come only from the compiled catalog. Callers cannot
/// introduce a credential-shaped field into trusted task context.
///
/// ```compile_fail
/// use search_application::context::{
///     PublicToolParameter, PublicToolParameterType, PublicToolSchema,
/// };
/// let parameter = PublicToolParameter::new(
///     "api_token", PublicToolParameterType::String, true,
/// ).unwrap();
/// let _schema = PublicToolSchema::new("credential_sink", vec![parameter]).unwrap();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicToolSchema {
    SearchKnowledge,
}

impl PublicToolSchema {
    pub const fn digest(self) -> &'static str {
        match self {
            Self::SearchKnowledge => "approved-search-knowledge-schema-v1",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::SearchKnowledge => "search_knowledge",
        }
    }

    const fn inputs(self) -> &'static [PublicToolParameter] {
        match self {
            Self::SearchKnowledge => SEARCH_KNOWLEDGE_INPUTS,
        }
    }

    fn render(self) -> String {
        let inputs: Vec<_> = self
            .inputs()
            .iter()
            .map(|input| {
                serde_json::json!({
                    "name": input.name,
                    "type": input.kind.as_str(),
                    "required": input.required,
                })
            })
            .collect();
        serde_json::json!({"tool": self.name(), "inputs": inputs}).to_string()
    }
}

/// Remote content digests come from the provider. Trusted tool schema digests
/// come only from the compiled catalog and must match any session binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSegment {
    segment_id: String,
    segment_type: ContextSegmentType,
    resource_binding: Option<ContextResourceBinding>,
    evidence_ref: Option<String>,
    purpose: String,
    trust_class: ContextTrustClass,
    role: ContextRole,
    provenance: ContextProvenance,
    content_digest: String,
    content: String,
}

impl ContextSegment {
    #[allow(clippy::too_many_arguments)]
    pub fn remote_data(
        segment_id: impl Into<String>,
        segment_type: ContextSegmentType,
        resource_binding: Option<ContextResourceBinding>,
        evidence_ref: Option<String>,
        purpose: impl Into<String>,
        source_ref: SourceId,
        content_digest: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self, SearchError> {
        if resource_binding
            .as_ref()
            .is_some_and(|resource| resource.key.source_ref != source_ref)
        {
            return Err(SearchError::InvalidRequest(
                "remote context Source differs from its resource".into(),
            ));
        }
        Self::new(
            segment_id,
            segment_type,
            resource_binding,
            evidence_ref,
            purpose,
            ContextProvenance::Remote(source_ref),
            content_digest,
            content,
            ContextTrustClass::UntrustedContent,
            ContextRole::Data,
        )
    }

    pub fn approved_local_instruction(instruction: ApprovedLocalInstruction) -> Self {
        Self::new(
            instruction.segment_id(),
            ContextSegmentType::Skill,
            None,
            None,
            instruction.purpose(),
            ContextProvenance::ApprovedLocal(instruction),
            instruction.digest(),
            instruction.content(),
            ContextTrustClass::TrustedLocal,
            ContextRole::Instruction,
        )
        .expect("approved local instruction is a complete static contract")
    }

    pub fn public_tool_schema(
        segment_id: impl Into<String>,
        resource_binding: Option<ContextResourceBinding>,
        schema: PublicToolSchema,
    ) -> Result<Self, SearchError> {
        if resource_binding.as_ref().is_some_and(|resource| {
            resource.binding().schema_digest.as_deref() != Some(schema.digest())
        }) {
            return Err(SearchError::InvalidRequest(
                "approved tool schema digest differs from the resource binding".into(),
            ));
        }
        Self::new(
            segment_id,
            ContextSegmentType::ConcreteToolSchema,
            resource_binding,
            None,
            "task tool input contract",
            ContextProvenance::PublicToolSchema,
            schema.digest(),
            schema.render(),
            ContextTrustClass::TrustedLocal,
            ContextRole::Instruction,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        segment_id: impl Into<String>,
        segment_type: ContextSegmentType,
        resource_binding: Option<ContextResourceBinding>,
        evidence_ref: Option<String>,
        purpose: impl Into<String>,
        provenance: ContextProvenance,
        content_digest: impl Into<String>,
        content: impl Into<String>,
        trust_class: ContextTrustClass,
        role: ContextRole,
    ) -> Result<Self, SearchError> {
        let segment_id = segment_id.into();
        let purpose = purpose.into();
        let content_digest = content_digest.into();
        if segment_id.is_empty() || purpose.is_empty() || content_digest.is_empty() {
            return Err(SearchError::InvalidRequest(
                "context segment identity, purpose and digest are required".into(),
            ));
        }
        Ok(Self {
            segment_id,
            segment_type,
            resource_binding,
            evidence_ref,
            purpose,
            trust_class,
            role,
            provenance,
            content_digest,
            content: content.into(),
        })
    }

    pub fn segment_id(&self) -> &str {
        &self.segment_id
    }

    pub const fn segment_type(&self) -> ContextSegmentType {
        self.segment_type
    }

    pub const fn resource_ref(&self) -> Option<BoundResourceKey> {
        match &self.resource_binding {
            Some(resource) => Some(resource.key),
            None => None,
        }
    }

    pub fn resource_binding(&self) -> Option<&ContextResourceBinding> {
        self.resource_binding.as_ref()
    }

    pub fn evidence_ref(&self) -> Option<&str> {
        self.evidence_ref.as_deref()
    }

    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    pub const fn trust_class(&self) -> ContextTrustClass {
        self.trust_class
    }

    pub const fn role(&self) -> ContextRole {
        self.role
    }

    pub const fn provenance(&self) -> &ContextProvenance {
        &self.provenance
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMode {
    Planning,
    Execution,
}

/// Selection comes from the task planner. These fields identify context, not
/// credentials or provider tokens.
pub struct TaskContextSelection {
    pub task_id: String,
    pub task_graph_revision: String,
    pub mode: ContextMode,
    pub selected_segment_ids: BTreeSet<String>,
    pub selected_resources: BTreeSet<BoundResourceKey>,
    pub selected_tool_schema_ids: BTreeSet<String>,
    pub context_budget: usize,
}

pub struct TaskContextManifest {
    task_id: String,
    task_graph_revision: String,
    mode: ContextMode,
    segments: Vec<ContextSegment>,
    context_budget: usize,
    revalidation_marker: RevalidationMarker,
}

impl TaskContextManifest {
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn task_graph_revision(&self) -> &str {
        &self.task_graph_revision
    }

    pub const fn mode(&self) -> ContextMode {
        self.mode
    }

    pub fn segments(&self) -> &[ContextSegment] {
        &self.segments
    }

    pub const fn context_budget(&self) -> usize {
        self.context_budget
    }

    pub const fn revalidation_marker(&self) -> RevalidationMarker {
        self.revalidation_marker
    }
}

pub struct ContextCompiler;

impl ContextCompiler {
    pub fn compile(
        selection: &TaskContextSelection,
        available: impl IntoIterator<Item = ContextSegment>,
        working_set: Option<&SessionWorkingSet>,
    ) -> Result<TaskContextManifest, SearchError> {
        if selection.task_id.is_empty() || selection.task_graph_revision.is_empty() {
            return Err(SearchError::InvalidRequest(
                "task identity and graph revision are required".into(),
            ));
        }
        if selection.mode == ContextMode::Execution {
            let session = working_set.ok_or_else(|| {
                SearchError::InvalidRequest("execution context requires session bindings".into())
            })?;
            if selection
                .selected_resources
                .iter()
                .any(|resource| session.bound(*resource).is_none())
            {
                return Err(SearchError::InvalidRequest(
                    "execution context includes an unbound resource".into(),
                ));
            }
        }
        let mut selected = BTreeMap::new();
        for segment in available {
            if !selection
                .selected_segment_ids
                .contains(segment.segment_id())
                || segment
                    .resource_ref()
                    .is_some_and(|resource| !selection.selected_resources.contains(&resource))
                || (segment.segment_type() == ContextSegmentType::ConcreteToolSchema
                    && !selection
                        .selected_tool_schema_ids
                        .contains(segment.segment_id()))
            {
                continue;
            }
            if selection.mode == ContextMode::Execution
                && matches!(segment.provenance, ContextProvenance::Remote(_))
                && segment.resource_binding.is_none()
            {
                return Err(SearchError::InvalidRequest(
                    "execution context requires a bound remote resource".into(),
                ));
            }
            if (selection.mode == ContextMode::Execution
                || segment.segment_type() == ContextSegmentType::ConcreteToolSchema)
                && let Some(resource) = segment.resource_binding()
            {
                let session = working_set.ok_or_else(|| {
                    SearchError::InvalidRequest(
                        "resource-derived tool schema requires session bindings".into(),
                    )
                })?;
                if session.bound(resource.key()) != Some(resource.binding())
                    || session.pinned_generation(resource.key()) != Some(resource.generation())
                {
                    return Err(SearchError::InvalidRequest(
                        "context resource differs from the session binding or generation".into(),
                    ));
                }
                let bound_digest =
                    if segment.segment_type() == ContextSegmentType::ConcreteToolSchema {
                        resource.binding().schema_digest.as_deref()
                    } else {
                        resource.binding().content_digest.as_deref()
                    };
                if segment.segment_type() == ContextSegmentType::ConcreteToolSchema
                    && bound_digest.is_none()
                {
                    return Err(SearchError::InvalidRequest(
                        "resource-derived tool schema requires a schema digest".into(),
                    ));
                }
                if bound_digest.is_some_and(|digest| segment.content_digest() != digest) {
                    return Err(SearchError::InvalidRequest(
                        "context digest differs from the session binding".into(),
                    ));
                }
            }
            match selected.entry(segment.segment_id.clone()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(segment);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    if slot.get() != &segment {
                        return Err(SearchError::InvalidRequest(
                            "conflicting context segments share an ID".into(),
                        ));
                    }
                }
            }
        }
        let mut segments: Vec<_> = selected.into_values().collect();
        segments.sort_by(|left, right| {
            left.segment_type
                .cmp(&right.segment_type)
                .then_with(|| left.resource_ref().cmp(&right.resource_ref()))
                .then_with(|| left.segment_id.cmp(&right.segment_id))
        });
        let content_bytes = segments.iter().try_fold(0usize, |sum, segment| {
            sum.checked_add(segment.content.len())
        });
        if !content_bytes.is_some_and(|bytes| bytes <= selection.context_budget) {
            return Err(SearchError::InvalidRequest(
                "selected context exceeds the task budget".into(),
            ));
        }
        Ok(TaskContextManifest {
            task_id: selection.task_id.clone(),
            task_graph_revision: selection.task_graph_revision.clone(),
            mode: selection.mode,
            segments,
            context_budget: selection.context_budget,
            revalidation_marker: if selection.mode == ContextMode::Execution {
                RevalidationMarker::Required
            } else {
                RevalidationMarker::NotRequired
            },
        })
    }
}
