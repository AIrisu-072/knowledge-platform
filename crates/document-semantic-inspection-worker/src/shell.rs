use std::io::{self, Read, Write};

use document_semantic_inspection_core::{
    DigitalSignatureEvidence, FormatId, WorkerProtocolVersion, WorkerResponse,
    canonical_worker_response_bytes,
};
use serde::Serialize;

use crate::{
    AdapterProfile, CsvAdapter, DocxAdapter, HtmlAdapter, PdfAdapter, PptxAdapter, SemanticAdapter,
    SemanticAdapterOutput, SignatureInspector, SignatureTrustContext, SpreadsheetAdapter,
    TextAdapter, WorkerFailure, WorkerFailureCode, decode_request_bounded, guard_worker_execution,
    prepare_input_bounded,
};

pub(crate) const MAX_STRUCTURED_RESULT_BYTES: usize = 16 * 1024 * 1024;

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
    run_worker_shell_with_signature_trust(
        request_bytes,
        input,
        stdout,
        stderr,
        max_request_bytes,
        max_input_bytes,
        &SignatureTrustContext::default(),
    )
}

/// Runs the worker with explicit offline trust anchors and CRLs supplied by the caller.
pub fn run_worker_shell_with_signature_trust<R, O, E>(
    request_bytes: &[u8],
    input: &mut R,
    stdout: &mut O,
    stderr: &mut E,
    max_request_bytes: usize,
    max_input_bytes: usize,
    trust: &SignatureTrustContext,
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
            document_semantic_inspection_core::FormatId::Pdf => {
                PdfAdapter.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Xlsx => {
                SpreadsheetAdapter::XLSX.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Xlsm => {
                SpreadsheetAdapter::XLSM.inspect(prepared.bytes(), &profile)?
            }
            document_semantic_inspection_core::FormatId::Pptx => {
                PptxAdapter.inspect(prepared.bytes(), &profile)?
            }
        };

        let signatures = match prepared.detected_format() {
            FormatId::Pdf => SignatureInspector::inspect_pdf_signatures(prepared.bytes(), trust)?,
            FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx => {
                SignatureInspector::verify_ooxml_package(prepared.bytes(), trust)
                    .map_err(WorkerFailure::from)?
            }
            FormatId::Txt | FormatId::Csv | FormatId::Html => Vec::new(),
        };
        let response = worker_response(&request, &prepared, adapter_output, signatures);
        response.validate().map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("adapter produced an invalid worker result: {error}"),
            )
        })?;
        canonical_response_bounded(&response)
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

fn canonical_response_bounded(response: &WorkerResponse) -> Result<Vec<u8>, WorkerFailure> {
    serialized_json_len_bounded(response)?;
    let response_bytes = canonical_worker_response_bytes(response).map_err(|error| {
        WorkerFailure::new(
            WorkerFailureCode::InvalidWorkerResult,
            format!("adapter result could not be canonicalized: {error}"),
        )
    })?;
    if response_bytes.len() > MAX_STRUCTURED_RESULT_BYTES {
        return Err(WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "structured worker result exceeds its byte bound",
        ));
    }
    Ok(response_bytes)
}

pub(crate) fn serialized_json_len_bounded<T: Serialize>(value: &T) -> Result<usize, WorkerFailure> {
    let mut counter = BoundedJsonCounter::default();
    if let Err(error) = serde_json::to_writer(&mut counter, value) {
        if counter.exceeded {
            return Err(WorkerFailure::new(
                WorkerFailureCode::InspectionResourceLimitExceeded,
                "structured worker result exceeds its byte bound",
            ));
        }
        return Err(WorkerFailure::new(
            WorkerFailureCode::InvalidWorkerResult,
            format!("adapter result could not be measured: {error}"),
        ));
    }
    Ok(counter.observed)
}

#[derive(Default)]
struct BoundedJsonCounter {
    observed: usize,
    exceeded: bool,
}

impl Write for BoundedJsonCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let next = self.observed.checked_add(buf.len());
        if next.is_none_or(|bytes| bytes > MAX_STRUCTURED_RESULT_BYTES) {
            self.exceeded = true;
            return Err(io::Error::other("structured result byte bound exceeded"));
        }
        self.observed = next.expect("checked above");
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn worker_response(
    request: &document_semantic_inspection_core::WorkerRequest,
    prepared: &crate::PreparedInput,
    adapter_output: SemanticAdapterOutput,
    signatures: Vec<DigitalSignatureEvidence>,
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
        external_dependencies: adapter_output.external_dependencies().to_vec(),
        digital_signature_evidence: signatures,
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

#[cfg(test)]
mod result_size_boundary_tests {
    use document_semantic_inspection_core::{
        EditorialProvenance, ExternalDependency, ExtractorProvenance, FormatId,
        InspectionProfileVersion, SemanticFingerprint,
    };

    use super::*;

    #[test]
    fn exact_structured_result_bound_is_accepted_and_one_over_fails() {
        let mut response = WorkerResponse {
            protocol_version: WorkerProtocolVersion::V0,
            inspection_profile_version: InspectionProfileVersion::DsiV0,
            observed_raw_content_hash: [7; 32],
            observed_size_bytes: 3,
            detected_format: FormatId::Xlsx,
            semantic_fingerprint: SemanticFingerprint::sha256_from_slice(&[1; 32])
                .expect("valid fingerprint"),
            semantic_capabilities: Vec::new(),
            editorial_provenance: EditorialProvenance::default(),
            external_dependencies: vec![ExternalDependency {
                dependency_kind: "external_workbook".into(),
                normalized_reference: String::new(),
                source_locator: "xl/externalLinks/externalLink1.xml".into(),
                version_significant: true,
            }],
            digital_signature_evidence: Vec::new(),
            extractor_provenance: ExtractorProvenance {
                worker_build_id: "test".into(),
                adapter_id: "spreadsheet".into(),
                adapter_version: "dsi-v0".into(),
                parser_libraries: Vec::new(),
                native_dependency_identity: Vec::new(),
            },
            diagnostics: Vec::new(),
        };
        let base_bytes = serde_json::to_vec(&response)
            .expect("serialize baseline")
            .len();
        response.external_dependencies[0].normalized_reference =
            "a".repeat(MAX_STRUCTURED_RESULT_BYTES - base_bytes);

        let exact = canonical_response_bounded(&response).expect("exact byte bound is allowed");
        assert_eq!(exact.len(), MAX_STRUCTURED_RESULT_BYTES);

        response.external_dependencies[0]
            .normalized_reference
            .push('a');
        let failure = canonical_response_bounded(&response)
            .expect_err("one byte over the bound must fail before canonicalization");
        assert_eq!(
            failure.code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }
}
