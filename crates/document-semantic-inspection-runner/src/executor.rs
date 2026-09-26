use std::sync::Arc;

use document_application::{ContentReader, InspectionExecutionError, SemanticInspectionExecutor};
use document_semantic_inspection_core::{WorkerRequest, WorkerResponse};
use tokio::io::AsyncReadExt;

use crate::{LinuxSandboxRunner, RunnerConfig, RunnerError, RunnerInput};

const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;

/// Infrastructure bridge from the Application's read-only content stream to
/// one fresh sandboxed worker process. No storage key or FileId crosses it.
pub struct RunnerInspectionExecutor {
    runner: Arc<LinuxSandboxRunner>,
}

impl RunnerInspectionExecutor {
    pub fn new(config: RunnerConfig) -> Result<Self, RunnerError> {
        Ok(Self {
            runner: Arc::new(LinuxSandboxRunner::new(config)?),
        })
    }
}

impl SemanticInspectionExecutor for RunnerInspectionExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        let mut bounded = content.take((MAX_INPUT_BYTES + 1) as u64);
        let mut bytes = Vec::new();
        bounded
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| InspectionExecutionError::ExtractorUnavailable)?;
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(InspectionExecutionError::InspectionResourceLimitExceeded);
        }
        let runner = self.runner.clone();
        tokio::task::spawn_blocking(move || {
            runner.inspect(RunnerInput {
                bytes: &bytes,
                declared_media_type: &request.declared_media_type,
                expected_raw_content_hash: request.expected_raw_content_hash,
                expected_size_bytes: request.expected_size_bytes,
                trace_context: request.trace_context,
            })
        })
        .await
        .map_err(|_| InspectionExecutionError::ExtractorUnavailable)?
        .map_err(map_runner_error)
    }
}

fn map_runner_error(error: RunnerError) -> InspectionExecutionError {
    match error {
        RunnerError::InspectionTimeout { .. } => InspectionExecutionError::InspectionTimeout,
        RunnerError::InspectionResourceLimitExceeded { .. } => {
            InspectionExecutionError::InspectionResourceLimitExceeded
        }
        RunnerError::ExtractorUnavailable { .. } => InspectionExecutionError::ExtractorUnavailable,
        RunnerError::InvalidWorkerResult { .. } => InspectionExecutionError::InvalidWorkerResult,
        RunnerError::RawBindingMismatch { .. } => InspectionExecutionError::RawBindingMismatch,
        RunnerError::WorkerFailure { code } => match code {
            "unsupported_document_format" => InspectionExecutionError::UnsupportedDocumentFormat,
            "requires_ocr" => InspectionExecutionError::RequiresOcr,
            "encrypted_content_unsupported" => {
                InspectionExecutionError::EncryptedContentUnsupported
            }
            "format_mismatch" => InspectionExecutionError::FormatMismatch,
            "semantic_extraction_failed" => InspectionExecutionError::SemanticExtractionFailed,
            "parser_disagreement" => InspectionExecutionError::ParserDisagreement,
            "unsupported_semantic_construct" => {
                InspectionExecutionError::UnsupportedSemanticConstruct
            }
            _ => InspectionExecutionError::InvalidWorkerResult,
        },
    }
}
