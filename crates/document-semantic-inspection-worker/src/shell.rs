use std::io::{Read, Write};

use crate::{
    WorkerFailure, WorkerFailureCode, decode_request_bounded, guard_worker_execution,
    prepare_input_bounded,
};

/// Runs the Task 3 worker shell over a bounded request and inherited input.
///
/// Task 3 deliberately stops before format-specific semantic adapters are
/// promoted. A fully validated input therefore ends in a controlled
/// `SemanticExtractionFailed` response rather than a fabricated success
/// `WorkerResponse`.
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
    let outcome = guard_worker_execution(|| -> Result<(), WorkerFailure> {
        let request = decode_request_bounded(request_bytes, max_request_bytes)?;
        let prepared = prepare_input_bounded(&request, input, max_input_bytes)?;

        Err(WorkerFailure::new(
            WorkerFailureCode::SemanticExtractionFailed,
            format!(
                "semantic adapter is not promoted yet for detected format {:?}",
                prepared.detected_format()
            ),
        ))
    });

    match outcome {
        Ok(()) => {
            // No Task 3 code path may manufacture a semantic success result.
            // Keep stdout untouched until a later format-adapter task supplies
            // a fully validated WorkerResponse.
            let _ = stdout;
            0
        }
        Err(failure) => {
            let _ = stdout;
            write_failure(stderr, &failure);
            exit_code(failure.code())
        }
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
