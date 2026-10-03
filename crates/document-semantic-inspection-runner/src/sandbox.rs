//! DSI worker-side adapter to the shared mandatory Linux seal.

use crate::RunnerError;

pub(crate) fn seal() -> Result<(), RunnerError> {
    document_sandbox_runner::seal_worker_sandbox().map_err(|_| RunnerError::ExtractorUnavailable {
        reason: "mandatory sandbox control unavailable",
    })
}
