//! Closed CloudEvents 1.0.2 structured-JSON envelope with payload v1.
//!
//! Validation is catalog driven and reports the first violation in a fixed
//! order (size, attributes, path/origin, catalog entry, payload members,
//! details, bindings, subject, reason, correlation, extensions, provenance).

use std::fmt;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::catalog::{
    AUDIT_RELAY_SOURCE, AUDIT_STORE_RESOURCE_ID, AUDIT_STORE_SOURCE, BindingTarget,
    CONTROL_TYPE_PREFIX, Catalog, EventSpec, Origin, ReasonPolicy, ResourceType,
    VersionRequirement,
};
use crate::codes::{Rejection, RejectionCode as C};
use crate::json::{canonicalize, parse_unique};
use crate::kinds::{
    MAX_STRING_BYTES, NIL_UUID, is_bounded_text, is_hex_digest, is_principal_part,
    is_utc_timestamp, is_uuid, is_w3c_trace_id,
};

pub const SPECVERSION: &str = "1.0";
pub const DATACONTENTTYPE: &str = "application/json";
pub const DATASCHEMA: &str = "urn:knowledge-platform:audit:payload:v1";
pub const SCHEMA_VERSION: i64 = 1;
/// Rust-side bound on the compact serialization. The Store bounds the
/// PostgreSQL `jsonb::text` at 32 KiB, so anything accepted here fits there.
pub const MAX_ENVELOPE_BYTES: usize = 24 * 1024;
/// Provenance format of the Document legacy adapter.
pub const LEGACY_SOURCE_FORMAT: &str = "document-audit-outbox-v0";
pub const STORE_CONTROL_SOURCE_FORMAT: &str = "audit-store-control-v1";
pub const RELAY_CONTROL_SOURCE_FORMAT: &str = "audit-relay-control-v1";
/// Where caller reason text stays (it is never copied to the Store).
pub const REASON_TEXT_RETAINED: &str = "source_systems";
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
const REASON_KEYS: [&str; 3] = ["provided", "text_retained", "utf8_bytes"];
const CORRELATION_KEYS: [&str; 4] = [
    "operation_id",
    "publish_operation_id",
    "source_correlation_id",
    "trace_id",
];

/// The provenance format each origin must declare.
pub const fn source_format_for(origin: Origin) -> &'static str {
    match origin {
        Origin::Relay => LEGACY_SOURCE_FORMAT,
        Origin::Store => STORE_CONTROL_SOURCE_FORMAT,
        Origin::RelayControl => RELAY_CONTROL_SOURCE_FORMAT,
    }
}

/// A validated envelope. It can only be constructed through validation, and
/// its members are stored in byte order so serialization is deterministic.
#[derive(Clone, PartialEq)]
pub struct AuditEnvelope {
    value: Value,
    id: Uuid,
    spec: &'static EventSpec,
}

impl AuditEnvelope {
    /// Validates `value` for the given submission path.
    pub fn from_value(value: Value, path: Origin) -> Result<Self, Rejection> {
        let spec = validate(&value, path)?;
        let id = value["id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or(Rejection::at(C::InvalidEnvelope, "id"))?;
        Ok(Self {
            value: canonicalize(value),
            id,
            spec,
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

    pub fn origin(&self) -> Origin {
        self.spec.origin
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
            .finish_non_exhaustive()
    }
}

/// Validates `value` against the closed envelope layout and the embedded
/// catalog for the given submission path. `Origin::Relay` refuses control
/// types, control sources and `AuditStore` resources.
pub fn validate_envelope(value: &Value, path: Origin) -> Result<(), Rejection> {
    validate(value, path).map(|_| ())
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

fn validate(value: &Value, path: Origin) -> Result<&'static EventSpec, Rejection> {
    if value.to_string().len() > MAX_ENVELOPE_BYTES {
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
        let control_resource = data
            .get("resource")
            .and_then(|resource| resource.get("type"))
            .and_then(Value::as_str)
            == Some(ResourceType::AuditStore.as_str());
        if event_type.starts_with(CONTROL_TYPE_PREFIX)
            || source == AUDIT_STORE_SOURCE
            || source == AUDIT_RELAY_SOURCE
            || control_resource
        {
            return reject(C::ControlTypeForbidden, "type");
        }
    }
    let Some(spec) = Catalog::embedded().get(event_type) else {
        return reject(C::UnknownEventType, "type");
    };
    if spec.origin != path {
        return reject(C::ControlTypeForbidden, "type");
    }
    if source != spec.source {
        return reject(C::InvalidSource, "source");
    }
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
    let resource = check_resource(spec, &data["resource"])?;
    if !data["result"]
        .as_str()
        .is_some_and(|r| spec.allows_result(r))
    {
        return reject(C::InvalidResult, "data.result");
    }
    let details = check_details(spec, &data["details"])?;
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
    check_correlation(spec, details, &data["correlation"])?;
    if !data["extensions"].as_object().is_some_and(Map::is_empty) {
        return reject(C::InvalidExtensions, "data.extensions");
    }
    if !is_provenance(spec.origin, &data["provenance"]) {
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

fn check_resource<'a>(spec: &EventSpec, value: &'a Value) -> Result<ResourceView<'a>, Rejection> {
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
    let id_ok = match kind {
        ResourceType::AuditStore => id == AUDIT_STORE_RESOURCE_ID,
        _ => is_uuid(id) || (spec.nil_resource_allowed && id == NIL_UUID),
    };
    if !id_ok {
        return Err(invalid);
    }
    let version_id = match object.get("version_id") {
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
        if details.get(name).is_some_and(|v| !field.accepts(v)) {
            return reject(C::InvalidField, name);
        }
    }
    Ok(details)
}

fn check_correlation(
    spec: &EventSpec,
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
    if correlation
        .get("trace_id")
        .is_some_and(|id| !id.as_str().is_some_and(is_w3c_trace_id))
    {
        return reject(C::InvalidCorrelation, "data.correlation.trace_id");
    }
    Ok(())
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
            && object["utf8_bytes"].as_i64().is_some_and(|n| n >= 0)
    })
}

fn is_provenance(origin: Origin, value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let keys_ok = match origin {
        Origin::Relay => matches!(
            check_keys(
                object,
                &[
                    "adapter_version",
                    "registration",
                    "source_commitment",
                    "source_format"
                ],
                &[],
            ),
            Keys::Exact
        ),
        Origin::Store | Origin::RelayControl => matches!(
            check_keys(object, &["adapter_version", "source_format"], &[]),
            Keys::Exact
        ),
    };
    keys_ok
        && object["source_format"] == source_format_for(origin)
        && object["adapter_version"]
            .as_i64()
            .is_some_and(|v| (1..=i64::from(i32::MAX)).contains(&v))
        && object
            .get("source_commitment")
            .is_none_or(|c| c.as_str().is_some_and(is_hex_digest))
        && object
            .get("registration")
            .is_none_or(|r| r.as_str().is_some_and(|r| REGISTRATION_KINDS.contains(&r)))
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
