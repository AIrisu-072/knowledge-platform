use crate::{AuditEnvelope, ValidationError, catalog};
use serde::Deserialize;
use serde_json::{Value, json};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

/// This input is admitted by the source SQL adapter before construction. Parsing
/// is independently wire-bounded; it cannot make an earlier unbounded fetch safe.
pub struct LegacyAuditRow(LegacyFields);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyFields {
    event_id: String,
    event_type: String,
    source: String,
    subject: String,
    actor_identity_provider: String,
    actor_principal_id: String,
    resource_type: String,
    resource_id: String,
    resource_version_id: Option<String>,
    result: String,
    trace_id: Option<String>,
    data: Value,
    occurred_at: String,
}
impl LegacyAuditRow {
    pub fn from_json(bytes: &[u8]) -> Result<Self, ValidationError> {
        if bytes.len() > 24 * 1024 {
            return Err(ValidationError::LegacyRowTooLarge);
        }
        serde_json::from_value(crate::json::parse(bytes)?)
            .map(Self)
            .map_err(|_| ValidationError::InvalidEnvelope)
    }
}
impl AuditEnvelope {
    pub fn from_legacy(row: LegacyAuditRow) -> Result<Self, ValidationError> {
        let row = row.0;
        let rule = catalog::rule(&row.event_type)?;
        if rule.deferred
            || row.data.get("reason").is_some()
            || (row.event_type == "access_policy.changed"
                && row.data.get("bootstrap") != Some(&Value::Bool(true)))
        {
            return Err(ValidationError::LegacyReasonContractUnqualified);
        }
        let at = OffsetDateTime::parse(&row.occurred_at, &Rfc3339)
            .map_err(|_| ValidationError::InvalidEnvelope)?
            .to_offset(UtcOffset::UTC)
            .format(&Rfc3339)
            .map_err(|_| ValidationError::InvalidEnvelope)?;
        let mut correlation = json!({});
        if let Some(trace) = row.trace_id {
            let key = if crate::event::trace_id(&trace) {
                "trace_id"
            } else {
                "legacy_correlation_id"
            };
            correlation[key] = json!(trace);
        }
        for key in ["publishOperationId", "operation_id", "operationId"] {
            if let Some(id) = row.data.get(key) {
                let target = if key == "publishOperationId" {
                    "publish_operation_id"
                } else {
                    "operation_id"
                };
                correlation[target] = id.clone();
            }
        }
        let mut resource = json!({"type":row.resource_type,"id":row.resource_id});
        if let Some(id) = row.resource_version_id {
            resource["version_id"] = json!(id);
        }
        let mut data = json!({
            "schema_version":1, "category":rule.category,
            "actor":{"identity_provider":row.actor_identity_provider,"principal_id":row.actor_principal_id,"kind":"unknown"},
            "action":row.event_type,"resource":resource,"result":row.result,
            "correlation":correlation,"metadata":row.data,
            "provenance":{"source_format":"document-audit-outbox-v0","adapter_version":1}
        });
        if let Some(executor) = data["metadata"].get("serviceExecutor").cloned() {
            data["service_executor"] = json!({"identity_provider":executor["identityProvider"],"principal_id":executor["principalId"]});
        }
        for key in ["reason_code", "terminalReason"] {
            if let Some(reason) = data["metadata"].get(key).cloned() {
                data["reason_code"] = reason;
            }
        }
        let value = json!({
            "specversion":"1.0","id":row.event_id,"source":row.source,"type":row.event_type,
            "subject":row.subject,"time":at,"datacontenttype":"application/json",
            "dataschema":"urn:knowledge-platform:audit:event:1","data":data
        });
        let event = Self::new(value);
        event.validate()?;
        Ok(event)
    }
}

impl std::fmt::Debug for LegacyAuditRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LegacyAuditRow(<redacted>)")
    }
}
