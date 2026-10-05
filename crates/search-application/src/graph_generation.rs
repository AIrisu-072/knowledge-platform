//! P3-G01: SQLx-free durable Graph generation records and ports.
//!
//! Reference values here are identities only. Anyone can construct them with
//! `from_identifiers`; private fields keep layout stable but never grant
//! anything. Child writes are admitted only by stored database rows, the actual
//! role, the guard token/fence and the database clock; reads only by a stored,
//! unexpired evaluation lease rechecked before and after the read.

use search_core::graph::GraphTraversalPlan;
use search_core::id::{DiscoveryEvaluationId, RelationId, ResourceId, ResourceVersionId};
use search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest, TemporalProjection,
};
use search_core::relation::TypedRelationInstance;
use search_core::resource::ResourceKind;
use uuid::Uuid;

use crate::ports::{AccessDecision, BoxFuture, GraphRetrievalResult};
use crate::scoped::{AuthorizedSourceScope, TrustedDiscoveryBinding};

/// Source-owned identity behind one Graph Resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphSourceMapping {
    Document {
        document_id: Uuid,
    },
    FolderPlacement {
        document_id: Uuid,
        folder_id: Uuid,
    },
    Version {
        document_id: Uuid,
        version_id: Uuid,
    },
    /// Passes only through a Source-registered mapping validator.
    Registered {
        adapter_id: String,
        native_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResourceRecord {
    pub resource_ref: ResourceId,
    pub kind: ResourceKind,
    pub resource_version_ref: Option<ResourceVersionId>,
    pub temporal: TemporalProjection,
    pub mapping: GraphSourceMapping,
    /// Every n-ary relation this Resource participates in, whole.
    pub attached_relations: Vec<TypedRelationInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureBasis {
    CompleteEnumeration,
    AuthoritativeChangeStream { cursor: String },
}

/// Proof that an incremental delta covers every relation touched between two
/// Source snapshots. Unprovable closure requires a full rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRelationClosureProof {
    pub base_snapshot: String,
    pub target_snapshot: String,
    pub affected_resources: Vec<ResourceId>,
    pub old_relation_ids: Vec<RelationId>,
    pub new_relation_ids: Vec<RelationId>,
    pub basis: ClosureBasis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphIncrementalDelta {
    pub changed_resources: Vec<GraphResourceRecord>,
    pub retired_resources: Vec<ResourceId>,
    pub changed_relation_ids: Vec<RelationId>,
    pub retired_relation_ids: Vec<RelationId>,
    pub replacement_relations: Vec<TypedRelationInstance>,
    pub target_source_mapping_digest: String,
    pub proof: GraphRelationClosureProof,
}

/// Recomputed, stored receipt of one READY Graph generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphGenerationReceipt {
    pub key: ProjectionGenerationKey,
    pub projection_manifest_digest: String,
    pub source_snapshot: String,
    pub source_mapping_digest: String,
    pub graph_content_digest: String,
    pub resource_count: u64,
    pub relation_count: u64,
    pub graph_schema_version: String,
}

/// Identity of a registered FULL target. Not an authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredFullBuildHandle {
    key: ProjectionGenerationKey,
    guard_token: Uuid,
    build_fence: i64,
}

impl RegisteredFullBuildHandle {
    pub const fn from_identifiers(
        key: ProjectionGenerationKey,
        guard_token: Uuid,
        build_fence: i64,
    ) -> Self {
        Self {
            key,
            guard_token,
            build_fence,
        }
    }
    pub const fn key(&self) -> ProjectionGenerationKey {
        self.key
    }
    pub const fn guard_token(&self) -> Uuid {
        self.guard_token
    }
    pub const fn build_fence(&self) -> i64 {
        self.build_fence
    }
}

/// Identity of an incremental base→target guard. Not an authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildGuardHandle {
    base_key: ProjectionGenerationKey,
    target_key: ProjectionGenerationKey,
    guard_token: Uuid,
    build_fence: i64,
}

impl BuildGuardHandle {
    pub const fn from_identifiers(
        base_key: ProjectionGenerationKey,
        target_key: ProjectionGenerationKey,
        guard_token: Uuid,
        build_fence: i64,
    ) -> Self {
        Self {
            base_key,
            target_key,
            guard_token,
            build_fence,
        }
    }
    pub const fn base_key(&self) -> ProjectionGenerationKey {
        self.base_key
    }
    pub const fn target_key(&self) -> ProjectionGenerationKey {
        self.target_key
    }
    pub const fn guard_token(&self) -> Uuid {
        self.guard_token
    }
    pub const fn build_fence(&self) -> i64 {
        self.build_fence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphBuildRef {
    Full(RegisteredFullBuildHandle),
    Incremental(BuildGuardHandle),
}

impl GraphBuildRef {
    pub const fn target_key(&self) -> ProjectionGenerationKey {
        match self {
            Self::Full(handle) => handle.key(),
            Self::Incremental(handle) => handle.target_key(),
        }
    }
}

/// Identity of a server-issued evaluation pin. Never authority by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphReadLease {
    key: ProjectionGenerationKey,
    evaluation_id: DiscoveryEvaluationId,
    lease_id: Uuid,
}

impl GraphReadLease {
    pub const fn from_identifiers(
        key: ProjectionGenerationKey,
        evaluation_id: DiscoveryEvaluationId,
        lease_id: Uuid,
    ) -> Self {
        Self {
            key,
            evaluation_id,
            lease_id,
        }
    }
    pub const fn key(&self) -> ProjectionGenerationKey {
        self.key
    }
    pub const fn evaluation_id(&self) -> DiscoveryEvaluationId {
        self.evaluation_id
    }
    pub const fn lease_id(&self) -> Uuid {
        self.lease_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphBatchPhase {
    Copy,
    Delta,
}

/// Expected position of the next batch. Not a committed checkpoint: the
/// database row is compared and advanced in the same transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphBatchCursor {
    pub target_key: ProjectionGenerationKey,
    pub phase: GraphBatchPhase,
    pub committed_sequence: u64,
}

/// A staged target. Carries no READY flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphStage {
    pub key: ProjectionGenerationKey,
}

/// Physical pre-READY comparison data recomputed from staged rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStageReport {
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub projection_manifest_digest: String,
    pub source_mapping_digest: String,
    pub graph_content_digest: String,
    pub resource_count: u64,
    pub relation_count: u64,
    pub graph_schema_version: String,
}

/// Untrusted comparison data from a mapping validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSourceMappingReceipt {
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub mapping_digest: String,
}

pub trait DurableGraphGenerationPort: Send + Sync {
    fn stage_full_registered<'a>(
        &'a self,
        target: &'a RegisteredFullBuildHandle,
        resources: &'a [GraphResourceRecord],
        relations: &'a [TypedRelationInstance],
    ) -> BoxFuture<'a, GraphStage>;

    fn copy_batch<'a>(
        &'a self,
        target: &'a BuildGuardHandle,
        expected: &'a GraphBatchCursor,
        limit: u32,
    ) -> BoxFuture<'a, GraphBatchCursor>;

    fn verify_copy<'a>(&'a self, target: &'a BuildGuardHandle) -> BoxFuture<'a, ()>;

    fn apply_delta_batch<'a>(
        &'a self,
        target: &'a BuildGuardHandle,
        delta: &'a GraphIncrementalDelta,
        expected: &'a GraphBatchCursor,
        limit: u32,
    ) -> BoxFuture<'a, GraphBatchCursor>;

    fn validate_staged<'a>(&'a self, target: &'a GraphBuildRef) -> BoxFuture<'a, GraphStageReport>;

    fn recover_ready<'a>(
        &'a self,
        key: &'a ProjectionGenerationKey,
        expected_manifest_digest: &'a str,
    ) -> BoxFuture<'a, GraphGenerationReceipt>;
}

pub trait GraphSourceMappingValidatorPort: Send + Sync {
    fn validate_authoritative<'a>(
        &'a self,
        manifest: &'a ProjectionGenerationManifest,
        records: &'a [GraphResourceRecord],
    ) -> BoxFuture<'a, GraphSourceMappingReceipt>;
}

pub trait GraphLeaseVerifierPort: Send + Sync {
    fn verify<'a>(
        &'a self,
        lease: &'a GraphReadLease,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, ()>;
}

pub trait GenerationScopedGraphAccessPort: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        key: &'a ProjectionGenerationKey,
        resource_ref: ResourceId,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision>;
}

pub trait PinnedGraphRetrievalPort: Send + Sync {
    fn retrieve_pinned<'a>(
        &'a self,
        lease: &'a GraphReadLease,
        plan: &'a GraphTraversalPlan,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, GraphRetrievalResult>;
}
