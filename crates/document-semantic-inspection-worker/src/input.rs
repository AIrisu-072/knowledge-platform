use std::{
    fs::File,
    io::Read,
    panic::{AssertUnwindSafe, catch_unwind},
};

use document_semantic_inspection_core::{FormatId, WorkerRequest};
use sha2::{Digest, Sha256};

use crate::{WorkerFailure, WorkerFailureCode, detect_format};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInput {
    bytes: Vec<u8>,
    observed_raw_content_hash: [u8; 32],
    observed_size_bytes: u64,
    detected_format: FormatId,
}

impl PreparedInput {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn observed_raw_content_hash(&self) -> &[u8; 32] {
        &self.observed_raw_content_hash
    }

    pub const fn observed_size_bytes(&self) -> u64 {
        self.observed_size_bytes
    }

    pub const fn detected_format(&self) -> FormatId {
        self.detected_format
    }
}

pub fn decode_request_bounded(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<WorkerRequest, WorkerFailure> {
    if bytes.len() > max_bytes {
        return Err(WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!(
                "worker request exceeded configured bound: observed {} bytes, limit {max_bytes}",
                bytes.len()
            ),
        ));
    }

    serde_json::from_slice(bytes).map_err(|error| {
        WorkerFailure::new(
            WorkerFailureCode::MalformedRequest,
            format!("worker request is not valid dsi-worker-v0 JSON: {error}"),
        )
    })
}

pub fn prepare_input_bounded<R: Read>(
    request: &WorkerRequest,
    reader: &mut R,
    max_input_bytes: usize,
) -> Result<PreparedInput, WorkerFailure> {
    let read_limit = max_input_bytes.saturating_add(1) as u64;
    let mut limited = reader.take(read_limit);
    let mut bytes = Vec::with_capacity(max_input_bytes.min(64 * 1024));
    limited.read_to_end(&mut bytes).map_err(|error| {
        WorkerFailure::new(
            WorkerFailureCode::ExtractorUnavailable,
            format!("failed to read inherited input: {error}"),
        )
    })?;

    if bytes.len() > max_input_bytes {
        return Err(WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!(
                "input exceeded configured bound: observed at least {} bytes, limit {max_input_bytes}",
                bytes.len()
            ),
        ));
    }

    let observed_size_bytes = u64::try_from(bytes.len()).map_err(|_| {
        WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "input size cannot be represented as u64",
        )
    })?;
    let observed_raw_content_hash: [u8; 32] = Sha256::digest(&bytes).into();

    if observed_raw_content_hash != request.expected_raw_content_hash
        || observed_size_bytes != request.expected_size_bytes
    {
        return Err(WorkerFailure::new(
            WorkerFailureCode::RawBindingMismatch,
            "worker-observed raw hash/size does not match the authoritative binding",
        ));
    }

    let detected_format = detect_format(&bytes, &request.declared_media_type)?;

    Ok(PreparedInput {
        bytes,
        observed_raw_content_hash,
        observed_size_bytes,
        detected_format,
    })
}

pub fn open_inherited_input(fd: i32) -> Result<File, WorkerFailure> {
    #[cfg(target_os = "linux")]
    let path = format!("/proc/self/fd/{fd}");
    #[cfg(all(unix, not(target_os = "linux")))]
    let path = format!("/dev/fd/{fd}");

    #[cfg(unix)]
    {
        File::open(path).map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::ExtractorUnavailable,
                format!("cannot open inherited read-only input descriptor: {error}"),
            )
        })
    }

    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(WorkerFailure::new(
            WorkerFailureCode::ExtractorUnavailable,
            "inherited file descriptor input is unavailable on this platform",
        ))
    }
}

pub fn guard_worker_execution<T, F>(operation: F) -> Result<T, WorkerFailure>
where
    F: FnOnce() -> Result<T, WorkerFailure>,
{
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(_) => Err(WorkerFailure::new(
            WorkerFailureCode::WorkerPanicked,
            "worker parser zone panicked; no success result is emitted",
        )),
    }
}
