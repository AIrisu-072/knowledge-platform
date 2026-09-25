use std::io::{Read, Write};

use document_semantic_inspection_core::{
    WorkerProtocolVersion, WorkerResponse, canonical_worker_response_bytes,
};

use crate::{
    AdapterProfile, CsvAdapter, DocxAdapter, HtmlAdapter, SemanticAdapter, SemanticAdapterOutput,
    TextAdapter, WorkerFailure, WorkerFailureCode, decode_request_bounded, guard_worker_execution,
    prepare_input_bounded,
};

/// Runs the worker over a bounded request and inherited input.
pub fn run_worker_shell<R, O, E>(
    request_bytes: &[u8],
    input: &mut R,
    stdout: &mut O,
    stderr: &mut E,
    max_request_bytes: usize,
    max_input_bytes: usize,
) -> i32
where
    R: Read,
    O: Write,
    E: Write,
{
    let outcome = guard_worker_execution(|| -> Result<Vec<u8>, WorkerFailure> {
        let request = decode_request_bounded(request_bytes, max_request_bytes)?;
        let prepared = prepare_input_bounded(&request, input, max_input_bytes)?;
        let profile = AdapterProfile::default();
        let adapter_output = match prepared.detected_format() {
            document_semantic_inspection_core::FormatId::Txt => {
                TextAdapter.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Csv => {
                CsvAdapter.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Html => {
                HtmlAdapter.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Docx => {
                DocxAdapter.inspect(prepared.bytes(), &profile)?
            }
            format => {
                return Err(WorkerFailure::new(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("semantic adapter is not promoted yet for detected format {format:?}"),
                ));
            }
        };

        let response = worker_response(&request, &prepared, adapter_output);
        response.validate().map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("adapter produced an invalid worker result: {error}"),
            )
        })?;
        canonical_worker_response_bytes(&response).map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("adapter result could not be canonicalized: {error}"),
            )
        })
    });

    match outcome {
        Ok(response_bytes) => {
            if stdout
                .write_all(&response_bytes)
                .and_then(|()| stdout.write_all(b"\n"))
                .is_ok()
            {
                0
            } else {
                let failure = WorkerFailure::new(
                    WorkerFailureCode::ExtractorUnavailable,
                    "failed to write the structured worker result",
                );
                write_failure(stderr, &failure);
                exit_code(failure.code())
            }
        }
        Err(failure) => {
            write_failure(stderr, &failure);
            exit_code(failure.code())
        }
    }
}

fn worker_response(
    request: &document_semantic_inspection_core::WorkerRequest,
    prepared: &crate::PreparedInput,
    adapter_output: SemanticAdapterOutput,
) -> WorkerResponse {
    WorkerResponse {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: request.inspection_profile_version,
        observed_raw_content_hash: *prepared.observed_raw_content_hash(),
        observed_size_bytes: prepared.observed_size_bytes(),
        detected_format: prepared.detected_format(),
        semantic_fingerprint: adapter_output.semantic_fingerprint(),
        semantic_capabilities: adapter_output.semantic_capabilities().to_vec(),
        editorial_provenance: adapter_output.editorial_provenance().clone(),
        external_dependencies: Vec::new(),
        digital_signature_evidence: Vec::new(),
        extractor_provenance: adapter_output.extractor_provenance().clone(),
        diagnostics: Vec::new(),
    }
}

pub fn write_failure<W: Write>(writer: &mut W, failure: &WorkerFailure) {
    if serde_json::to_writer(&mut *writer, failure).is_err() {
        let _ = writer.write_all(
            br#"{"code":"invalid_worker_result","message":"failed to serialize worker failure"}"#,
        );
    }
}

const fn exit_code(code: WorkerFailureCode) -> i32 {
    match code {
        WorkerFailureCode::MalformedRequest
        | WorkerFailureCode::UnsupportedDocumentFormat
        | WorkerFailureCode::FormatMismatch
        | WorkerFailureCode::RawBindingMismatch
        | WorkerFailureCode::InspectionResourceLimitExceeded
        | WorkerFailureCode::SemanticExtractionFailed
        | WorkerFailureCode::ParserDisagreement
        | WorkerFailureCode::UnsupportedSemanticConstruct
        | WorkerFailureCode::RequiresOcr
        | WorkerFailureCode::EncryptedContentUnsupported => 65,
        WorkerFailureCode::ExtractorUnavailable
        | WorkerFailureCode::InspectionTimeout
        | WorkerFailureCode::InvalidWorkerResult => 69,
        WorkerFailureCode::WorkerPanicked => 70,
    }
}
