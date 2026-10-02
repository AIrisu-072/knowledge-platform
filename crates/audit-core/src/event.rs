use crate::catalog::{self, FieldRule};
use serde_json::{Map, Value};
use thiserror::Error;
use time::{Date, OffsetDateTime, Time, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("audit_envelope_too_large")]
    EnvelopeTooLarge,
    #[error("audit_legacy_row_too_large")]
    LegacyRowTooLarge,
    #[error("audit_invalid_envelope")]
    InvalidEnvelope,
    #[error("audit_invalid_metadata")]
    InvalidMetadata,
    #[error("audit_unsupported_event")]
    UnsupportedEvent,
    #[error("audit_invalid_catalog")]
    InvalidCatalog,
    #[error("legacy_reason_contract_unqualified")]
    LegacyReasonContractUnqualified,
}
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEnvelope(Value);
impl AuditEnvelope {
    pub(crate) fn new(value: Value) -> Self {
        Self(value)
    }
    pub fn as_value(&self) -> &Value {
        &self.0
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, ValidationError> {
        if bytes.len() > 32 * 1024 {
            return Err(ValidationError::EnvelopeTooLarge);
        }
        let event = Self(crate::json::parse(bytes)?);
        event.validate()?;
        Ok(event)
    }
    pub fn event_id(&self) -> Uuid {
        Uuid::parse_str(self.0["id"].as_str().expect("validated event ID")).expect("validated UUID")
    }
    pub fn event_type(&self) -> &str {
        self.0["type"].as_str().expect("validated event type")
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        let fail = ValidationError::InvalidEnvelope;
        let root = closed(
            &self.0,
            &[
                "specversion",
                "id",
                "source",
                "type",
                "subject",
                "time",
                "datacontenttype",
                "dataschema",
                "data",
            ],
            &[],
        )?;
        if root["specversion"] != "1.0"
            || root["datacontenttype"] != "application/json"
            || root["dataschema"] != "urn:knowledge-platform:audit:event:1"
            || !uuid(&root["id"], false)
        {
            return Err(fail);
        }
        let kind = string(&root["type"])?;
        let rule = catalog::rule(kind)?;
        if rule.deferred {
            return Err(ValidationError::LegacyReasonContractUnqualified);
        }
        if root["source"] != rule.source || !bounded_text(&root["subject"]) {
            return Err(fail);
        }
        let at = string(&root["time"])?;
        if !at.ends_with('Z') || OffsetDateTime::parse(at, &Rfc3339).is_err() {
            return Err(fail);
        }
        let data = closed(
            &root["data"],
            &[
                "schema_version",
                "category",
                "actor",
                "action",
                "resource",
                "result",
                "correlation",
                "metadata",
                "provenance",
            ],
            &["service_executor", "reason_code"],
        )?;
        if data["schema_version"] != 1
            || data["category"] != rule.category
            || data["action"] != kind
            || data["result"] != rule.result
        {
            return Err(fail);
        }
        // Normal T8 reason evidence is missing from the legacy payload/ledger.
        // Only an explicit, source-backed bootstrap variant can be qualified.
        if kind == "access_policy.changed"
            && data["metadata"].get("bootstrap") != Some(&Value::Bool(true))
        {
            return Err(ValidationError::LegacyReasonContractUnqualified);
        }
        let actor = closed(
            &data["actor"],
            &["identity_provider", "principal_id", "kind"],
            &[],
        )?;
        if !bounded_text(&actor["identity_provider"])
            || !bounded_text(&actor["principal_id"])
            || actor["kind"] != "unknown"
        {
            return Err(fail);
        }
        let resource = closed(&data["resource"], &["type", "id"], &["version_id"])?;
        let resource_type = string(&resource["type"])?;
        if !rule.resources.iter().any(|t| t == resource_type)
            || !uuid(&resource["id"], kind == "authorization.denied")
        {
            return Err(fail);
        }
        if rule.requires_version != resource.contains_key("version_id")
            || resource.get("version_id").is_some_and(|v| !uuid(v, false))
        {
            return Err(fail);
        }
        if kind == "authorization.denied" && resource["id"] != Uuid::nil().to_string() {
            return Err(fail);
        }
        let subject = string(&root["subject"])?;
        let base_subject = format!(
            "{}/{}",
            resource_type.to_ascii_lowercase(),
            string(&resource["id"])?
        );
        let mut subjects = vec![base_subject.clone()];
        if let Some(version) = resource.get("version_id") {
            let version_subject = format!("{base_subject}/version/{}", string(version)?);
            subjects.push(version_subject.clone());
            if let Some(representation) = data["metadata"].get("representation_id") {
                subjects.push(format!(
                    "{version_subject}/representation/{}",
                    string(representation)?
                ));
            }
        }
        if kind == "document.diff.result_access_granted" {
            subjects.push(format!("{base_subject}/diff"));
        }
        if kind == "authorization.denied" {
            subjects = vec!["authorization/denied".to_owned()];
        }
        if !subjects.iter().any(|allowed| allowed == subject) {
            return Err(fail);
        }
        let correlation = closed(
            &data["correlation"],
            &[],
            &[
                "operation_id",
                "publish_operation_id",
                "trace_id",
                "legacy_correlation_id",
            ],
        )?;
        for key in ["operation_id", "publish_operation_id"] {
            if correlation.get(key).is_some_and(|v| !uuid(v, false)) {
                return Err(fail);
            }
        }
        if correlation
            .get("trace_id")
            .is_some_and(|v| !v.as_str().is_some_and(trace_id))
        {
            return Err(fail);
        }
        if correlation
            .get("legacy_correlation_id")
            .is_some_and(|v| !identifier(v))
        {
            return Err(fail);
        }
        let provenance = closed(
            &data["provenance"],
            &["source_format", "adapter_version"],
            &[],
        )?;
        if provenance["source_format"] != "document-audit-outbox-v0"
            || provenance["adapter_version"] != 1
        {
            return Err(fail);
        }
        let metadata = data["metadata"]
            .as_object()
            .ok_or(ValidationError::InvalidMetadata)?;
        if metadata.contains_key("reason") {
            return Err(ValidationError::LegacyReasonContractUnqualified);
        }
        if metadata.len() > 64 || rule.required.iter().any(|k| !metadata.contains_key(k)) {
            return Err(ValidationError::InvalidMetadata);
        }
        for (key, value) in metadata {
            let field = rule
                .fields
                .get(key)
                .ok_or(ValidationError::InvalidMetadata)?;
            if !field_valid(value, field) {
                return Err(ValidationError::InvalidMetadata);
            }
        }
        if kind == "access_policy.changed"
            && (metadata.get("target_type") != Some(&resource["type"])
                || metadata.get("target_id") != Some(&resource["id"]))
        {
            return Err(fail);
        }
        for key in ["documentId", "document_id"] {
            if metadata.get(key).is_some_and(|v| v != &resource["id"]) {
                return Err(fail);
            }
        }
        for key in [
            "documentVersionId",
            "document_version_id",
            "target_version_id",
        ] {
            if metadata
                .get(key)
                .is_some_and(|v| resource.get("version_id") != Some(v))
            {
                return Err(fail);
            }
        }
        let operation = metadata
            .get("operation_id")
            .or_else(|| metadata.get("operationId"));
        if correlation.get("operation_id") != operation
            || correlation.get("publish_operation_id") != metadata.get("publishOperationId")
            || (correlation.contains_key("trace_id")
                && correlation.contains_key("legacy_correlation_id"))
        {
            return Err(fail);
        }
        if let Some(legacy) = metadata.get("serviceExecutor") {
            let executor = data.get("service_executor").ok_or(fail)?;
            let executor = closed(executor, &["identity_provider", "principal_id"], &[])?;
            if executor["identity_provider"] != legacy["identityProvider"]
                || executor["principal_id"] != legacy["principalId"]
            {
                return Err(fail);
            }
        } else if data.contains_key("service_executor") {
            return Err(fail);
        }
        let reason = metadata
            .get("reason_code")
            .or_else(|| metadata.get("terminalReason"));
        if data.get("reason_code") != reason {
            return Err(fail);
        }
        if serde_json::to_vec(&self.0).map_err(|_| fail)?.len() > 32 * 1024 {
            return Err(ValidationError::EnvelopeTooLarge);
        }
        if !crate::schema_validate(&self.0)? {
            return Err(fail);
        }
        Ok(())
    }
}
fn closed<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Map<String, Value>, ValidationError> {
    let obj = value.as_object().ok_or(ValidationError::InvalidEnvelope)?;
    if required.iter().any(|k| !obj.contains_key(*k))
        || obj
            .keys()
            .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(ValidationError::InvalidEnvelope);
    }
    Ok(obj)
}
fn string(value: &Value) -> Result<&str, ValidationError> {
    value.as_str().ok_or(ValidationError::InvalidEnvelope)
}
pub(crate) fn bounded_text(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 512 && s.trim() == s && !s.chars().any(char::is_control)
    })
}
fn identifier(value: &Value) -> bool {
    bounded_text(value)
        && value.as_str().is_some_and(|s| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-/".contains(&b))
        })
}
pub(crate) fn trace_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && value.bytes().any(|b| b != b'0')
}
fn uuid(value: &Value, allow_nil: bool) -> bool {
    value.as_str().is_some_and(|s| {
        Uuid::parse_str(s).is_ok_and(|id| (allow_nil || !id.is_nil()) && id.to_string() == s)
    })
}
fn field_valid(value: &Value, rule: &FieldRule) -> bool {
    match rule.kind.as_str() {
        "uuid" => uuid(value, false),
        "nullable_uuid" => value.is_null() || uuid(value, false),
        "counter" => value.as_i64().is_some_and(|n| n >= 0),
        "positive_counter" => value.as_i64().is_some_and(|n| n > 0),
        "boolean" => value.is_boolean(),
        "enum" => rule.values.contains(value),
        "digest" => value.as_array().is_some_and(|a| {
            a.len() == 32 && a.iter().all(|v| v.as_u64().is_some_and(|n| n <= 255))
        }),
        "principal" => closed(value, &["identityProvider", "principalId"], &[])
            .is_ok_and(|o| bounded_text(&o["identityProvider"]) && bounded_text(&o["principalId"])),
        "legacy_time" => legacy_time(value),
        _ => false,
    }
}
pub(crate) fn legacy_time(value: &Value) -> bool {
    let Some(a) = value.as_array() else {
        return false;
    };
    if a.len() != 9 {
        return false;
    }
    let Some(n) = a.iter().map(Value::as_i64).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let Ok(y) = i32::try_from(n[0]) else {
        return false;
    };
    let Ok(day) = u16::try_from(n[1]) else {
        return false;
    };
    let (Ok(h), Ok(m), Ok(s), Ok(ns)) = (
        u8::try_from(n[2]),
        u8::try_from(n[3]),
        u8::try_from(n[4]),
        u32::try_from(n[5]),
    ) else {
        return false;
    };
    let (Ok(oh), Ok(om), Ok(os)) = (i8::try_from(n[6]), i8::try_from(n[7]), i8::try_from(n[8]))
    else {
        return false;
    };
    Date::from_ordinal_date(y, day).is_ok()
        && Time::from_hms_nano(h, m, s, ns).is_ok()
        && UtcOffset::from_hms(oh, om, os).is_ok_and(|offset| offset.as_hms() == (oh, om, os))
}
