use std::io::Read;

use document_diff_core::{WorkerDiffRequest, WorkerDiffResponse};
use sha2::{Digest, Sha256};

use crate::{WorkerError, adapters};

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;

pub fn decode_request_bounded(bytes: &[u8]) -> Result<WorkerDiffRequest, WorkerError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(WorkerError::ResourceLimit("request bytes"));
    }
    let request: WorkerDiffRequest =
        serde_json::from_slice(bytes).map_err(|_| WorkerError::InvalidRequest)?;
    request
        .validate()
        .map_err(|_| WorkerError::ResourceLimit("source bytes"))?;
    Ok(request)
}

pub fn run_worker_shell<B: Read, T: Read>(
    request_bytes: &[u8],
    mut base: B,
    mut target: T,
) -> Result<WorkerDiffResponse, WorkerError> {
    guard_worker_execution(|| {
        let request = decode_request_bounded(request_bytes)?;
        let base = read_source(&mut base, request.base_size_bytes, request.base_raw_sha256)?;
        let target = read_source(
            &mut target,
            request.target_size_bytes,
            request.target_raw_sha256,
        )?;
        let response = adapters::compare(&request, &base, &target)?;
        response
            .validate_against(&request)
            .map_err(|_| WorkerError::InvalidResult)?;
        let bytes = serde_json::to_vec(&response).map_err(|_| WorkerError::InvalidResult)?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err(WorkerError::ResourceLimit("result bytes"));
        }
        Ok(response)
    })
}

pub fn guard_worker_execution<T>(
    operation: impl FnOnce() -> Result<T, WorkerError>,
) -> Result<T, WorkerError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .map_err(|_| WorkerError::WorkerPanic)?
}

fn read_source(
    reader: &mut impl Read,
    expected_size: u64,
    expected_hash: [u8; 32],
) -> Result<Vec<u8>, WorkerError> {
    if expected_size > MAX_SOURCE_BYTES as u64 {
        return Err(WorkerError::ResourceLimit("source bytes"));
    }
    let mut bytes = Vec::new();
    reader
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| WorkerError::UnreadableInput)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(WorkerError::ResourceLimit("source bytes"));
    }
    if bytes.len() as u64 != expected_size
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected_hash
    {
        return Err(WorkerError::RawBindingMismatch);
    }
    Ok(bytes)
}
