use document_publication_scheduler::{DueScheduler, SchedulerError};

#[cfg(target_os = "linux")]
use document_publication_scheduler::probe_mandatory_sandbox;
#[cfg(target_os = "linux")]
use document_semantic_inspection_runner::{RunnerConfig, RunnerInspectionExecutor};

#[test]
fn due_scheduler_has_a_runnable_entrypoint() {
    fn require_scheduler(_: DueScheduler) {}
    let _ = require_scheduler;
}

#[tokio::test]
async fn startup_requires_an_identity_resolver() {
    let result = DueScheduler::connect(
        "unused",
        std::path::Path::new("unused"),
        std::path::Path::new("unused"),
        None,
    )
    .await;
    assert!(matches!(
        result,
        Err(SchedulerError::IdentityResolverRequired)
    ));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn startup_fails_closed_when_worker_cannot_install_mandatory_sandbox() {
    let executor = RunnerInspectionExecutor::new(RunnerConfig::new("/bin/false")).unwrap();
    assert!(matches!(
        probe_mandatory_sandbox(&executor).await,
        Err(SchedulerError::SandboxUnavailable)
    ));
}
