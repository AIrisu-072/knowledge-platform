//! P4-14: binding and revalidating a qualified remote Resource.
//!
//! A remote representation binding is made only for a candidate of the
//! actor's own sealed evaluation generation and a target pinned from that
//! same snapshot: stable ID plus the sealed version/digest. A version pin
//! needs a version, a snapshot or live binding needs a version or digest,
//! and a live reference always requires current revalidation. Revalidation
//! reads only through the fixed Source's registered adapter with the pinned
//! identity (never a provider locator), rechecks actor/Source/item/policy
//! and budget before the read and again before returning, and reports a
//! changed version or digest as a target that needs a new qualification.
//! Nothing is appended to the sealed generation.

use search_core::binding::{BindingMode, RepresentationBinding};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::id::{BindingId, LogicalResourceId, RepresentationId};
use search_core::materialization::MaterializationState;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::SearchError;
use crate::materialization::MaterializationBudget;
use crate::ports::AccessDecision;
use crate::remote::{
    PinnedRemoteTarget, RemoteAccessTarget, RemoteReadOutcome, RemoteSourcePort,
    RemoteUnknownReason, TrustedRemoteContext,
};
use crate::remote_generation::RemoteEvaluationGeneration;
use crate::remote_identity::remote_version_id;
use crate::scoped::{AccessContextAuthorityPort, CurrentSourceVisibilityPort};

fn refused() -> SearchError {
    SearchError::InvalidRequest("remote binding is not permitted".into())
}

fn denied() -> SearchError {
    SearchError::InvalidRequest("remote read is not currently permitted".into())
}

pub struct RemoteBindingService<'a> {
    port: &'a dyn RemoteSourcePort,
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
}

impl<'a> RemoteBindingService<'a> {
    pub fn new(
        port: &'a dyn RemoteSourcePort,
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
    ) -> Self {
        Self {
            port,
            authority,
            visibility,
        }
    }

    /// A live reference to a qualified remote candidate.
    pub fn bind_live(
        &self,
        scope: &TrustedRemoteContext,
        generation: &RemoteEvaluationGeneration,
        candidate: &FederatedCandidate,
        target: &PinnedRemoteTarget,
    ) -> Result<RepresentationBinding, SearchError> {
        self.bind(
            scope,
            generation,
            candidate,
            target,
            BindingMode::LiveReference,
        )
    }

    pub fn bind(
        &self,
        scope: &TrustedRemoteContext,
        generation: &RemoteEvaluationGeneration,
        candidate: &FederatedCandidate,
        target: &PinnedRemoteTarget,
        mode: BindingMode,
    ) -> Result<RepresentationBinding, SearchError> {
        let key = generation.key();
        let resource = candidate.resource_ref.ok_or_else(refused)?;
        if generation.context() != scope
            || candidate.source_ref != key.source_id
            || candidate.identity_class != CandidateIdentityClass::RemoteStableReference
            || candidate.retrieval_trace_ref.as_deref()
                != Some(
                    format!(
                        "{}:{}",
                        key.source_id.as_uuid(),
                        key.generation_id.as_uuid()
                    )
                    .as_str(),
                )
            || target.identity().source_scope() != scope.source_scope()
            || !target
                .snapshot()
                .same_source_snapshot(generation.snapshot())
        {
            return Err(refused());
        }
        // The target must be exactly what this generation sealed.
        let staged = generation.identity(resource).ok_or_else(refused)?;
        if &staged.native_id != target.identity().native_id()
            || staged.version.as_deref() != target.version()
            || staged.digest.as_deref() != target.digest()
            || (target.version().is_none() && target.digest().is_none())
            || mode == BindingMode::SessionSnapshot
        {
            return Err(refused());
        }
        let mut binding = RepresentationBinding::new(
            BindingId::from_uuid(Uuid::now_v7()),
            LogicalResourceId::from_uuid(resource.as_uuid()),
            RepresentationId::from_uuid(resource.as_uuid()),
            key.source_id,
            mode,
            OffsetDateTime::now_utc(),
        );
        binding.resource_version_ref = target
            .version()
            .map(|version| remote_version_id(resource, version))
            .transpose()?;
        binding.content_digest = target.digest().map(str::to_owned);
        binding.provider_ref = Some(scope.registration().provider_kind().to_owned());
        binding.validate().map_err(|_| refused())?;
        Ok(binding)
    }

    /// Reads the pinned target through the registered adapter only. A changed
    /// version or digest is `SnapshotIncompatible`: the caller must run a new
    /// evaluation and qualification instead of rebinding silently.
    pub async fn revalidate_target(
        &self,
        scope: &TrustedRemoteContext,
        target: &PinnedRemoteTarget,
        requested_state: MaterializationState,
        budget: &MaterializationBudget,
    ) -> Result<RemoteReadOutcome, SearchError> {
        let content = matches!(
            requested_state,
            MaterializationState::Fragment | MaterializationState::FullContent
        );
        if target.identity().source_scope() != scope.source_scope()
            || budget.max_remote_calls == 0
            || budget.max_latency_ms == 0
            || (content && budget.max_content_bytes == 0)
        {
            return Err(denied());
        }
        self.current(scope, target, requested_state).await?;
        let outcome = self
            .port
            .probe_or_materialize(scope, target, requested_state)
            .await?;
        let RemoteReadOutcome::Observed {
            target: observed,
            state,
        } = outcome
        else {
            return Ok(outcome);
        };
        if observed.identity() != target.identity() || !state.can_advance_to(requested_state) {
            return Err(SearchError::OperationFailed(
                "remote read outcome is inconsistent".into(),
            ));
        }
        if observed.version() != target.version() || observed.digest() != target.digest() {
            return Ok(RemoteReadOutcome::Unknown(
                RemoteUnknownReason::SnapshotIncompatible,
            ));
        }
        // Access may have changed during the read.
        self.current(scope, target, requested_state).await?;
        Ok(RemoteReadOutcome::Observed {
            target: observed,
            state,
        })
    }

    async fn current(
        &self,
        scope: &TrustedRemoteContext,
        target: &PinnedRemoteTarget,
        requested_state: MaterializationState,
    ) -> Result<(), SearchError> {
        scope
            .check_current(scope.registration(), self.authority, self.visibility)
            .await
            .map_err(|_| denied())?;
        if self
            .port
            .current_access(
                scope,
                &RemoteAccessTarget::Resource(target.identity().clone()),
            )
            .await?
            != AccessDecision::Allowed
        {
            return Err(denied());
        }
        let policy = self.port.current_policy(scope, target.identity()).await?;
        if !policy.provider_permission.permits(requested_state)
            || policy.retention_mode != scope.registration().retention_mode()
        {
            return Err(denied());
        }
        Ok(())
    }
}
