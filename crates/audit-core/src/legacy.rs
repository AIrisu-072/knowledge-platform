//! Projection of a Document `audit_outbox_events` staging row, as returned by
//! the relay claim function, into a payload-v1 envelope (design §4.2, §4.4).
//!
//! Nothing is inferred that the legacy row does not contain: no actor kind,
//! no W3C trace, no cancel-side operation id, and never the reason text.

use std::fmt;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::catalog::{
    AUDIT_RELAY_SOURCE, AUDIT_STORE_SOURCE, CONTROL_TYPE_PREFIX, Catalog, Origin, ReasonPolicy,
    ResourceType,
};
use crate::codes::{Rejection, RejectionCode as C};
use crate::envelope::{
    AuditEnvelope, DATACONTENTTYPE, DATASCHEMA, LEGACY_SOURCE_FORMAT, REASON_TEXT_RETAINED,
    SCHEMA_VERSION, SPECVERSION,
};
use crate::kinds::{Kind, is_uuid};

/// Adapter version of [`project`].
pub const LEGACY_ADAPTER_VERSION: i32 = 1;

/// The claim projection of one staging row (design §5.4). The SQL claim
/// function has already removed the top-level `reason` from `data` and
/// reports only its JSON type (`reason_kind`, from `jsonb_typeof`) and UTF-8
/// length (`reason_bytes`). When `oversize` is true the other text members may
/// be empty and `data` null. `occurred_at` is the UTC microsecond rendering
/// `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
#[derive(Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentStagingProjection {
    pub event_id: String,
    pub event_type: String,
    pub source: String,
    pub subject: String,
    pub actor_identity_provider: String,
    pub actor_principal_id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_version_id: Option<String>,
    pub result: String,
    pub trace_id: Option<String>,
    pub occurred_at: String,
    pub oversize: bool,
    pub data: Option<Value>,
    pub data_kind: String,
    pub reason_kind: Option<String>,
    pub reason_bytes: Option<i64>,
    pub source_intact: bool,
    pub source_commitment: String,
    pub registration_kind: String,
}

impl fmt::Debug for DocumentStagingProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentStagingProjection")
            .field("event_id", &self.event_id)
            .field("event_type", &self.event_type)
            .finish_non_exhaustive()
    }
}

fn reject<T>(code: C, field: &'static str) -> Result<T, Rejection> {
    Err(Rejection::at(code, field))
}

/// Projects a claim row into a validated envelope or a quarantine code.
///
/// Order: oversize, source digest, data shape, control/catalog admission,
/// reason summary, data members, duplicated actor, service executor, then the
/// full envelope validation.
pub fn project(row: &DocumentStagingProjection) -> Result<AuditEnvelope, Rejection> {
    if row.oversize {
        return Err(Rejection::new(C::SourceRowTooLarge));
    }
    if !row.source_intact {
        return Err(Rejection::new(C::SourceDigestMismatch));
    }
    let data = match (&row.data, row.data_kind.as_str()) {
        (Some(Value::Object(data)), "object") => data,
        _ => return reject(C::InvalidField, "data"),
    };
    if row.event_type.starts_with(CONTROL_TYPE_PREFIX)
        || row.source == AUDIT_STORE_SOURCE
        || row.source == AUDIT_RELAY_SOURCE
        || row.resource_type == ResourceType::AuditStore.as_str()
    {
        return reject(C::ControlTypeForbidden, "event_type");
    }
    let Some(spec) = Catalog::embedded().get(&row.event_type) else {
        return reject(C::UnknownEventType, "event_type");
    };
    if spec.origin != Origin::Relay {
        return reject(C::ControlTypeForbidden, "event_type");
    }

    if data.contains_key("reason") {
        return reject(C::UnknownField, "reason");
    }
    let reason = match spec.reason {
        ReasonPolicy::CallerText => match (row.reason_kind.as_deref(), row.reason_bytes) {
            (Some("string"), Some(bytes)) if bytes >= 0 => Some(json!({
                "provided": true,
                "utf8_bytes": bytes,
                "text_retained": REASON_TEXT_RETAINED,
            })),
            (Some("string") | None, _) => return reject(C::InvalidReason, "reason"),
            (Some(_), _) => return reject(C::ReasonNotString, "reason"),
        },
        ReasonPolicy::Absent => {
            if row.reason_kind.is_some() || row.reason_bytes.is_some() {
                return reject(C::UnknownField, "reason");
            }
            None
        }
    };

    if data.keys().any(|key| !spec.fields.contains_key(key)) {
        return reject(C::UnknownField, "data");
    }
    if let Some(missing) = spec.required.iter().find(|name| !data.contains_key(*name)) {
        return reject(C::MissingField, missing.as_str());
    }
    if let Some(field) = &spec.duplicated_actor_field {
        let actor = &data[field];
        if !Kind::Principal.accepts(&[], actor) {
            return reject(C::InvalidActor, field.as_str());
        }
        if actor["identityProvider"] != row.actor_identity_provider
            || actor["principalId"] != row.actor_principal_id
        {
            return reject(C::ActorMismatch, field.as_str());
        }
    }
    let service_executor = match spec
        .service_executor_field
        .as_ref()
        .and_then(|field| data.get(field).map(|value| (field, value)))
    {
        Some((field, executor)) => {
            if !Kind::Principal.accepts(&[], executor) {
                return reject(C::InvalidServiceExecutor, field.as_str());
            }
            Some(json!({
                "issuer": executor["identityProvider"],
                "principal_id": executor["principalId"],
            }))
        }
        None => None,
    };

    let details: Map<String, Value> = data
        .iter()
        .filter(|(key, _)| spec.is_detail_field(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    let mut correlation = Map::new();
    for (member, field) in [
        ("operation_id", &spec.operation_id_field),
        ("publish_operation_id", &spec.publish_operation_id_field),
    ] {
        if let Some(value) = field.as_ref().and_then(|name| details.get(name)) {
            correlation.insert(member.to_owned(), value.clone());
        }
    }
    if let Some(trace) = &row.trace_id {
        if !is_uuid(trace) {
            return reject(C::InvalidSourceCorrelation, "trace_id");
        }
        correlation.insert("source_correlation_id".to_owned(), json!(trace));
    }

    let mut resource = Map::new();
    resource.insert("type".to_owned(), json!(row.resource_type));
    resource.insert("id".to_owned(), json!(row.resource_id));
    if let Some(version) = &row.resource_version_id {
        resource.insert("version_id".to_owned(), json!(version));
    }

    let mut payload = Map::new();
    payload.insert("schema_version".to_owned(), json!(SCHEMA_VERSION));
    payload.insert("event_class".to_owned(), json!(spec.event_class.as_str()));
    payload.insert("action".to_owned(), json!(row.event_type));
    payload.insert(
        "actor".to_owned(),
        json!({"issuer": row.actor_identity_provider, "principal_id": row.actor_principal_id}),
    );
    if let Some(executor) = service_executor {
        payload.insert("service_executor".to_owned(), executor);
    }
    payload.insert("resource".to_owned(), Value::Object(resource));
    payload.insert("result".to_owned(), json!(row.result));
    if let Some(code) = spec
        .reason_code_field
        .as_ref()
        .and_then(|field| details.get(field))
    {
        payload.insert("reason_code".to_owned(), code.clone());
    }
    if let Some(reason) = reason {
        payload.insert("reason".to_owned(), reason);
    }
    payload.insert("correlation".to_owned(), Value::Object(correlation));
    payload.insert("details".to_owned(), Value::Object(details));
    payload.insert("extensions".to_owned(), json!({}));
    payload.insert(
        "provenance".to_owned(),
        json!({
            "source_format": LEGACY_SOURCE_FORMAT,
            "adapter_version": LEGACY_ADAPTER_VERSION,
            "source_commitment": row.source_commitment,
            "registration": row.registration_kind,
        }),
    );

    let envelope = json!({
        "specversion": SPECVERSION,
        "id": row.event_id,
        "source": row.source,
        "type": row.event_type,
        "subject": row.subject,
        "time": row.occurred_at,
        "datacontenttype": DATACONTENTTYPE,
        "dataschema": DATASCHEMA,
        "data": Value::Object(payload),
    });
    AuditEnvelope::from_value(envelope, Origin::Relay)
}
