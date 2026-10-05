//! `RemoteSourcePort`, provenance lookup and remote candidate access over
//! the guarded transport.
//!
//! Each planned action becomes one request to a fixed path (an enumeration
//! sweep, one request per page); its decoded, untrusted input goes through
//! the application's checked observation adapter, which rechecks
//! actor/Source currency and turns the provider's snapshot claim into a
//! proof. Transport, status and decode failures are low-cardinality
//! `Unknown` outcomes, never absence. List and content requests per
//! evaluation are bounded by the registration; current item access is asked
//! of the Source's `/authorize` every time and is never cached.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentCandidateAccessEvaluatorPort, CurrentSourcePolicy,
};
use search_application::remote::{
    OpaqueCursor, PinnedRemoteTarget, PlannedRemoteAction, RemoteAccessTarget, RemoteActionOutcome,
    RemoteIdentity, RemoteOperation, RemotePage, RemoteReadOutcome, RemoteResponseInput,
    RemoteResponseStatus, RemoteSourcePort, RemoteUnknownReason, TrustedRemoteContext,
    UntrustedRemoteHit, validate_remote_batch,
};
use search_application::remote_evidence::{RemoteProvenanceLookupPort, VerifiedSourceProvenance};
use search_application::remote_identity::remote_resource_id;
use search_application::remote_observation::{
    CheckedRemoteObservationAdapter, RemoteSnapshotVerifierPort, SnapshotAttestation,
    SnapshotExtent,
};
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::retrieval::OpaqueNativeId;
use search_application::scoped::{
    AccessContextAuthorityPort, AuthorizedSourceScope, CurrentSourceVisibilityPort,
};
use search_core::discovery::FederatedCandidate;
use search_core::id::{DiscoveryEvaluationId, ResourceId};
use search_core::materialization::MaterializationState;
use time::OffsetDateTime;

use crate::protocol::{
    DecodedAuthorization, DecodedContent, DecodedSnapshot, authorize_body, decode_authorize,
    decode_content, decode_response, live_body, lookup_body, search_body,
};
use crate::transport::{GuardedHttpTransport, RegisteredPath, TransportError};

/// Evaluations remembered at once for request budgets and item identities.
const TRACKED_EVALUATIONS: usize = 256;

/// The protocol verifier for one decoded response: the provider's snapshot
/// token, extent and inventory, observed now. The application decides what
/// they prove.
struct ProtocolSnapshot(DecodedSnapshot);

impl RemoteSnapshotVerifierPort for ProtocolSnapshot {
    fn verify<'a>(
        &'a self,
        _context: &'a TrustedRemoteContext,
        _action: &'a PlannedRemoteAction,
        _input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            Ok(Some(SnapshotAttestation::new(
                self.0.token.clone(),
                self.0.extent,
                OffsetDateTime::now_utc(),
                self.0.known.clone(),
            )?))
        })
    }
}

fn reason(error: TransportError) -> RemoteUnknownReason {
    match error {
        TransportError::HttpStatus(401 | 403) => RemoteUnknownReason::Denied,
        TransportError::HttpStatus(404) => RemoteUnknownReason::NotFound,
        TransportError::Timeout | TransportError::DeadlineExceeded => RemoteUnknownReason::Timeout,
        TransportError::ContentLengthExceeded
        | TransportError::DecodedLimitExceeded
        | TransportError::Redirect => RemoteUnknownReason::Malformed,
        _ => RemoteUnknownReason::Unavailable,
    }
}

/// Per-evaluation request count and the native IDs this adapter observed,
/// so a candidate's current access can be asked of the Source.
struct Evaluation {
    context: TrustedRemoteContext,
    requests: usize,
    natives: BTreeMap<ResourceId, OpaqueNativeId>,
}

pub struct HttpRemoteSourceAdapter<'a> {
    registration: RemoteSourceRegistration,
    transport: GuardedHttpTransport,
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
    evaluations: Mutex<BTreeMap<DiscoveryEvaluationId, Evaluation>>,
}

impl fmt::Debug for HttpRemoteSourceAdapter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpRemoteSourceAdapter(<registered>)")
    }
}

impl<'a> HttpRemoteSourceAdapter<'a> {
    pub fn new(
        registration: RemoteSourceRegistration,
        transport: GuardedHttpTransport,
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
    ) -> Result<Self, SearchError> {
        let limits = transport.limits();
        let registered = registration.limits();
        if limits.max_request_bytes > registered.max_request_bytes
            || limits.max_decoded_response_bytes > registered.max_decoded_response_bytes
            || limits.call_timeout > Duration::from_millis(registered.call_millis)
        {
            return Err(SearchError::InvalidRequest(
                "transport limits exceed the registration".into(),
            ));
        }
        Ok(Self {
            registration,
            transport,
            authority,
            visibility,
            evaluations: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn registration(&self) -> &RemoteSourceRegistration {
        &self.registration
    }

    fn owns(&self, context: &TrustedRemoteContext) -> bool {
        context.registration() == &self.registration
    }

    /// Runs `change` on the evaluation's entry, opening it if needed.
    fn with_evaluation<T>(
        &self,
        context: &TrustedRemoteContext,
        change: impl FnOnce(&mut Evaluation) -> T,
    ) -> Option<T> {
        let mut evaluations = self.evaluations.lock().ok()?;
        let key = context.binding().evaluation();
        // At capacity a new evaluation is refused (Unknown), never admitted
        // by evicting a running one and resetting its budget.
        if !evaluations.contains_key(&key) && evaluations.len() >= TRACKED_EVALUATIONS {
            return None;
        }
        let entry = evaluations.entry(key).or_insert_with(|| Evaluation {
            context: context.clone(),
            requests: 0,
            natives: BTreeMap::new(),
        });
        (entry.context == *context).then(|| change(entry))
    }

    /// One request; list/content requests count against the evaluation's
    /// page/request budget.
    async fn call(
        &self,
        context: &TrustedRemoteContext,
        path: RegisteredPath,
        body: Vec<u8>,
        deadline: Instant,
        budgeted: bool,
    ) -> Result<Vec<u8>, RemoteUnknownReason> {
        if budgeted {
            let limit = self.registration.limits().max_pages_or_requests;
            let admitted = self
                .with_evaluation(context, |evaluation| {
                    let admitted = evaluation.requests < limit;
                    evaluation.requests += usize::from(admitted);
                    admitted
                })
                .unwrap_or(false);
            if !admitted {
                return Err(RemoteUnknownReason::Unavailable);
            }
        }
        let response = self
            .transport
            .request(&path, &body, deadline)
            .await
            .map_err(reason)?;
        Ok(response.as_bytes().to_vec())
    }

    fn call_deadline(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.registration.limits().call_millis)
    }

    /// One page or list for `action`; `page` overrides an enumeration cursor
    /// within the same sweep action.
    async fn observe(
        &self,
        context: &TrustedRemoteContext,
        action: &PlannedRemoteAction,
        page: Option<&OpaqueCursor>,
        deadline: Instant,
    ) -> Result<RemoteActionOutcome, SearchError> {
        let operation = action.operation();
        let unknown = |reason| RemoteActionOutcome::Unknown {
            retriever_id: action.retriever_id().into(),
            operation: operation.kind(),
            reason,
        };
        let (path, body) = match operation {
            RemoteOperation::Enumerate { cursor } => (
                RegisteredPath::catalog(page.or(cursor.as_ref()).map(OpaqueCursor::as_str)),
                Vec::new(),
            ),
            RemoteOperation::Query { input } => (RegisteredPath::search(), search_body(input)),
            RemoteOperation::Lookup { native_id } => {
                (RegisteredPath::lookup(), lookup_body(native_id))
            }
            RemoteOperation::Live { input } => (RegisteredPath::live(), live_body(input)),
        };
        let bytes = match self.call(context, path, body, deadline, true).await {
            Ok(bytes) => bytes,
            Err(reason) => return Ok(unknown(reason)),
        };
        let decoded = decode_response(operation.kind(), &bytes, &self.registration);
        drop(bytes);
        let Ok(decoded) = decoded else {
            return Ok(unknown(RemoteUnknownReason::Malformed));
        };
        let verifier = ProtocolSnapshot(decoded.snapshot);
        let outcome = CheckedRemoteObservationAdapter::new(
            &self.registration,
            self.authority,
            self.visibility,
            &verifier,
        )
        .observe(context, action, decoded.input)
        .await?;
        if let RemoteActionOutcome::Completed(response) = &outcome {
            self.remember(context, response.hits());
        }
        Ok(outcome)
    }

    fn remember(&self, context: &TrustedRemoteContext, hits: &[UntrustedRemoteHit]) {
        let registration = &self.registration;
        let observed: Vec<_> = hits
            .iter()
            .filter_map(|hit| hit.native_id())
            .filter_map(|native| {
                remote_resource_id(
                    registration.tenant(),
                    registration.source_id(),
                    registration.provider_kind(),
                    native,
                )
                .ok()
                .map(|id| (id, native.clone()))
            })
            .collect();
        self.with_evaluation(context, |evaluation| evaluation.natives.extend(observed));
    }

    /// The whole enumeration sweep of one `Enumerate { cursor: None }` action:
    /// every page observed under that same action until the terminal page, a
    /// failure, or the page budget. Only such a sweep can support absence.
    pub async fn sweep(
        &self,
        context: &TrustedRemoteContext,
        action: &PlannedRemoteAction,
    ) -> Result<Vec<RemoteActionOutcome>, SearchError> {
        if !self.owns(context)
            || !matches!(
                action.operation(),
                RemoteOperation::Enumerate { cursor: None }
            )
        {
            return Err(SearchError::InvalidRequest(
                "a sweep starts from an unpaged enumeration".into(),
            ));
        }
        let deadline =
            Instant::now() + Duration::from_millis(self.registration.limits().evaluation_millis);
        let mut cursor: Option<OpaqueCursor> = None;
        let mut pages = Vec::new();
        for _ in 0..self.registration.limits().max_pages_or_requests {
            let outcome = self
                .observe(context, action, cursor.as_ref(), deadline)
                .await?;
            let next = match &outcome {
                RemoteActionOutcome::Completed(response) => match response.page() {
                    RemotePage::Enumeration {
                        next: Some(next),
                        terminal: false,
                        ..
                    } => Some(next.clone()),
                    _ => None,
                },
                RemoteActionOutcome::Unknown { .. } => None,
            };
            pages.push(outcome);
            match next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(pages)
    }

    async fn content(
        &self,
        context: &TrustedRemoteContext,
        native_id: &str,
    ) -> Result<DecodedContent, RemoteUnknownReason> {
        let bytes = self
            .call(
                context,
                RegisteredPath::content(native_id),
                Vec::new(),
                self.call_deadline(),
                true,
            )
            .await?;
        let decoded = decode_content(&bytes, &self.registration);
        drop(bytes);
        let decoded = decoded.map_err(|_| RemoteUnknownReason::Malformed)?;
        if decoded.native_id.as_str() != native_id {
            return Err(RemoteUnknownReason::Malformed);
        }
        Ok(decoded)
    }

    async fn authorize(
        &self,
        context: &TrustedRemoteContext,
        target: Option<&RemoteIdentity>,
    ) -> Result<DecodedAuthorization, RemoteUnknownReason> {
        let bytes = self
            .call(
                context,
                RegisteredPath::authorize(),
                authorize_body(
                    context.binding().actor().principal().as_str(),
                    target.map(RemoteIdentity::native_id),
                ),
                self.call_deadline(),
                false,
            )
            .await?;
        decode_authorize(&bytes, &self.registration).map_err(|_| RemoteUnknownReason::Malformed)
    }

    /// The context of the evaluation whose binding issued `access_context`
    /// and whose responses contained `resource`.
    fn observed(
        &self,
        access_context: &str,
        resource: ResourceId,
    ) -> Option<(TrustedRemoteContext, OpaqueNativeId)> {
        let evaluations = self.evaluations.lock().ok()?;
        evaluations.values().find_map(|evaluation| {
            let handle = evaluation
                .context
                .binding()
                .actor()
                .access_handle()
                .to_opaque_string();
            (handle == access_context)
                .then(|| evaluation.natives.get(&resource))
                .flatten()
                .map(|native| (evaluation.context.clone(), native.clone()))
        })
    }
}

impl RemoteSourcePort for HttpRemoteSourceAdapter<'_> {
    fn execute_batch<'b>(
        &'b self,
        context: &'b TrustedRemoteContext,
        actions: &'b [PlannedRemoteAction],
    ) -> BoxFuture<'b, Vec<RemoteActionOutcome>> {
        Box::pin(async move {
            if !self.owns(context) {
                return Err(SearchError::InvalidRequest(
                    "remote batch belongs to another registration".into(),
                ));
            }
            validate_remote_batch(context, actions)?;
            let deadline = Instant::now()
                + Duration::from_millis(self.registration.limits().evaluation_millis);
            let mut outcomes = Vec::with_capacity(actions.len());
            for action in actions {
                outcomes.push(self.observe(context, action, None, deadline).await?);
            }
            Ok(outcomes)
        })
    }

    fn current_access<'b>(
        &'b self,
        context: &'b TrustedRemoteContext,
        target: &'b RemoteAccessTarget,
    ) -> BoxFuture<'b, AccessDecision> {
        Box::pin(async move {
            if !self.owns(context) {
                return Ok(AccessDecision::Unknown);
            }
            let identity = match target {
                RemoteAccessTarget::SourceScope => None,
                RemoteAccessTarget::Resource(identity)
                    if identity.source_scope() == context.source_scope() =>
                {
                    Some(identity)
                }
                RemoteAccessTarget::Resource(_) => return Ok(AccessDecision::Unknown),
            };
            Ok(self
                .authorize(context, identity)
                .await
                .map_or(AccessDecision::Unknown, |authorization| {
                    authorization.decision
                }))
        })
    }

    fn current_policy<'b>(
        &'b self,
        context: &'b TrustedRemoteContext,
        target: &'b RemoteIdentity,
    ) -> BoxFuture<'b, CurrentSourcePolicy> {
        Box::pin(async move {
            let unknown =
                || SearchError::SourceUnavailable("current remote policy is unknown".into());
            // Nothing, not even the actor's principal, goes to this origin for
            // a context or target of another registration or scope.
            if !self.owns(context) || target.source_scope() != context.source_scope() {
                return Err(unknown());
            }
            // The item's own kind is not observed here: only a single-kind
            // registration can answer.
            let [resource_kind] = self.registration.allowed_resource_kinds() else {
                return Err(unknown());
            };
            let resource_kind = *resource_kind;
            let authorization = match self.authorize(context, Some(target)).await {
                Ok(authorization) if authorization.decision == AccessDecision::Allowed => {
                    authorization
                }
                _ => return Err(unknown()),
            };
            Ok(CurrentSourcePolicy {
                resource_kind,
                provider_permission: authorization.permission,
                retention_mode: self.registration.retention_mode(),
                probe_allowed: false,
            })
        })
    }

    fn probe_or_materialize<'b>(
        &'b self,
        context: &'b TrustedRemoteContext,
        target: &'b PinnedRemoteTarget,
        stage: MaterializationState,
    ) -> BoxFuture<'b, RemoteReadOutcome> {
        Box::pin(async move {
            if !self.owns(context) || target.identity().source_scope() != context.source_scope() {
                return Err(SearchError::InvalidRequest(
                    "remote target belongs to another Source scope".into(),
                ));
            }
            let native_id = target.identity().native_id();
            let content = match self.content(context, native_id.as_str()).await {
                Ok(content) => content,
                Err(reason) => return Ok(RemoteReadOutcome::Unknown(reason)),
            };
            // The observed identity/version/digest becomes a checked lookup
            // observation; the content body never leaves this call.
            let action = PlannedRemoteAction::new(
                context,
                "content",
                RemoteOperation::Lookup {
                    native_id: native_id.clone(),
                },
            )?;
            let input = RemoteResponseInput::new(
                RemoteResponseStatus::Success,
                RemotePage::Unpaged,
                vec![
                    UntrustedRemoteHit::new(
                        Some(content.native_id.clone()),
                        content.version.clone(),
                        content.digest.clone(),
                    )
                    .map_err(|_| {
                        SearchError::OperationFailed("remote content is malformed".into())
                    })?,
                ],
                None,
            )?;
            let verifier = ProtocolSnapshot(DecodedSnapshot {
                token: format!(
                    "content:{}",
                    OffsetDateTime::now_utc().unix_timestamp_nanos()
                ),
                extent: SnapshotExtent::SingleResponse,
                known: vec![],
            });
            let outcome = CheckedRemoteObservationAdapter::new(
                &self.registration,
                self.authority,
                self.visibility,
                &verifier,
            )
            .observe(context, &action, input)
            .await?;
            let RemoteActionOutcome::Completed(response) = outcome else {
                return Ok(RemoteReadOutcome::Unknown(RemoteUnknownReason::Unavailable));
            };
            let Ok(observed) = PinnedRemoteTarget::from_response(context, &response, native_id)
            else {
                // Neither version nor digest: the live target cannot be pinned.
                return Ok(RemoteReadOutcome::Unknown(RemoteUnknownReason::Malformed));
            };
            Ok(RemoteReadOutcome::Observed {
                target: Box::new(observed),
                state: stage,
            })
        })
    }
}

impl RemoteProvenanceLookupPort for HttpRemoteSourceAdapter<'_> {
    fn lookup<'b>(
        &'b self,
        scope: &'b AuthorizedSourceScope,
        target: &'b PinnedRemoteTarget,
        evidence_ref: &'b str,
    ) -> BoxFuture<'b, Option<VerifiedSourceProvenance>> {
        Box::pin(async move {
            let context = {
                let Ok(evaluations) = self.evaluations.lock() else {
                    return Ok(None);
                };
                evaluations
                    .get(&target.snapshot().evaluation())
                    .map(|evaluation| evaluation.context.clone())
            };
            let Some(context) = context.filter(|context| {
                context.source_scope() == scope && target.identity().source_scope() == scope
            }) else {
                return Ok(None);
            };
            // Provenance of an item is read only while the actor may read it.
            match self.authorize(&context, Some(target.identity())).await {
                Ok(authorization) if authorization.decision == AccessDecision::Allowed => {}
                _ => return Ok(None),
            }
            let Ok(content) = self
                .content(&context, target.identity().native_id().as_str())
                .await
            else {
                return Ok(None);
            };
            Ok(content
                .provenance
                .into_iter()
                .find(|record| record.evidence_ref == evidence_ref)
                .map(|record| VerifiedSourceProvenance {
                    direct: record.direct,
                    summary: record.summary,
                    version: content.version.clone(),
                    digest: content.digest.clone(),
                    lineage_label: record.lineage,
                    predicate: record.predicate,
                    citation_chain: record.citations,
                }))
        })
    }
}

/// Current item access of a remote candidate, asked of the Source's
/// `/authorize` for the identity this adapter observed in the actor's own
/// evaluation. Anything not observed that way is `Unknown`.
impl CurrentCandidateAccessEvaluatorPort for HttpRemoteSourceAdapter<'_> {
    fn evaluate<'b>(
        &'b self,
        candidate: &'b FederatedCandidate,
        access_context: &'b str,
    ) -> BoxFuture<'b, AccessDecision> {
        Box::pin(async move {
            let Some(resource) = candidate
                .resource_ref
                .filter(|_| candidate.source_ref == self.registration.source_id())
            else {
                return Ok(AccessDecision::Unknown);
            };
            let Some((context, native)) = self.observed(access_context, resource) else {
                return Ok(AccessDecision::Unknown);
            };
            let Ok(identity) = RemoteIdentity::new(&context, native) else {
                return Ok(AccessDecision::Unknown);
            };
            self.current_access(&context, &RemoteAccessTarget::Resource(identity))
                .await
        })
    }
}
