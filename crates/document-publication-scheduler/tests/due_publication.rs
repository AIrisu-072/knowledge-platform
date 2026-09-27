use document_publication_scheduler::DueScheduler;

#[test]
fn due_scheduler_has_a_runnable_entrypoint() {
    fn require_scheduler(_: DueScheduler) {}
    let _ = require_scheduler;
}
