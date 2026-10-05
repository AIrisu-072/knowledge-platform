//! P4-06: one sealed, immutable RAM generation per Source and evaluation.
//!
//! Responses of several remote actions are staged into one Source batch only
//! when they share one verified Source snapshot and the same trusted context
//! (actor, Source scope, registration and visibility revisions). The same
//! Resource seen twice must agree on version, digest and every same-name
//! field and its provenance, else the whole batch is an integrity conflict.
//! A response that cannot prove a shared snapshot is an explicit gap and is
//! not appended. An ID-less hit is an explicit gap and never a candidate.
//! The generation key is minted here, never by a provider, and the generation
//! is never converted into a persistable manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use search_core::assertion::Assertion;
use search_core::discovery::{
    CandidateIdentityClass, FederatedCandidate, GapReason, InformationGap,
};
use search_core::id::{DiscoveryEvaluationId, ProjectionGenerationId, ResourceId};
use search_core::observation::Coverage;
use search_core::profile::FacetState;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalDiscoveryProfile;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::SearchError;
use crate::remote::{
    EvaluationLeaseId, PinnedRemoteTarget, RemoteActionResponse, RemoteIdentity,
    RemoteOperationKind, TrustedRemoteContext, UntrustedFieldValue, UntrustedRemoteHit,
};
use crate::remote_evidence::{
    RegisteredLineage, RemoteProvenanceLookupPort, UntrustedEvidenceHint, VerifiedProvenance,
    verify_provenance,
};
use crate::remote_identity::{remote_candidate_id, remote_resource_id};
use crate::remote_observation::{SnapshotExtent, SourceSnapshotProof};
use crate::retrieval::OpaqueNativeId;

pub const REMOTE_PROJECTION_SCHEMA: &str = "remote-evaluation-v1";

/// Subject of every remote Assertion; remote ClaimSelectors use the same one.
pub const REMOTE_CLAIM_SUBJECT: &str = "remote-resource";

fn conflict() -> SearchError {
    SearchError::OperationFailed("remote Source batch integrity conflict".into())
}

/// Low-cardinality record of one completed action; no provider token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionReceipt {
    pub retriever_id: String,
    pub operation: RemoteOperationKind,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageOutcome {
    Staged,
    /// The action was not appended; the batch continues without it.
    Gap(InformationGap),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StagedResource {
    pub(crate) native_id: OpaqueNativeId,
    pub(crate) version: Option<String>,
    pub(crate) digest: Option<String>,
    kind: Option<ResourceKind>,
    title: Option<String>,
    pub(crate) fields: BTreeMap<String, UntrustedFieldValue>,
}

impl StagedResource {
    fn from_hit(native_id: OpaqueNativeId, hit: &UntrustedRemoteHit) -> Self {
        Self {
            native_id,
            version: hit.version().map(str::to_owned),
            digest: hit.digest().map(str::to_owned),
            kind: hit.kind(),
            title: hit.title().map(str::to_owned),
            fields: hit.fields().clone(),
        }
    }

    /// One canonical projection: identical identity, version and digest; a
    /// partial view may add fields only when both carry a version or digest.
    fn merge(&mut self, other: Self) -> Result<(), SearchError> {
        if self.native_id != other.native_id
            || self.version != other.version
            || self.digest != other.digest
            || (self.kind.is_some() && other.kind.is_some() && self.kind != other.kind)
            || (self.title.is_some() && other.title.is_some() && self.title != other.title)
        {
            return Err(conflict());
        }
        let verified_partial = self.version.is_some() || self.digest.is_some();
        if !verified_partial && self.fields != other.fields {
            return Err(conflict());
        }
        for (name, value) in other.fields {
            match self.fields.get(&name) {
                Some(existing) if *existing != value => return Err(conflict()),
                Some(_) => {}
                None => {
                    self.fields.insert(name, value);
                }
            }
        }
        self.kind = self.kind.or(other.kind);
        self.title = self.title.take().or(other.title);
        Ok(())
    }
}

pub struct RemoteGenerationBuilder {
    context: TrustedRemoteContext,
    lease: EvaluationLeaseId,
    proof: Option<SourceSnapshotProof>,
    receipts: Vec<ActionReceipt>,
    resources: BTreeMap<ResourceId, StagedResource>,
    lists: Vec<(String, RemoteOperationKind, Vec<ResourceId>)>,
    gaps: Vec<InformationGap>,
    verified: BTreeMap<(ResourceId, String), (String, VerifiedProvenance)>,
    poisoned: bool,
}

impl fmt::Debug for RemoteGenerationBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteGenerationBuilder(<opaque>)")
    }
}

fn source_gap(source: &TrustedRemoteContext, code: &str) -> InformationGap {
    InformationGap::new(
        format!(
            "source:{}:{code}",
            source.source_scope().source_id().as_uuid()
        ),
        GapReason::UnsupportedCoverage,
        false,
    )
}

impl RemoteGenerationBuilder {
    pub fn new(
        context: TrustedRemoteContext,
        evaluation: DiscoveryEvaluationId,
        lease: EvaluationLeaseId,
    ) -> Result<Self, SearchError> {
        if context.binding().evaluation() != evaluation {
            return Err(SearchError::InvalidRequest(
                "remote generation is bound to another evaluation".into(),
            ));
        }
        Ok(Self {
            context,
            lease,
            proof: None,
            receipts: Vec::new(),
            resources: BTreeMap::new(),
            lists: Vec::new(),
            gaps: Vec::new(),
            verified: BTreeMap::new(),
            poisoned: false,
        })
    }

    /// Before seal: each field's provider provenance label is only a hint;
    /// the fixed Source's lookup against the pinned version and digest and the
    /// registered lineage decide whether it becomes verified evidence.
    pub async fn verify_evidence(
        &mut self,
        lineage: &RegisteredLineage,
        lookup: &dyn RemoteProvenanceLookupPort,
    ) -> Result<(), SearchError> {
        let Some(proof) = self.proof.clone() else {
            return Ok(());
        };
        let registration = self.context.registration().clone();
        let candidates: Vec<(ResourceId, StagedResource)> = self
            .resources
            .iter()
            .map(|(id, staged)| (*id, staged.clone()))
            .collect();
        for (id, staged) in candidates {
            let target = PinnedRemoteTarget::from_parts(
                RemoteIdentity::new(&self.context, staged.native_id.clone())?,
                proof.clone(),
                staged.version.clone(),
                staged.digest.clone(),
            );
            for (field, value) in &staged.fields {
                let Some(label) = &value.provenance else {
                    continue;
                };
                let hint = UntrustedEvidenceHint::new(label.clone(), None, None, false)?;
                if let Some(verified) = verify_provenance(
                    &registration,
                    lineage,
                    self.context.source_scope(),
                    &target,
                    &hint,
                    lookup,
                )
                .await?
                {
                    self.verified
                        .insert((id, field.clone()), (label.clone(), verified));
                }
            }
        }
        Ok(())
    }

    fn poison(&mut self) -> SearchError {
        self.poisoned = true;
        self.resources.clear();
        self.lists.clear();
        conflict()
    }

    pub fn stage(&mut self, response: RemoteActionResponse) -> Result<StageOutcome, SearchError> {
        if self.poisoned {
            return Err(conflict());
        }
        if !response.action.matches_context(&self.context)
            || !response.proof.matches_context(&self.context)
        {
            // Another actor, Source scope, registration or ACL revision.
            return Err(self.poison());
        }
        if self
            .lists
            .iter()
            .any(|(retriever, _, _)| retriever == response.retriever_id())
        {
            return Err(SearchError::InvalidRequest(
                "remote retriever staged twice in one batch".into(),
            ));
        }
        match &self.proof {
            None => self.proof = Some(response.proof.clone()),
            Some(first) if first.same_source_snapshot(&response.proof) => {}
            Some(first)
                if first.extent() == SnapshotExtent::SingleResponse
                    || response.proof.extent() == SnapshotExtent::SingleResponse =>
            {
                let gap = source_gap(&self.context, "remote_snapshot_incompatible");
                self.gaps.push(gap.clone());
                return Ok(StageOutcome::Gap(gap));
            }
            Some(_) => return Err(self.poison()),
        }
        let registration = self.context.registration().clone();
        let allowed = registration.allowed_resource_kinds();
        let tenant = registration.tenant().clone();
        let source = registration.source_id();
        let mut ids = Vec::new();
        let mut ephemeral = false;
        let mut unregistered_kind = false;
        for hit in response.hits() {
            let Some(native) = hit.native_id().cloned() else {
                ephemeral = true;
                continue;
            };
            if hit
                .kind()
                .is_some_and(|kind| !allowed.is_empty() && !allowed.contains(&kind))
            {
                unregistered_kind = true;
                continue;
            }
            let id = remote_resource_id(&tenant, source, registration.provider_kind(), &native)?;
            let staged = StagedResource::from_hit(native, hit);
            match self.resources.get_mut(&id) {
                Some(existing) => {
                    if existing.merge(staged).is_err() {
                        return Err(self.poison());
                    }
                }
                None => {
                    self.resources.insert(id, staged);
                }
            }
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ephemeral {
            self.gaps.push(source_gap(
                &self.context,
                "ephemeral_identity_not_qualifiable",
            ));
        }
        if unregistered_kind {
            self.gaps.push(source_gap(
                &self.context,
                "remote_resource_kind_not_registered",
            ));
        }
        self.receipts.push(ActionReceipt {
            retriever_id: response.retriever_id().into(),
            operation: response.operation(),
            coverage: response.coverage(),
        });
        self.lists
            .push((response.retriever_id().into(), response.operation(), ids));
        Ok(StageOutcome::Staged)
    }

    /// Seals once. Nothing can be appended or replaced afterwards.
    pub fn seal(self) -> Result<RemoteEvaluationGeneration, SearchError> {
        let Some(proof) = self.proof.clone() else {
            return Err(SearchError::InvalidRequest(
                "a remote generation needs one completed action".into(),
            ));
        };
        if self.poisoned {
            return Err(conflict());
        }
        let registration = self.context.registration();
        let source = registration.source_id();
        let key = ProjectionGenerationKey {
            source_id: source,
            generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
        };
        let coverage = if self
            .receipts
            .iter()
            .any(|r| r.operation == RemoteOperationKind::Enumerate)
        {
            Coverage::PartialEnumeration
        } else if self
            .receipts
            .iter()
            .any(|r| matches!(r.operation, RemoteOperationKind::Query))
        {
            Coverage::QueryResult
        } else {
            Coverage::DirectLookup
        };
        let default_kind = registration
            .allowed_resource_kinds()
            .first()
            .copied()
            .unwrap_or(ResourceKind::Knowledge);
        let mut digest = Sha256::new();
        digest.update(b"search-remote-generation:v1");
        for (id, staged) in &self.resources {
            digest.update(id.as_uuid().as_bytes());
            digest.update(staged.native_id.as_str().as_bytes());
            digest.update(staged.version.as_deref().unwrap_or("").as_bytes());
            digest.update(staged.digest.as_deref().unwrap_or("").as_bytes());
        }
        let hex: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let manifest = ProjectionGenerationManifest {
            source_id: source,
            generation_id: key.generation_id,
            projection_schema_version: REMOTE_PROJECTION_SCHEMA.into(),
            lens_version: 1,
            semantic_registry_version: "remote-evaluation".into(),
            analyzer_version: None,
            embedding_model_version: None,
            graph_schema_version: None,
            source_snapshot: proof.fingerprint(),
            resource_count: self.resources.len() as u64,
            relation_count: Some(0),
            coverage,
            digest: format!("sha256:{hex}"),
            built_at: OffsetDateTime::now_utc(),
        };
        let access_model = registration.discoverable_source().access_model;
        let mut resources = BTreeMap::new();
        let mut identities = BTreeMap::new();
        for (id, staged) in self.resources {
            let projection = CompiledResourceProjection {
                manifest: manifest.clone(),
                retention_mode: registration.retention_mode(),
                directory: DirectoryProjection {
                    resource_ref: id,
                    resource_version: None,
                    kind: staged.kind.unwrap_or(default_kind),
                    canonical_name: staged
                        .title
                        .clone()
                        .unwrap_or_else(|| "remote resource".into()),
                    title: staged.title.clone(),
                    aliases: vec![],
                },
                structured: StructuredProjection {
                    resource_ref: id,
                    concept_refs: vec![],
                    high_signal_facets: BTreeMap::new(),
                    typed_facets: staged
                        .fields
                        .iter()
                        .map(|(name, field)| (name.clone(), FacetState::Known(field.value.clone())))
                        .collect(),
                    assertions: vec![],
                    authority_resolutions: BTreeMap::new(),
                },
                temporal: TemporalProjection {
                    resource_ref: id,
                    valid_from: None,
                    valid_to: None,
                    profile: TemporalDiscoveryProfile::default(),
                },
                access: AccessProjection {
                    resource_ref: id,
                    access_scope: None,
                    source_access_model: access_model.clone(),
                },
                relations: vec![],
            };
            resources.insert(id, projection);
            identities.insert(id, staged);
        }
        let mut assertions: BTreeMap<ResourceId, Vec<Assertion>> = BTreeMap::new();
        let mut evidence = BTreeMap::new();
        for ((id, field), (label, verified)) in self.verified {
            let Some(value) = identities
                .get(&id)
                .and_then(|staged: &StagedResource| staged.fields.get(&field))
            else {
                continue;
            };
            let mut assertion = Assertion::new(
                REMOTE_CLAIM_SUBJECT,
                field.clone(),
                value.value.clone(),
                source.as_uuid().to_string(),
                verified.assertion_origin(),
                registration.canonical_upstream_lineage(),
                proof.observed_at(),
            );
            assertion.evidence_refs = vec![label.clone()];
            assertions.entry(id).or_default().push(assertion);
            evidence.insert((id, label), verified);
        }
        let trace = format!("{}:{}", source.as_uuid(), key.generation_id.as_uuid());
        let lists = self
            .lists
            .into_iter()
            .map(|(retriever, operation, ids)| {
                let method = match operation {
                    RemoteOperationKind::Enumerate => "remote_enumeration",
                    RemoteOperationKind::Query => "remote_query",
                    RemoteOperationKind::Lookup => "direct_address",
                    RemoteOperationKind::Live => "live_only",
                };
                let candidates = ids
                    .into_iter()
                    .map(|id| {
                        let mut candidate = FederatedCandidate::new(
                            remote_candidate_id(source, id),
                            CandidateIdentityClass::RemoteStableReference,
                            source,
                            method,
                        );
                        candidate.resource_ref = Some(id);
                        candidate.retrieval_trace_ref = Some(trace.clone());
                        candidate
                    })
                    .collect();
                (retriever, candidates)
            })
            .collect();
        Ok(RemoteEvaluationGeneration {
            key,
            owner: self.lease,
            context: self.context,
            proof,
            manifest,
            receipts: self.receipts,
            resources,
            identities,
            assertions,
            evidence,
            lists,
            gaps: self.gaps,
        })
    }
}

/// Immutable after seal. Reads leave only through the lease-guarded view.
pub struct RemoteEvaluationGeneration {
    key: ProjectionGenerationKey,
    owner: EvaluationLeaseId,
    context: TrustedRemoteContext,
    proof: SourceSnapshotProof,
    manifest: ProjectionGenerationManifest,
    receipts: Vec<ActionReceipt>,
    resources: BTreeMap<ResourceId, CompiledResourceProjection>,
    identities: BTreeMap<ResourceId, StagedResource>,
    assertions: BTreeMap<ResourceId, Vec<Assertion>>,
    evidence: BTreeMap<(ResourceId, String), VerifiedProvenance>,
    lists: Vec<(String, Vec<FederatedCandidate>)>,
    gaps: Vec<InformationGap>,
}

impl fmt::Debug for RemoteEvaluationGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteEvaluationGeneration(<lease-owned>)")
    }
}

impl RemoteEvaluationGeneration {
    pub const fn key(&self) -> ProjectionGenerationKey {
        self.key
    }
    pub fn owner(&self) -> &EvaluationLeaseId {
        &self.owner
    }
    pub fn context(&self) -> &TrustedRemoteContext {
        &self.context
    }
    pub fn snapshot(&self) -> &SourceSnapshotProof {
        &self.proof
    }
    pub fn receipts(&self) -> &[ActionReceipt] {
        &self.receipts
    }
    pub fn gaps(&self) -> &[InformationGap] {
        &self.gaps
    }
    pub fn resource_ids(&self) -> BTreeSet<ResourceId> {
        self.resources.keys().copied().collect()
    }
    /// Owned copies of one retriever list, in provider rank order.
    pub fn candidates(&self, retriever_id: &str) -> Option<Vec<FederatedCandidate>> {
        self.lists
            .iter()
            .find(|(retriever, _)| retriever == retriever_id)
            .map(|(_, candidates)| candidates.clone())
    }
    pub(crate) fn projection(&self, id: ResourceId) -> Option<&CompiledResourceProjection> {
        self.resources.get(&id)
    }
    pub(crate) fn manifest(&self) -> &ProjectionGenerationManifest {
        &self.manifest
    }
    pub(crate) fn assertions(&self, id: ResourceId, predicate: &str) -> Vec<Assertion> {
        self.assertions
            .get(&id)
            .map(|items| {
                items
                    .iter()
                    .filter(|assertion| assertion.predicate == predicate)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn evidence(
        &self,
        id: ResourceId,
        evidence_ref: &str,
    ) -> Option<&VerifiedProvenance> {
        self.evidence.get(&(id, evidence_ref.to_owned()))
    }
    pub(crate) fn identity(&self, id: ResourceId) -> Option<&StagedResource> {
        self.identities.get(&id)
    }

    /// The only route from a sealed remote projection to durable state: the
    /// registration's retention, the current Source policy and server-owned
    /// field proofs must all permit it. A Resource not in this seal is a
    /// Source mismatch.
    pub fn persistable_projection(
        &self,
        resource: ResourceId,
        current_policy: &crate::ports::CurrentSourcePolicy,
        proofs: &crate::projection::RemoteFieldProofs,
    ) -> Result<crate::projection::PersistableResourceProjection, crate::projection::ProjectionError>
    {
        let projection = self
            .resources
            .get(&resource)
            .cloned()
            .ok_or(crate::projection::ProjectionError::SourceMismatch)?;
        crate::projection::VerifiedPersistentProjection::try_from_remote(
            projection,
            self.context.registration(),
            current_policy,
            proofs,
        )
    }
}
