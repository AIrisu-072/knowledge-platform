use document_application::{
    CancelScheduleCommand, PublishOperationId, SchedulePublishCommand, VersionOperationId,
};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

#[test]
fn schedule_command_is_a_durable_publish_intent() {
    fn require_command(_: SchedulePublishCommand) {}
    let _ = require_command;
}

#[test]
fn schedule_requires_utc_and_cancel_has_a_distinct_operation_id() {
    let publish_id = PublishOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8001-000000000001").unwrap(),
    )
    .unwrap();
    let cancel_id = VersionOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8002-000000000001").unwrap(),
    )
    .unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(1));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(2));
    let actor = PrincipalRef::new("test", "actor").unwrap();
    let due = OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap();
    assert!(
        SchedulePublishCommand::new(publish_id, document_id, version_id, -1, actor.clone(), due)
            .is_err()
    );
    assert!(
        SchedulePublishCommand::new(
            publish_id,
            document_id,
            version_id,
            1,
            actor.clone(),
            due.to_offset(UtcOffset::from_hms(9, 0, 0).unwrap())
        )
        .is_err()
    );
    let scheduled =
        SchedulePublishCommand::new(publish_id, document_id, version_id, 1, actor.clone(), due)
            .unwrap();
    assert_eq!(scheduled.scheduled_publish_at(), due);
    let cancelled =
        CancelScheduleCommand::new(cancel_id, publish_id, document_id, version_id, 2, actor)
            .unwrap();
    assert_ne!(
        cancelled.operation_id().as_uuid(),
        cancelled.publish_operation_id().as_uuid()
    );
}
