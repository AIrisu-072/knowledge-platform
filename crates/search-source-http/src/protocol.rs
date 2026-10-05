//! The registered catalog protocol: request encoders over trusted inputs and
//! a bounded decoder that treats every provider JSON field as untrusted.
//!
//! The decoder enforces the registration's JSON depth, page, identity and
//! cursor bounds, checks the tenant/Source echo and returns only values the
//! application re-checks again. Provider role, origin, tool text or URL
//! fields are never interpreted; unknown fields are ignored.

use std::fmt;

use search_application::ports::AccessDecision;
use search_application::remote::{
    OpaqueCursor, RemoteOperationKind, RemotePage, RemoteResponseInput, RemoteResponseStatus,
    UntrustedRemoteHit,
};
use search_application::remote_observation::SnapshotExtent;
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::retrieval::{LiveInput, OpaqueNativeId, RemoteQueryInput};
use search_core::materialization::ProviderContentPermission;
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use serde::Deserialize;
use serde_json::{Value, json};

/// Low-cardinality decode failures; none carries provider text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProtocolError {
    Malformed,
    DepthExceeded,
    LimitExceeded,
    ScopeMismatch,
}

/// The provider's snapshot claim, checked for shape only. The application's
/// observation adapter decides what it proves.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedSnapshot {
    pub token: String,
    pub extent: SnapshotExtent,
}

impl fmt::Debug for DecodedSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DecodedSnapshot(<untrusted>)")
    }
}

#[derive(Debug)]
pub struct DecodedResponse {
    pub input: RemoteResponseInput,
    pub snapshot: DecodedSnapshot,
}

/// One provenance record of the content endpoint, still unverified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceRecord {
    pub evidence_ref: String,
    pub direct: bool,
    pub summary: bool,
    pub lineage: String,
    pub predicate: String,
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedContent {
    pub native_id: OpaqueNativeId,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub provenance: Vec<ProvenanceRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedAuthorization {
    pub decision: AccessDecision,
    pub permission: ProviderContentPermission,
}

#[derive(Deserialize)]
struct WireSnapshot {
    token: String,
    extent: String,
}

#[derive(Deserialize)]
struct WirePage {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    next: Option<String>,
    terminal: bool,
}

#[derive(Deserialize)]
struct WireField {
    name: String,
    value: Value,
    #[serde(default)]
    provenance: Option<String>,
}

#[derive(Deserialize)]
struct WireHit {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    fields: Vec<WireField>,
}

#[derive(Deserialize)]
struct WireList {
    tenant: String,
    source: String,
    snapshot: WireSnapshot,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    page: Option<WirePage>,
    #[serde(default)]
    total: Option<u64>,
    hits: Vec<WireHit>,
}

#[derive(Deserialize)]
struct WireProvenance {
    #[serde(rename = "ref")]
    evidence_ref: String,
    direct: bool,
    #[serde(default)]
    summary: bool,
    lineage: String,
    predicate: String,
    #[serde(default)]
    citations: Vec<String>,
}

#[derive(Deserialize)]
struct WireContent {
    tenant: String,
    source: String,
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    provenance: Vec<WireProvenance>,
}

#[derive(Deserialize)]
struct WireAuthorization {
    tenant: String,
    source: String,
    decision: String,
    acl_revision: u64,
    #[serde(default)]
    permission: Option<String>,
}

pub fn search_body(input: &RemoteQueryInput) -> Vec<u8> {
    let facets: Vec<Value> = input
        .facets()
        .iter()
        .map(|facet| json!({"name": facet.facet, "value": typed_json(&facet.expected)}))
        .collect();
    json!({"query": input.text(), "limit": input.window(), "facets": facets})
        .to_string()
        .into_bytes()
}

pub fn lookup_body(native_id: &OpaqueNativeId) -> Vec<u8> {
    json!({"id": native_id.as_str()}).to_string().into_bytes()
}

pub fn live_body(input: &LiveInput) -> Vec<u8> {
    match (input.query_input(), input.native_id()) {
        (Some(query), _) => json!({"query": query.text(), "limit": query.window()}),
        (None, Some(native_id)) => json!({"id": native_id.as_str()}),
        (None, None) => json!({}),
    }
    .to_string()
    .into_bytes()
}

pub fn authorize_body(principal: &str, native_id: Option<&OpaqueNativeId>) -> Vec<u8> {
    json!({"principal": principal, "id": native_id.map(OpaqueNativeId::as_str)})
        .to_string()
        .into_bytes()
}

fn typed_json(value: &TypedValue) -> Value {
    match value {
        TypedValue::Bool(value) => json!(value),
        TypedValue::String(value) | TypedValue::ConceptRef(value) => json!(value),
        TypedValue::Integer(value) => i64::try_from(*value).map_or(Value::Null, |v| json!(v)),
        _ => Value::Null,
    }
}

/// Nesting depth outside strings, without building a value first.
fn within_depth(bytes: &[u8], max: usize) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &byte in bytes {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max {
                    return false;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}

fn parse<'a, T: Deserialize<'a>>(
    bytes: &'a [u8],
    registration: &RemoteSourceRegistration,
) -> Result<T, RemoteProtocolError> {
    if !within_depth(bytes, registration.limits().max_json_depth) {
        return Err(RemoteProtocolError::DepthExceeded);
    }
    serde_json::from_slice(bytes).map_err(|_| RemoteProtocolError::Malformed)
}

fn check_scope(
    tenant: &str,
    source: &str,
    registration: &RemoteSourceRegistration,
) -> Result<(), RemoteProtocolError> {
    if tenant != registration.tenant().as_str()
        || source != registration.source_id().as_uuid().to_string()
    {
        return Err(RemoteProtocolError::ScopeMismatch);
    }
    Ok(())
}

fn kind(value: &str) -> Result<ResourceKind, RemoteProtocolError> {
    Ok(match value {
        "knowledge" => ResourceKind::Knowledge,
        "document" => ResourceKind::Document,
        "semantic" => ResourceKind::Semantic,
        "capability" => ResourceKind::Capability,
        "agent_skill" => ResourceKind::AgentSkill,
        "workflow" => ResourceKind::Workflow,
        "policy" => ResourceKind::Policy,
        _ => return Err(RemoteProtocolError::Malformed),
    })
}

fn field_value(value: Value) -> Result<TypedValue, RemoteProtocolError> {
    match value {
        Value::Bool(value) => Ok(TypedValue::Bool(value)),
        Value::String(value) => Ok(TypedValue::String(value)),
        Value::Number(number) => number
            .as_i64()
            .map(|value| TypedValue::Integer(value.into()))
            .ok_or(RemoteProtocolError::Malformed),
        _ => Err(RemoteProtocolError::Malformed),
    }
}

fn native(
    value: String,
    registration: &RemoteSourceRegistration,
) -> Result<OpaqueNativeId, RemoteProtocolError> {
    if value.len() > registration.limits().max_native_id_bytes {
        return Err(RemoteProtocolError::LimitExceeded);
    }
    OpaqueNativeId::new(value).map_err(|_| RemoteProtocolError::Malformed)
}

fn cursor(
    value: Option<String>,
    registration: &RemoteSourceRegistration,
) -> Result<Option<OpaqueCursor>, RemoteProtocolError> {
    value
        .map(|value| {
            if value.len() > registration.limits().max_cursor_bytes {
                return Err(RemoteProtocolError::LimitExceeded);
            }
            OpaqueCursor::new(value).map_err(|_| RemoteProtocolError::Malformed)
        })
        .transpose()
}

/// Decodes one list response of `mode` into untrusted application input.
pub fn decode_response(
    mode: RemoteOperationKind,
    bytes: &[u8],
    registration: &RemoteSourceRegistration,
) -> Result<DecodedResponse, RemoteProtocolError> {
    let wire: WireList = parse(bytes, registration)?;
    check_scope(&wire.tenant, &wire.source, registration)?;
    let limits = registration.limits();
    let extent = match wire.snapshot.extent.as_str() {
        "complete" => SnapshotExtent::CompleteSource,
        "partial" => SnapshotExtent::PartialSource,
        "single" => SnapshotExtent::SingleResponse,
        _ => return Err(RemoteProtocolError::Malformed),
    };
    if wire.snapshot.token.is_empty() || wire.snapshot.token.len() > 1024 {
        return Err(RemoteProtocolError::Malformed);
    }
    if wire.hits.len() > limits.max_hits_per_page {
        return Err(RemoteProtocolError::LimitExceeded);
    }
    let status = match wire.status.as_deref() {
        None | Some("ok") => RemoteResponseStatus::Success,
        Some("partial") => RemoteResponseStatus::Partial,
        _ => return Err(RemoteProtocolError::Malformed),
    };
    let page = match (mode, wire.page) {
        (RemoteOperationKind::Enumerate, Some(page)) => RemotePage::Enumeration {
            requested: cursor(page.cursor, registration)?,
            next: cursor(page.next, registration)?,
            terminal: page.terminal,
        },
        (RemoteOperationKind::Enumerate, None) => return Err(RemoteProtocolError::Malformed),
        (_, None) => RemotePage::Unpaged,
        (_, Some(_)) => return Err(RemoteProtocolError::Malformed),
    };
    let mut hits = Vec::with_capacity(wire.hits.len());
    for hit in wire.hits {
        let id = hit.id.map(|id| native(id, registration)).transpose()?;
        let fields = hit
            .fields
            .into_iter()
            .map(|field| Ok((field.name, field_value(field.value)?, field.provenance)))
            .collect::<Result<Vec<_>, RemoteProtocolError>>()?;
        let kind = hit.kind.as_deref().map(kind).transpose()?;
        hits.push(
            UntrustedRemoteHit::new(id, hit.version, hit.digest)
                .and_then(|untrusted| untrusted.with_projection(kind, hit.title, fields))
                .map_err(|_| RemoteProtocolError::Malformed)?,
        );
    }
    let input = RemoteResponseInput::new(status, page, hits, wire.total)
        .map_err(|_| RemoteProtocolError::Malformed)?;
    Ok(DecodedResponse {
        input,
        snapshot: DecodedSnapshot {
            token: wire.snapshot.token,
            extent,
        },
    })
}

/// Decodes the content endpoint's identity, version/digest and provenance;
/// any body in the response is dropped with the bytes.
pub fn decode_content(
    bytes: &[u8],
    registration: &RemoteSourceRegistration,
) -> Result<DecodedContent, RemoteProtocolError> {
    let wire: WireContent = parse(bytes, registration)?;
    check_scope(&wire.tenant, &wire.source, registration)?;
    let bounded = |value: &str, max: usize| {
        !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
    };
    if wire.provenance.len() > 64
        || [&wire.version, &wire.digest]
            .into_iter()
            .flatten()
            .any(|value| !bounded(value, 512))
    {
        return Err(RemoteProtocolError::Malformed);
    }
    let mut provenance = Vec::with_capacity(wire.provenance.len());
    for record in wire.provenance {
        if !bounded(&record.evidence_ref, 512)
            || !bounded(&record.lineage, 256)
            || !bounded(&record.predicate, 128)
            || record.citations.len() > 16
            || record
                .citations
                .iter()
                .any(|citation| !bounded(citation, 512))
        {
            return Err(RemoteProtocolError::Malformed);
        }
        provenance.push(ProvenanceRecord {
            evidence_ref: record.evidence_ref,
            direct: record.direct,
            summary: record.summary,
            lineage: record.lineage,
            predicate: record.predicate,
            citations: record.citations,
        });
    }
    Ok(DecodedContent {
        native_id: native(wire.id, registration)?,
        version: wire.version,
        digest: wire.digest,
        provenance,
    })
}

/// Decodes `/authorize`. The ACL revision must be the one this Source scope
/// was authorized under; anything else is unknown, never allowed.
pub fn decode_authorize(
    bytes: &[u8],
    registration: &RemoteSourceRegistration,
) -> Result<DecodedAuthorization, RemoteProtocolError> {
    let wire: WireAuthorization = parse(bytes, registration)?;
    check_scope(&wire.tenant, &wire.source, registration)?;
    let decision = match (
        wire.decision.as_str(),
        wire.acl_revision == registration.visibility_revision().get(),
    ) {
        ("allowed", true) => AccessDecision::Allowed,
        ("denied", _) => AccessDecision::Denied,
        _ => AccessDecision::Unknown,
    };
    let permission = match wire.permission.as_deref() {
        None | Some("reference_only") => ProviderContentPermission::ReferenceOnly,
        Some("metadata") => ProviderContentPermission::Metadata,
        Some("fragment") => ProviderContentPermission::Fragment,
        Some("full_content") => ProviderContentPermission::FullContent,
        Some(_) => return Err(RemoteProtocolError::Malformed),
    };
    Ok(DecodedAuthorization {
        decision,
        permission,
    })
}
