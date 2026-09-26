use serde::Serialize;
use serde_json::{Map, Value};

use crate::{CoreError, WorkerResponse};

pub fn canonical_worker_response_bytes(response: &WorkerResponse) -> Result<Vec<u8>, CoreError> {
    let mut normalized = response.clone();
    normalize_unordered_collections(&mut normalized);
    normalized.validate()?;
    canonical_json_bytes(&normalized)
}

fn normalize_unordered_collections(response: &mut WorkerResponse) {
    response
        .semantic_capabilities
        .sort_by(|left, right| left.capability_id.cmp(&right.capability_id));
    response.external_dependencies.sort_by(|left, right| {
        (
            &left.dependency_kind,
            &left.normalized_reference,
            &left.source_locator,
        )
            .cmp(&(
                &right.dependency_kind,
                &right.normalized_reference,
                &right.source_locator,
            ))
    });
    response.digital_signature_evidence.sort_by(|left, right| {
        (
            &left.signature_type,
            &left.certificate_fingerprint,
            &left.signed_at,
        )
            .cmp(&(
                &right.signature_type,
                &right.certificate_fingerprint,
                &right.signed_at,
            ))
    });
    for signature in &mut response.digital_signature_evidence {
        signature.covered_content.sort();
        signature.covered_content.dedup();
        signature.validation_diagnostics.sort();
        signature.validation_diagnostics.dedup();
    }

    response.editorial_provenance.document_author_labels.sort();
    response.editorial_provenance.document_author_labels.dedup();

    response
        .extractor_provenance
        .parser_libraries
        .sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    response
        .extractor_provenance
        .native_dependency_identity
        .sort_by(|left, right| {
            (&left.name, &left.version, &left.sha256).cmp(&(
                &right.name,
                &right.version,
                &right.sha256,
            ))
        });
    response
        .diagnostics
        .sort_by(|left, right| (&left.code, &left.message).cmp(&(&right.code, &right.message)));
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CoreError> {
    let value = serde_json::to_value(value)?;
    Ok(serde_json::to_vec(&canonicalize(value))?)
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));

            let mut sorted = Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize(value));
            }
            Value::Object(sorted)
        }
        scalar => scalar,
    }
}
