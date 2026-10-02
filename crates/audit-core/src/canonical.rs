use crate::{AuditEnvelope, ValidationError};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// audit-json-v1: sorted object keys, retained array order, integer-only numbers.
/// It is deliberately not represented as a general-purpose RFC8785 encoder.
pub fn canonical_bytes(event: &AuditEnvelope) -> Result<Vec<u8>, ValidationError> {
    event.validate()?;
    let value = sorted(event.as_value());
    serde_json::to_vec(&value).map_err(|_| ValidationError::InvalidEnvelope)
}
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered: std::collections::BTreeMap<_, _> = object.iter().collect();
            Value::Object(
                ordered
                    .into_iter()
                    .map(|(k, v)| (k.clone(), sorted(v)))
                    .collect(),
            )
        }
        Value::Array(array) => Value::Array(array.iter().map(sorted).collect()),
        other => other.clone(),
    }
}
pub fn event_digest(event: &AuditEnvelope) -> Result<[u8; 32], ValidationError> {
    Ok(Sha256::digest(canonical_bytes(event)?).into())
}
