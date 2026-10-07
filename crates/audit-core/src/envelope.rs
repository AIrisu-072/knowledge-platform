//! Closed CloudEvents 1.0.2 structured-JSON envelope with payload v1.
//!
//! Validation is catalog driven and reports the first violation in a fixed
//! order (size, attributes, path/origin, catalog entry, payload members,
//! details, the `db_role` actor rule of control paths, bindings, subject,
//! reason, correlation, extensions, provenance). `nil_client_id` is reported
//! only for ids the catalog marks client-chosen, on the relay path.
//! The provenance contract and the `correlation.trace_id` capability come
//! from the catalog adapter of the envelope's source.

use std::fmt;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::catalog::{
    AUDIT_RELAY_SOURCE, AUDIT_STORE_RESOURCE_ID, AUDIT_STORE_SOURCE, AdapterSpec, BindingTarget,
    CONTROL_TYPE_PREFIX, Catalog, EventSpec, Origin, ReasonPolicy, Requirement, ResourceType,
    VersionRequirement,
};
use crate::codes::{Rejection, RejectionCode as C};
use crate::json::{canonicalize, jsonb_text_len, parse_unique};
use crate::kinds::{
    Kind, MAX_STRING_BYTES, NIL_UUID, is_bounded_text, is_db_role, is_hex_digest,
    is_principal_part, is_utc_timestamp, is_uuid, is_w3c_trace_id,
};

pub const SPECVERSION: &str = "1.0";
pub const DATACONTENTTYPE: &str = "application/json";
pub const DATASCHEMA: &str = "urn:knowledge-platform:audit:payload:v1";
pub const SCHEMA_VERSION: i64 = 1;
/// Upper bound on the PostgreSQL `jsonb::text` rendering of an envelope, in
/// bytes (design §4.1). Rust measures the same rendering
/// ([`jsonb_text_len`]) that the Store bounds, so the two limits agree
/// exactly. There is no separate compact-size bound: the compact rendering is
/// never longer than the jsonb rendering.
pub const JSONB_TEXT_LIMIT: usize = 32 * 1024;
/// Provenance format of the Document legacy adapter (catalog adapter of
/// `urn:knowledge-platform:document-platform`).
pub const LEGACY_SOURCE_FORMAT: &str = "document-audit-outbox-v0";
/// Where caller reason text stays (it is never copied to the Store).
pub const REASON_TEXT_RETAINED: &str = "source_systems";
/// Upper bound on `reason.utf8_bytes`: the Document HTTP body limit.
pub const MAX_REASON_UTF8_BYTES: i64 = 1_048_576;
pub const REGISTRATION_KINDS: [&str; 3] = ["trigger", "backfill", "repair"];

/// The closed CloudEvents attribute set.
pub const ATTRIBUTES: [&str; 9] = [
    "data",
    "datacontenttype",
    "dataschema",
    "id",
    "source",
    "specversion",
    "subject",
    "time",
    "type",
];
const DATA_REQUIRED: [&str; 10] = [
    "action",
    "actor",
    "correlation",
    "details",
    "event_class",
    "extensions",
    "provenance",
    "resource",
    "result",
    "schema_version",
];
const DATA_OPTIONAL: [&str; 3] = ["reason", "reason_code", "service_executor"];
const PRINCIPAL_KEYS: [&str; 2] = ["issuer", "principal_id"];
/// The actor issuer of control events recorded without a bound principal
/// (for example an `unbound` denial or a bootstrap from an owner login): the
/// principal id is then the session's database role (`details.session_role`).
pub const DB_ROLE_ISSUER: &str = "db_role";
const REASON_KEYS: [&str; 3] = ["provided", "text_retained", "utf8_bytes"];
const CORRELATION_KEYS: [&str; 4] = [
    "operation_id",
    "publish_operation_id",
    "source_correlation_id",
    "trace_id",
];

/// A validated envelope. It can only be constructed through validation, and
/// its members are stored in byte order so serialization is deterministic.
/// It remembers the submission path it was validated for ([`Self::origin`]).
#[derive(Clone, PartialEq)]
pub struct AuditEnvelope {
    value: Value,
    id: Uuid,
    spec: &'static EventSpec,
    origin: Origin,
}

impl AuditEnvelope {
    /// Validates `value` for the given submission path.
    pub fn from_value(value: Value, path: Origin) -> Result<Self, Rejection> {
        let spec = validate_with(Catalog::embedded(), &value, path)?;
        let id = value["id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or(Rejection::at(C::InvalidEnvelope, "id"))?;
        Ok(Self {
            value: canonicalize(value),
            id,
            spec,
            origin: path,
        })
    }

    /// Parses (rejecting duplicate keys) and validates envelope text.
    pub fn from_json(text: &str, path: Origin) -> Result<Self, Rejection> {
        Self::from_value(parse_unique(text)?, path)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The catalog entry of this envelope's type.
    pub fn spec(&self) -> &'static EventSpec {
        self.spec
    }

    /// The submission path this envelope was validated for (always equal to
    /// the catalog entry's origin). `AuditStore::ingest` only accepts
    /// [`Origin::Relay`] ([`crate::port::precheck_ingest`]).
    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn event_type(&self) -> &str {
        &self.spec.event_type
    }

    pub fn source(&self) -> &str {
        self.attribute("source")
    }

    pub fn subject(&self) -> &str {
        self.attribute("subject")
    }

    pub fn time(&self) -> &str {
        self.attribute("time")
    }

    /// The payload (`data`) object.
    pub fn data(&self) -> &Value {
        &self.value["data"]
    }

    pub fn as_value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }

    /// Compact JSON with members in byte order.
    pub fn to_json_string(&self) -> String {
        self.value.to_string()
    }

    fn attribute(&self, name: &str) -> &str {
        self.value[name].as_str().unwrap_or_default()
    }
}

impl fmt::Debug for AuditEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditEnvelope")
            .field("id", &self.id)
            .field("type", &self.spec.event_type)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// Validates `value` against the closed envelope layout and the embedded
/// catalog for the given submission path. `Origin::Relay` refuses control
/// types, control sources and `AuditStore` resources.
pub fn validate_envelope(value: &Value, path: Origin) -> Result<(), Rejection> {
    validate_with(Catalog::embedded(), value, path).map(|_| ())
}

enum Keys {
    Exact,
    Unknown,
    Missing(&'static str),
}

fn check_keys(object: &Map<String, Value>, required: &[&'static str], optional: &[&str]) -> Keys {
    if object
        .keys()
        .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Keys::Unknown;
    }
    match required.iter().find(|key| !object.contains_key(**key)) {
        Some(missing) => Keys::Missing(missing),
        None => Keys::Exact,
    }
}

fn reject<T>(code: C, field: &'static str) -> Result<T, Rejection> {
    Err(Rejection::at(code, field))
}

pub(crate) fn validate_with(
    catalog: &'static Catalog,
    value: &Value,
    path: Origin,
) -> Result<&'static EventSpec, Rejection> {
    if jsonb_text_len(value) > JSONB_TEXT_LIMIT {
        return Err(Rejection::new(C::EnvelopeTooLarge));
    }
    let Some(envelope) = value.as_object() else {
        return Err(Rejection::new(C::InvalidEnvelope));
    };
    if !matches!(check_keys(envelope, &ATTRIBUTES, &[]), Keys::Exact) {
        return Err(Rejection::new(C::InvalidEnvelope));
    }
    for (name, expected) in [
        ("specversion", SPECVERSION),
        ("datacontenttype", DATACONTENTTYPE),
        ("dataschema", DATASCHEMA),
    ] {
        if envelope[name] != expected {
            return reject(C::InvalidEnvelope, name);
        }
    }
    if !envelope["id"].as_str().is_some_and(is_uuid) {
        return reject(C::InvalidEnvelope, "id");
    }
    if !envelope["time"].as_str().is_some_and(is_utc_timestamp) {
        return reject(C::InvalidEnvelope, "time");
    }
    let bounded = |name: &'static str| {
        envelope[name]
            .as_str()
            .filter(|text| !text.is_empty() && is_bounded_text(text, MAX_STRING_BYTES))
            .ok_or(Rejection::at(C::InvalidEnvelope, name))
    };
    let event_type = bounded("type")?;
    let source = bounded("source")?;
    let Some(data) = envelope["data"].as_object() else {
        return reject(C::InvalidEnvelope, "data");
    };
    if path == Origin::Relay {
        if event_type.starts_with(CONTROL_TYPE_PREFIX) {
            return reject(C::ControlTypeForbidden, "type");
        }
        if source == AUDIT_STORE_SOURCE || source == AUDIT_RELAY_SOURCE {
            return reject(C::ControlTypeForbidden, "source");
        }
        let control_resource = data
            .get("resource")
            .and_then(|resource| resource.get("type"))
            .and_then(Value::as_str)
            == Some(ResourceType::AuditStore.as_str());
        if control_resource {
            return reject(C::ControlTypeForbidden, "data.resource.type");
        }
    }
    let Some(spec) = catalog.get(event_type) else {
        return reject(C::UnknownEventType, "type");
    };
    if spec.origin != path {
        return reject(C::ControlTypeForbidden, "type");
    }
    if source != spec.source {
        return reject(C::InvalidSource, "source");
    }
    let Some(adapter) = catalog.adapter(&spec.source) else {
        return reject(C::InvalidSource, "source");
    };
    match check_keys(data, &DATA_REQUIRED, &DATA_OPTIONAL) {
        Keys::Exact => {}
        Keys::Unknown => return reject(C::UnknownField, "data"),
        Keys::Missing(name) => return reject(C::MissingField, name),
    }
    if data["schema_version"].as_i64() != Some(SCHEMA_VERSION) {
        return reject(C::InvalidEnvelope, "data.schema_version");
    }
    if data["action"] != event_type {
        return reject(C::InvalidEnvelope, "data.action");
    }
    if data["event_class"] != spec.event_class.as_str() {
        return reject(C::InvalidEnvelope, "data.event_class");
    }
    if !is_principal(&data["actor"]) {
        return reject(C::InvalidActor, "data.actor");
    }
    if let Some(executor) = data.get("service_executor")
        && (spec.service_executor_field.is_none() || !is_principal(executor))
    {
        return reject(C::InvalidServiceExecutor, "data.service_executor");
    }
    let resource = check_resource(spec, path, &data["resource"])?;
    if !data["result"]
        .as_str()
        .is_some_and(|r| spec.allows_result(r))
    {
        return reject(C::InvalidResult, "data.result");
    }
    let details = check_details(spec, path, &data["details"])?;
    if path != Origin::Relay && !is_db_role_actor_consistent(&data["actor"], details) {
        return reject(C::InvalidActor, "data.actor");
    }
    for binding in &spec.bindings {
        let expected = match binding.equals {
            BindingTarget::ResourceId => Some(resource.id),
            BindingTarget::ResourceVersionId => resource.version_id,
            BindingTarget::ResourceType => Some(resource.kind.as_str()),
        };
        if details.get(&binding.field).and_then(Value::as_str) != expected {
            return reject(C::InvalidField, binding.field.as_str());
        }
    }
    let subject = envelope["subject"].as_str().unwrap_or_default();
    let subject_ok = is_bounded_text(subject, MAX_STRING_BYTES)
        && spec.subjects.iter().any(|shape| {
            shape
                .render(resource.kind, resource.id, resource.version_id, details)
                .is_some_and(|rendered| rendered == subject)
        });
    if !subject_ok {
        return reject(C::InvalidSubject, "subject");
    }
    let expected_code = spec
        .reason_code_field
        .as_ref()
        .and_then(|field| details.get(field));
    if data.get("reason_code") != expected_code {
        return reject(C::InvalidField, "data.reason_code");
    }
    let reason_ok = match (spec.reason, data.get("reason")) {
        (ReasonPolicy::CallerText, Some(summary)) => is_reason_summary(summary),
        (ReasonPolicy::Absent, None) => true,
        _ => false,
    };
    if !reason_ok {
        return reject(C::InvalidReason, "data.reason");
    }
    check_correlation(spec, adapter, details, &data["correlation"])?;
    if !data["extensions"].as_object().is_some_and(Map::is_empty) {
        return reject(C::InvalidExtensions, "data.extensions");
    }
    if !is_provenance(adapter, &data["provenance"]) {
        return reject(C::InvalidProvenance, "data.provenance");
    }
    if !strings_are_bounded(value) {
        return Err(Rejection::new(C::InvalidEnvelope));
    }
    Ok(spec)
}

struct ResourceView<'a> {
    kind: ResourceType,
    id: &'a str,
    version_id: Option<&'a str>,
}

fn check_resource<'a>(
    spec: &EventSpec,
    path: Origin,
    value: &'a Value,
) -> Result<ResourceView<'a>, Rejection> {
    let invalid = Rejection::at(C::InvalidResource, "data.resource");
    let object = value.as_object().ok_or(invalid)?;
    if !matches!(
        check_keys(object, &["id", "type"], &["version_id"]),
        Keys::Exact
    ) {
        return Err(invalid);
    }
    let kind = object["type"]
        .as_str()
        .and_then(ResourceType::parse)
        .filter(|kind| spec.allows_resource(*kind))
        .ok_or(invalid)?;
    let id = object["id"].as_str().ok_or(invalid)?;
    let relay = path == Origin::Relay;
    if id == NIL_UUID
        && !spec.nil_resource_allowed
        && relay
        && spec.client_chosen_resource_ids.contains(&kind)
    {
        return reject(C::NilClientId, "data.resource.id");
    }
    let id_ok = match kind {
        ResourceType::AuditStore => id == AUDIT_STORE_RESOURCE_ID,
        _ => is_uuid(id) || (spec.nil_resource_allowed && id == NIL_UUID),
    };
    if !id_ok {
        return Err(invalid);
    }
    let version_id = match object.get("version_id") {
        Some(version) if version == NIL_UUID && relay && spec.client_chosen_version_id => {
            return reject(C::NilClientId, "data.resource.version_id");
        }
        Some(version) => Some(version.as_str().filter(|v| is_uuid(v)).ok_or(invalid)?),
        None => None,
    };
    let version_ok = match spec.version_required {
        VersionRequirement::Required => version_id.is_some(),
        VersionRequirement::Forbidden => version_id.is_none(),
        VersionRequirement::Optional => true,
    };
    if !version_ok {
        return Err(invalid);
    }
    Ok(ResourceView {
        kind,
        id,
        version_id,
    })
}

fn check_details<'a>(
    spec: &'static EventSpec,
    path: Origin,
    value: &'a Value,
) -> Result<&'a Map<String, Value>, Rejection> {
    let Some(details) = value.as_object() else {
        return reject(C::InvalidField, "data.details");
    };
    if details.keys().any(|key| !spec.is_detail_field(key)) {
        return reject(C::UnknownField, "data.details");
    }
    if let Some(missing) = spec
        .detail_required()
        .find(|name| !details.contains_key(*name))
    {
        return reject(C::MissingField, missing);
    }
    for (name, field) in spec.detail_fields() {
        let Some(value) = details.get(name) else {
            continue;
        };
        if path == Origin::Relay && field.client_chosen && is_nil_client_id(field.kind, value) {
            return reject(C::NilClientId, name);
        }
        if !field.accepts(value) {
            return reject(C::InvalidField, name);
        }
    }
    Ok(details)
}

/// A uuid-kind value that is (or, for lists, contains) the nil UUID.
fn is_nil_client_id(kind: Kind, value: &Value) -> bool {
    match kind {
        Kind::Uuid | Kind::NullableUuid => value == NIL_UUID,
        Kind::UuidList => value
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == NIL_UUID)),
        _ => false,
    }
}

fn check_correlation(
    spec: &EventSpec,
    adapter: &AdapterSpec,
    details: &Map<String, Value>,
    value: &Value,
) -> Result<(), Rejection> {
    let Some(correlation) = value.as_object() else {
        return reject(C::InvalidCorrelation, "data.correlation");
    };
    if !matches!(check_keys(correlation, &[], &CORRELATION_KEYS), Keys::Exact) {
        return reject(C::InvalidCorrelation, "data.correlation");
    }
    for (member, field) in [
        ("operation_id", &spec.operation_id_field),
        ("publish_operation_id", &spec.publish_operation_id_field),
    ] {
        let expected = field.as_ref().and_then(|name| details.get(name));
        if correlation.get(member) != expected {
            return reject(C::InvalidCorrelation, "data.correlation");
        }
    }
    if correlation
        .get("source_correlation_id")
        .is_some_and(|id| !id.as_str().is_some_and(is_uuid))
    {
        return reject(
            C::InvalidSourceCorrelation,
            "data.correlation.source_correlation_id",
        );
    }
    if let Some(trace) = correlation.get("trace_id")
        && (!adapter.trace_id || !trace.as_str().is_some_and(is_w3c_trace_id))
    {
        return reject(C::InvalidCorrelation, "data.correlation.trace_id");
    }
    Ok(())
}

/// Control events: an actor with issuer [`DB_ROLE_ISSUER`] names the session
/// role, so its principal id must be a `db_role` equal to
/// `details.session_role`.
fn is_db_role_actor_consistent(actor: &Value, details: &Map<String, Value>) -> bool {
    if actor["issuer"] != DB_ROLE_ISSUER {
        return true;
    }
    actor["principal_id"].as_str().is_some_and(|role| {
        is_db_role(role) && details.get("session_role") == Some(&actor["principal_id"])
    })
}

fn is_principal(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        matches!(check_keys(object, &PRINCIPAL_KEYS, &[]), Keys::Exact)
            && PRINCIPAL_KEYS
                .iter()
                .all(|key| object[*key].as_str().is_some_and(is_principal_part))
    })
}

fn is_reason_summary(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        matches!(check_keys(object, &REASON_KEYS, &[]), Keys::Exact)
            && object["provided"] == Value::Bool(true)
            && object["text_retained"] == REASON_TEXT_RETAINED
            && object["utf8_bytes"]
                .as_i64()
                .is_some_and(|n| (0..=MAX_REASON_UTF8_BYTES).contains(&n))
    })
}

fn is_provenance(adapter: &AdapterSpec, value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let member_ok = |name: &str, requirement: Requirement, valid: &dyn Fn(&Value) -> bool| match (
        requirement,
        object.get(name),
    ) {
        (Requirement::Required | Requirement::Optional, Some(member)) => valid(member),
        (Requirement::Optional | Requirement::Forbidden, None) => true,
        (Requirement::Required, None) | (Requirement::Forbidden, Some(_)) => false,
    };
    object.keys().all(|key| {
        matches!(
            key.as_str(),
            "adapter_version" | "registration" | "source_commitment" | "source_format"
        )
    }) && object.get("source_format").and_then(Value::as_str)
        == Some(adapter.source_format.as_str())
        && object.get("adapter_version").and_then(Value::as_i64)
            == Some(i64::from(adapter.adapter_version))
        && member_ok("source_commitment", adapter.commitment, &|c| {
            c.as_str().is_some_and(is_hex_digest)
        })
        && member_ok("registration", adapter.registration, &|r| {
            r.as_str().is_some_and(|r| REGISTRATION_KINDS.contains(&r))
        })
}

/// Defense in depth: every string anywhere is bounded and control-free.
fn strings_are_bounded(value: &Value) -> bool {
    match value {
        Value::String(text) => is_bounded_text(text, MAX_STRING_BYTES),
        Value::Array(items) => items.iter().all(strings_are_bounded),
        Value::Object(map) => map.iter().all(|(key, member)| {
            is_bounded_text(key, MAX_STRING_BYTES) && strings_are_bounded(member)
        }),
        _ => true,
    }
}
