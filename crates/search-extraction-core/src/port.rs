//! Source-ID-free extraction port between the trusted host and a worker runner.

use crate::protocol::{WorkerFragment, WorkerReport, WorkerRequest};
use crate::validation::{ExtractionError, RegisteredProfile};

/// Runs one request against already bound raw bytes with a host-registered
/// profile. Implementations never see Source, actor, item or storage identity.
pub trait ContentExtractor: Send + Sync {
    fn extract(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError>;

    fn resolve_locators(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<Vec<WorkerFragment>, ExtractionError>;
}
