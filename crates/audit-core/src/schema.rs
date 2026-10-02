//! JSON Schema 2020-12 validation with mandatory bounded-contract keywords.
use crate::ValidationError;
use jsonschema::{Keyword, Validator};
use serde_json::Value;
use std::sync::OnceLock;

struct Utf8Limit(usize);
impl<'i> Keyword<'i> for Utf8Limit {
    fn validate(&self, value: &'i Value) -> Result<(), jsonschema::ValidationError<'i>> {
        if self.is_valid(value) {
            Ok(())
        } else {
            Err(jsonschema::ValidationError::custom(
                "audit string byte limit",
            ))
        }
    }
    fn is_valid(&self, value: &Value) -> bool {
        value.as_str().is_some_and(|v| v.len() <= self.0)
    }
}
struct WireLimit(usize);
impl<'i> Keyword<'i> for WireLimit {
    fn validate(&self, value: &'i Value) -> Result<(), jsonschema::ValidationError<'i>> {
        if self.is_valid(value) {
            Ok(())
        } else {
            Err(jsonschema::ValidationError::custom(
                "audit envelope byte limit",
            ))
        }
    }
    fn is_valid(&self, value: &Value) -> bool {
        serde_json::to_vec(value).is_ok_and(|v| v.len() <= self.0)
    }
}
struct LegacyTime;
impl<'i> Keyword<'i> for LegacyTime {
    fn validate(&self, value: &'i Value) -> Result<(), jsonschema::ValidationError<'i>> {
        if self.is_valid(value) {
            Ok(())
        } else {
            Err(jsonschema::ValidationError::custom(
                "invalid legacy time tuple",
            ))
        }
    }
    fn is_valid(&self, value: &Value) -> bool {
        crate::event::legacy_time(value)
    }
}
fn validator() -> Result<&'static Validator, ValidationError> {
    static SCHEMA: OnceLock<Result<Validator, ValidationError>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            let schema: Value = serde_json::from_str(include_str!(
                "../../../spec/telemetry/audit-event.schema.json"
            ))
            .map_err(|_| ValidationError::InvalidCatalog)?;
            jsonschema::options()
                .should_validate_formats(true)
                .with_keyword("auditMaxUtf8Bytes", |_, value, _| {
                    Ok(Box::new(Utf8Limit(
                        value
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(|| {
                                jsonschema::ValidationError::custom("invalid byte quota")
                            })?,
                    )))
                })
                .with_keyword("auditEnvelopeBytes", |_, value, _| {
                    Ok(Box::new(WireLimit(
                        value
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(|| {
                                jsonschema::ValidationError::custom("invalid wire quota")
                            })?,
                    )))
                })
                .with_keyword("auditLegacyTime", |_, _, _| Ok(Box::new(LegacyTime)))
                .build(&schema)
                .map_err(|_| ValidationError::InvalidCatalog)
        })
        .as_ref()
        .map_err(|e| *e)
}
/// Structural schema validation. Producer/resource/correlation cross-bindings
/// are additionally mandatory through `AuditEnvelope::validate`.
/// Raw input must pass `AuditEnvelope::from_json`'s pre-parse wire bound.
pub fn schema_validate(value: &Value) -> Result<bool, ValidationError> {
    // Bound even a caller-supplied in-memory Value before the schema engine or
    // root wire keyword can render it. This is not SQL source admission.
    if !shape(value, 0, &mut 0) {
        return Ok(false);
    }
    Ok(validator()?.is_valid(value))
}
fn shape(value: &Value, depth: usize, nodes: &mut usize) -> bool {
    *nodes += 1;
    if depth > 8 || *nodes > 512 {
        return false;
    }
    match value {
        Value::String(s) => s.len() <= 512,
        Value::Number(n) => n.as_i64().is_some(),
        Value::Array(a) => a.iter().all(|v| shape(v, depth + 1, nodes)),
        Value::Object(o) => {
            o.len() <= 64
                && o.iter()
                    .all(|(k, v)| k.len() <= 128 && shape(v, depth + 1, nodes))
        }
        _ => true,
    }
}
