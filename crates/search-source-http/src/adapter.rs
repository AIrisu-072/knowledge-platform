//! `RemoteSourcePort` and provenance lookup over the guarded transport.
//!
//! Each planned action becomes one request to a fixed path; its decoded,
//! untrusted input goes through the application's checked observation
//! adapter, which rechecks actor/Source currency and turns the provider's
//! snapshot claim into a proof. Transport, status and decode failures are
//! low-cardinality `Unknown` outcomes, never absence. Requests per
//! evaluation are bounded by the registration.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use search_application::SearchError;
use search_application::ports::{AccessDecision, BoxFuture, CurrentSourcePolicy};
use search_application::remote::{
    PinnedRemoteTarget, PlannedRemoteAction, RemoteAccessTarget, RemoteActionOutcome,
    RemoteIdentity, RemoteOperation, RemotePage, RemoteReadOutcome, RemoteResponseInput,
    RemoteResponseStatus, RemoteSourcePort, RemoteUnknownReason, TrustedRemoteContext,
    UntrustedRemoteHit, validate_remote_batch,
};
use search_application::remote_evidence::{RemoteProvenanceLookupPort, VerifiedSourceProvenance};
use search_application::remote_observation::{
    CheckedRemoteObservationAdapter, RemoteSnapshotVerifierPort, SnapshotAttestation,
    SnapshotExtent,
};
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::scoped::{
    AccessContextAuthorityPort, AuthorizedSourceScope, CurrentSourceVisibilityPort,
};
use search_core::id::DiscoveryEvaluationId;
use search_core::materialization::MaterializationState;
use search_core::resource::ResourceKind;
use time::OffsetDateTime;

use crate::protocol::{
    DecodedContent, DecodedSnapshot, authorize_body, decode_authorize, decode_content,
    decode_response, live_body, lookup_body, search_body,
};
use crate::transport::{GuardedHttpTransport, RegisteredPath, TransportError};

/// Evaluations whose request budget is remembered at once.
const TRACKED_EVALUATIONS: usize = 256;

/// The protocol verifier for one decoded response: the provider's snapshot
/// token and extent, observed now. The application decides what it proves.
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
                vec![],
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

pub struct HttpRemoteSourceAdapter<'a> {
    registration: RemoteSourceRegistration,
    transport: GuardedHttpTransport,
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
    requests: Mutex<BTreeMap<DiscoveryEvaluationId, usize>>,
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
            requests: Mutex::new(BTreeMap::new()),
        })
    }

    /// One request within the evaluation's request budget.
    async fn call(
        &self,
        evaluation: DiscoveryEvaluationId,
        path: RegisteredPath,
        body: Vec<u8>,
        deadline: Instant,
    ) -> Result<Vec<u8>, RemoteUnknownReason> {
        {
            let mut requests = self
                .requests
                .lock()
                .map_err(|_| RemoteUnknownReason::Unavailable)?;
            if !requests.contains_key(&evaluation) && requests.len() >= TRACKED_EVALUATIONS {
                let oldest = *requests.keys().next().expect("nonempty");
                requests.remove(&oldest);
            }
            let used = requests.entry(evaluation).or_insert(0);
            if *used >= self.registration.limits().max_pages_or_requests {
                return Err(RemoteUnknownReason::Unavailable);
            }
            *used += 1;
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

    async fn observe(
        &self,
        context: &TrustedRemoteContext,
        action: &PlannedRemoteAction,
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
                RegisteredPath::catalog(cursor.as_ref().map(|cursor| cursor.as_str())),
                Vec::new(),
            ),
            RemoteOperation::Query { input } => (RegisteredPath::search(), search_body(input)),
            RemoteOperation::Lookup { native_id } => {
                (RegisteredPath::lookup(), lookup_body(native_id))
            }
            RemoteOperation::Live { input } => (RegisteredPath::live(), live_body(input)),
        };
        let bytes = match self
            .call(context.binding().evaluation(), path, body, deadline)
            .await
        {
            Ok(bytes) => bytes,
            Err(reason) => return Ok(unknown(reason)),
        };
        let decoded = decode_response(operation.kind(), &bytes, &self.registration);
        drop(bytes);
        let Ok(decoded) = decoded else {
            return Ok(unknown(RemoteUnknownReason::Malformed));
        };
        let verifier = ProtocolSnapshot(decoded.snapshot);
        CheckedRemoteObservationAdapter::new(
            &self.registration,
            self.authority,
            self.visibility,
            &verifier,
        )
        .observe(context, action, decoded.input)
        .await
    }

    async fn content(
        &self,
        evaluation: DiscoveryEvaluationId,
        native_id: &str,
    ) -> Result<DecodedContent, RemoteUnknownReason> {
        let bytes = self
            .call(
                evaluation,
                RegisteredPath::content(native_id),
                Vec::new(),
                self.call_deadline(),
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
    ) -> Result<crate::protocol::DecodedAuthorization, RemoteUnknownReason> {
        let bytes = self
            .call(
                context.binding().evaluation(),
                RegisteredPath::authorize(),
                authorize_body(
                    context.binding().actor().principal().as_str(),
                    target.map(RemoteIdentity::native_id),
                ),
                self.call_deadline(),
            )
            .await?;
        decode_authorize(&bytes, &self.registration).map_err(|_| RemoteUnknownReason::Malformed)
    }

    fn owns(&self, context: &TrustedRemoteContext) -> bool {
        context.registration() == &self.registration
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
                outcomes.push(self.observe(context, action, deadline).await?);
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
            let authorization = match self.authorize(context, Some(target)).await {
                Ok(authorization)
                    if self.owns(context) && authorization.decision == AccessDecision::Allowed =>
                {
                    authorization
                }
                _ => {
                    return Err(SearchError::SourceUnavailable(
                        "current remote policy is unknown".into(),
                    ));
                }
            };
            Ok(CurrentSourcePolicy {
                resource_kind: self
                    .registration
                    .allowed_resource_kinds()
                    .first()
                    .copied()
                    .unwrap_or(ResourceKind::Knowledge),
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
            let content = match self
                .content(context.binding().evaluation(), native_id.as_str())
                .await
            {
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
            if target.identity().source_scope() != scope
                || scope.source_id() != self.registration.source_id()
            {
                return Ok(None);
            }
            // The provenance record is read for the binding's evaluation.
            let evaluation = target.snapshot().evaluation();
            let Ok(content) = self
                .content(evaluation, target.identity().native_id().as_str())
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
