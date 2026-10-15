//! Diagnostic-only adapter over the unchanged mandatory Linux sandbox runner.
//! No worker messages, document contents, paths or identity strings are emitted.
use std::{fs::File, io::Read};

use document_semantic_inspection_core::{FormatId, SignatureValidity};
use document_semantic_inspection_runner::{
    LinuxSandboxRunner, RunnerConfig, RunnerError, RunnerInput,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;

fn parse_hash(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return None;
    }
    let mut hash = [0_u8; 32];
    for (index, target) in hash.iter_mut().enumerate() {
        *target = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(hash)
}

fn failure_code(error: &RunnerError) -> &'static str {
    match error {
        RunnerError::InspectionTimeout { .. } => "inspection_timeout",
        RunnerError::InspectionResourceLimitExceeded { .. } => "inspection_resource_limit_exceeded",
        RunnerError::ExtractorUnavailable { .. } => "extractor_unavailable",
        RunnerError::InvalidWorkerResult { .. } => "invalid_worker_result",
        RunnerError::RawBindingMismatch { .. } => "raw_binding_mismatch",
        RunnerError::WorkerFailure { code } => match *code {
            "unsupported_document_format" => "unsupported_document_format",
            "requires_ocr" => "requires_ocr",
            "encrypted_content_unsupported" => "encrypted_content_unsupported",
            "format_mismatch" => "format_mismatch",
            "raw_binding_mismatch" => "raw_binding_mismatch",
            "semantic_extraction_failed" => "semantic_extraction_failed",
            "parser_disagreement" => "parser_disagreement",
            "unsupported_semantic_construct" => "unsupported_semantic_construct",
            "malformed_request" => "malformed_request",
            "worker_panicked" => "worker_panicked",
            _ => "invalid_worker_result",
        },
    }
}

fn inspect(args: &[String]) -> Option<Value> {
    if args.len() != 6 || args[5] != "application/pdf" {
        return None;
    }
    let expected_hash = parse_hash(&args[3])?;
    let expected_size: u64 = args[4].parse().ok()?;
    if expected_size == 0 || expected_size > MAX_INPUT_BYTES {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&args[0]).ok()?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(&args[0])
        .ok()?
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() as u64 != expected_size || hash != expected_hash {
        return None;
    }
    let config = RunnerConfig::new(&args[1]).with_pdfium_runtime_dir(&args[2]);
    let result = LinuxSandboxRunner::new(config).and_then(|runner| {
        runner.inspect(RunnerInput {
            bytes: &bytes,
            declared_media_type: "application/pdf",
            expected_raw_content_hash: expected_hash,
            expected_size_bytes: expected_size,
            trace_context: None,
        })
    });
    Some(match result {
        Ok(response) => json!({
            "status": "inspected",
            "rawHashMatches": response.observed_raw_content_hash == expected_hash,
            "sizeMatches": response.observed_size_bytes == expected_size,
            "pdf": response.detected_format == FormatId::Pdf,
            "unresolvedTrackedChanges": response.editorial_provenance.tracked_changes.iter().filter(|change| change.unresolved).count(),
            "embeddedComments": response.editorial_provenance.comments.len(),
            "invalidSignatures": response.digital_signature_evidence.iter().filter(|signature| signature.cryptographic_validity == SignatureValidity::Invalid).count(),
            "unverifiableSignatures": response.digital_signature_evidence.iter().filter(|signature| signature.cryptographic_validity == SignatureValidity::Unverifiable).count(),
        }),
        Err(error) => json!({ "status": "worker-failure", "failureCode": failure_code(&error) }),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = inspect(&args).unwrap_or_else(|| json!({ "status": "unavailable" }));
    println!("{result}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_arguments_fail_closed() {
        assert_eq!(parse_hash(&"ab".repeat(32)), Some([0xab; 32]));
        assert!(parse_hash(&"AB".repeat(32)).is_none());
        assert!(parse_hash("private-sentinel").is_none());
        assert!(inspect(&[]).is_none());
    }

    #[test]
    fn errors_emit_only_fixed_codes() {
        assert_eq!(
            failure_code(&RunnerError::WorkerFailure {
                code: "private-sentinel",
            }),
            "invalid_worker_result"
        );
        assert_eq!(
            failure_code(&RunnerError::ExtractorUnavailable {
                reason: "private-sentinel",
            }),
            "extractor_unavailable"
        );
    }
}
