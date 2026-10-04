//! Mandatory worker-side Linux Landlock and seccomp seal.

use crate::SandboxRunError;
#[cfg(not(target_os = "linux"))]
use crate::SandboxRunErrorKind;

pub fn seal_worker_sandbox() -> Result<(), SandboxRunError> {
    #[cfg(target_os = "linux")]
    {
        crate::linux::seal()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(unavailable())
    }
}

#[cfg(not(target_os = "linux"))]
fn unavailable() -> SandboxRunError {
    SandboxRunError::new(
        SandboxRunErrorKind::Unavailable,
        "mandatory sandbox control unavailable",
    )
}
