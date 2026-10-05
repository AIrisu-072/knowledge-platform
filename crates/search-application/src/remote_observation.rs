//! Checked remote observations and private, exact-ID absence receipts.
//!
//! Proofs record what the trusted adapter verified at observation time. They
//! do not grant future access; execution/disclosure must recheck current access.

use std::collections::BTreeSet;
use std::fmt;

use search_core::observation::{Coverage, Presence};
use search_core::source::EnumerationSemantics;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::SearchError;
use crate::ports::BoxFuture;
use crate::remote::{
    PlannedRemoteAction, RemoteActionOutcome, RemoteActionResponse, RemoteOperation,
    RemoteOperationKind, RemotePage, RemoteResponseInput, RemoteResponseStatus,
    RemoteUnknownReason, TrustedRemoteContext, invalid_remote,
};
use crate::remote_registration::RemoteSourceRegistration;
use crate::retrieval::OpaqueNativeId;
use crate::scoped::{AccessContextAuthorityPort, CurrentSourceVisibilityPort};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotExtent {
    /// Adapter verification covers the Source's complete authorized collection;
    /// the server registration must independently permit complete enumeration.
    CompleteSource,
    PartialSource,
    /// A response fingerprint does not establish a reusable Source snapshot.
    SingleResponse,
}

/// A descriptor returned by the host-configured verifier, never by a provider
/// DTO conversion. A token is meaningful only after the verifier has checked
/// the fixed Source protocol, scope/revisions and snapshot integrity. Known IDs
/// come from the adapter's verified Source inventory, not provider totals.
#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotAttestation {
    token: String,
    extent: SnapshotExtent,
    observed_at: OffsetDateTime,
    known_native_ids: Vec<OpaqueNativeId>,
}
impl fmt::Debug for SnapshotAttestation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SnapshotAttestation(<opaque>)")
    }
}
impl SnapshotAttestation {
    pub fn new(
        token: impl Into<String>,
        extent: SnapshotExtent,
        observed_at: OffsetDateTime,
        known_native_ids: Vec<OpaqueNativeId>,
    ) -> Result<Self, SearchError> {
        let token = token.into();
        if token.is_empty()
            || token.len() > 1024
            || token.chars().any(char::is_control)
            || observed_at > OffsetDateTime::now_utc()
            || known_native_ids.len() > 3200
            || known_native_ids
                .iter()
                .map(OpaqueNativeId::as_str)
                .collect::<BTreeSet<_>>()
                .len()
                != known_native_ids.len()
        {
            return Err(invalid_remote());
        }
        Ok(Self {
            token,
            extent,
            observed_at,
            known_native_ids,
        })
    }
}

/// Host trust boundary for protocol/snapshot verification only. The host wires
/// this verifier; caller/provider data cannot choose it. It must not issue
/// actor/Source access, mutate registrations, or infer completeness from totals.
/// Returning None leaves the result Unknown. A production transport verifier
/// is deliberately not supplied by this pure contract slice.
pub trait RemoteSnapshotVerifierPort: Send + Sync {
    fn verify<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        action: &'a PlannedRemoteAction,
        input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>>;
}

/// Not deserializable and no public constructor: only checked adapter output.
/// The full actor, session, evaluation and registration activation are bound.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceSnapshotProof {
    context: TrustedRemoteContext,
    token_digest: [u8; 32],
    extent: SnapshotExtent,
    observed_at: OffsetDateTime,
    known_native_ids: Vec<OpaqueNativeId>,
}
impl fmt::Debug for SourceSnapshotProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceSnapshotProof(<opaque>)")
    }
}
impl SourceSnapshotProof {
    pub fn matches_context(&self, context: &TrustedRemoteContext) -> bool {
        &self.context == context && context.binding().actor().is_live()
    }
    pub const fn observed_at(&self) -> OffsetDateTime {
        self.observed_at
    }
    pub const fn extent(&self) -> SnapshotExtent {
        self.extent
    }
    /// Opaque fingerprint of the verified token, safe for a manifest field.
    pub(crate) fn fingerprint(&self) -> String {
        let hex: String = self
            .token_digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("remote-snapshot:sha256:{hex}")
    }

    /// Observations may share a seal only when a Source snapshot was verified.
    /// Single-response fingerprints deliberately never prove a shared snapshot.
    pub fn same_source_snapshot(&self, other: &Self) -> bool {
        self.context == other.context
            && self.token_digest == other.token_digest
            && self.extent == other.extent
            && self.extent != SnapshotExtent::SingleResponse
    }
}

pub struct CheckedRemoteObservationAdapter<'a> {
    registration: &'a RemoteSourceRegistration,
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
    verifier: Option<&'a dyn RemoteSnapshotVerifierPort>,
}
impl<'a> CheckedRemoteObservationAdapter<'a> {
    pub fn new(
        registration: &'a RemoteSourceRegistration,
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
        verifier: &'a dyn RemoteSnapshotVerifierPort,
    ) -> Self {
        Self {
            registration,
            authority,
            visibility,
            verifier: Some(verifier),
        }
    }
    pub fn unwired(
        registration: &'a RemoteSourceRegistration,
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
    ) -> Self {
        Self {
            registration,
            authority,
            visibility,
            verifier: None,
        }
    }

    pub async fn observe(
        &self,
        context: &TrustedRemoteContext,
        action: &PlannedRemoteAction,
        input: RemoteResponseInput,
    ) -> Result<RemoteActionOutcome, SearchError> {
        context
            .check_current(self.registration, self.authority, self.visibility)
            .await?;
        if !action.matches_context(context) {
            return Err(invalid_remote());
        }
        let unknown = |reason| RemoteActionOutcome::Unknown {
            retriever_id: action.retriever_id().into(),
            operation: action.operation().kind(),
            reason,
        };
        let reason = match input.status {
            RemoteResponseStatus::Success => None,
            RemoteResponseStatus::Partial => Some(RemoteUnknownReason::Partial),
            RemoteResponseStatus::NotFound => Some(RemoteUnknownReason::NotFound),
            RemoteResponseStatus::Forbidden => Some(RemoteUnknownReason::Denied),
            RemoteResponseStatus::Timeout => Some(RemoteUnknownReason::Timeout),
            RemoteResponseStatus::Unavailable => Some(RemoteUnknownReason::Unavailable),
            RemoteResponseStatus::Malformed => Some(RemoteUnknownReason::Malformed),
        };
        if let Some(reason) = reason {
            return Ok(unknown(reason));
        }
        let limits = self.registration.limits();
        if input.hits.len() > limits.max_hits_per_page
            || input.hits.iter().any(|hit| {
                hit.native_id()
                    .is_some_and(|id| id.as_str().len() > limits.max_native_id_bytes)
            })
        {
            return Ok(unknown(RemoteUnknownReason::Malformed));
        }
        let coverage = match (action.operation(), &input.page) {
            (
                RemoteOperation::Enumerate { .. },
                RemotePage::Enumeration {
                    requested,
                    next,
                    terminal,
                },
            ) => {
                if *terminal != next.is_none()
                    || requested
                        .as_ref()
                        .is_some_and(|v| v.as_str().len() > limits.max_cursor_bytes)
                    || next
                        .as_ref()
                        .is_some_and(|v| v.as_str().len() > limits.max_cursor_bytes)
                    || (next.is_some() && requested == next)
                {
                    return Ok(unknown(RemoteUnknownReason::Malformed));
                }
                // No individual page claims complete enumeration.
                Coverage::PartialEnumeration
            }
            (RemoteOperation::Query { .. }, RemotePage::Unpaged) => Coverage::QueryResult,
            (RemoteOperation::Lookup { .. }, RemotePage::Unpaged) => Coverage::DirectLookup,
            (RemoteOperation::Live { input }, RemotePage::Unpaged) => {
                if input.native_id().is_some() {
                    Coverage::DirectLookup
                } else {
                    Coverage::QueryResult
                }
            }
            _ => return Ok(unknown(RemoteUnknownReason::Malformed)),
        };
        let exact_id = match action.operation() {
            RemoteOperation::Lookup { native_id } => Some(native_id),
            RemoteOperation::Live { input } => input.native_id(),
            _ => None,
        };
        if exact_id.is_some_and(|id| {
            input.hits.len() > 1 || input.hits.iter().any(|hit| hit.native_id() != Some(id))
        }) {
            return Ok(unknown(RemoteUnknownReason::Malformed));
        }
        let Some(verifier) = self.verifier else {
            return Ok(unknown(RemoteUnknownReason::UnverifiedSnapshot));
        };
        let attestation = verifier.verify(context, action, &input).await?;
        context
            .check_current(self.registration, self.authority, self.visibility)
            .await?;
        let Some(attestation) = attestation else {
            return Ok(unknown(RemoteUnknownReason::UnverifiedSnapshot));
        };
        if attestation.known_native_ids.len() > limits.max_hits
            || attestation
                .known_native_ids
                .iter()
                .any(|id| id.as_str().len() > limits.max_native_id_bytes)
            || (action.operation().kind() == RemoteOperationKind::Enumerate
                && attestation.extent == SnapshotExtent::CompleteSource
                && self.registration.enumeration_semantics() != EnumerationSemantics::Complete)
        {
            return Ok(unknown(RemoteUnknownReason::UnverifiedSnapshot));
        }
        let proof = SourceSnapshotProof {
            context: context.clone(),
            token_digest: Sha256::digest(attestation.token.as_bytes()).into(),
            extent: attestation.extent,
            observed_at: attestation.observed_at,
            known_native_ids: attestation.known_native_ids,
        };
        Ok(RemoteActionOutcome::Completed(Box::new(
            RemoteActionResponse {
                action: action.clone(),
                proof,
                coverage,
                page: input.page,
                hits: input.hits,
            },
        )))
    }

    /// Current access is checked around the pure historical proof validator.
    /// Recheck again when consuming this receipt after any later await.
    pub async fn verify_absence(
        &self,
        context: &TrustedRemoteContext,
        pages: &[RemoteActionResponse],
        native_id: &OpaqueNativeId,
    ) -> Result<Option<VerifiedAbsence>, SearchError> {
        context
            .check_current(self.registration, self.authority, self.visibility)
            .await?;
        let receipt = verify_absence(self.registration, context, pages, native_id);
        context
            .check_current(self.registration, self.authority, self.visibility)
            .await?;
        Ok(receipt)
    }
}

/// Historical observation proof, not a Resource deletion or access grant.
///
/// ```compile_fail
/// use search_application::remote_observation::VerifiedAbsence;
/// let receipt = VerifiedAbsence {};
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedAbsence {
    proof: SourceSnapshotProof,
    native_id: OpaqueNativeId,
}
impl fmt::Debug for VerifiedAbsence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedAbsence(<opaque>)")
    }
}
impl VerifiedAbsence {
    pub const fn presence(&self) -> Presence {
        Presence::Absent
    }
    pub const fn coverage(&self) -> Coverage {
        Coverage::CompleteEnumeration
    }
    pub fn native_id(&self) -> &OpaqueNativeId {
        &self.native_id
    }
    pub fn snapshot_proof(&self) -> &SourceSnapshotProof {
        &self.proof
    }
}

/// Verify a complete, bounded sequence for a known exact ID. This pure function
/// validates the observation-time context, not a new current-access grant.
/// Current consumers use the checked adapter method and their final access gate.
/// Direct absence remains unqualified: the current registration has no explicit
/// authoritative ACL-unmasked lookup grant. Predicate labels are not that grant.
pub fn verify_absence(
    registration: &RemoteSourceRegistration,
    context: &TrustedRemoteContext,
    pages: &[RemoteActionResponse],
    native_id: &OpaqueNativeId,
) -> Option<VerifiedAbsence> {
    if !context.matches_registration(registration)
        || registration.enumeration_semantics() != EnumerationSemantics::Complete
        || pages.is_empty()
        || pages.len() > registration.limits().max_pages_or_requests
    {
        return None;
    }
    let first = pages.first()?;
    if first.proof.extent != SnapshotExtent::CompleteSource
        || !first.proof.known_native_ids.contains(native_id)
        || !matches!(
            first.action.operation(),
            RemoteOperation::Enumerate { cursor: None }
        )
    {
        return None;
    }
    let mut expected_cursor = None;
    let mut seen = BTreeSet::new();
    let mut hits = 0usize;
    let mut native_ids = BTreeSet::new();
    for (index, page) in pages.iter().enumerate() {
        if !page.action.matches_context(context)
            || page.action != first.action
            || !page.proof.matches_context(context)
            || !page.proof.same_source_snapshot(&first.proof)
            || page.coverage != Coverage::PartialEnumeration
            || !page.proof.known_native_ids.contains(native_id)
            || page
                .hits
                .iter()
                .any(|hit| hit.native_id() == Some(native_id))
        {
            return None;
        }
        for hit in &page.hits {
            if !native_ids.insert(hit.native_id()?.as_str()) {
                return None;
            }
        }
        hits = hits.checked_add(page.hits.len())?;
        if hits > registration.limits().max_hits {
            return None;
        }
        let RemotePage::Enumeration {
            requested,
            next,
            terminal,
        } = &page.page
        else {
            return None;
        };
        if requested.as_ref() != expected_cursor
            || *terminal != (index + 1 == pages.len())
            || *terminal != next.is_none()
        {
            return None;
        }
        if let Some(cursor) = next
            && !seen.insert(cursor.as_str())
        {
            return None;
        }
        expected_cursor = next.as_ref();
    }
    Some(VerifiedAbsence {
        proof: pages.last()?.proof.clone(),
        native_id: native_id.clone(),
    })
}
