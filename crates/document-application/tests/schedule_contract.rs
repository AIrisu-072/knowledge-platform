use document_application::SchedulePublishCommand;

#[test]
fn schedule_command_is_a_durable_publish_intent() {
    fn require_command(_: SchedulePublishCommand) {}
    let _ = require_command;
}
