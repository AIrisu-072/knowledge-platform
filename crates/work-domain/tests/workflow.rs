use uuid::Uuid;
use work_domain::*;
const NOW: &str = "2026-10-04T05:00:00Z";
fn context(actor: VerifiedActor, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: actor.assignment_id(),
    }
}
fn save(workflow: &mut Workflow) -> WorkingArtifact {
    let command = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: context(VerifiedActor::Sales01, 0),
        value: TextValue {
            text: "営業の非公開メモ".into(),
        },
    };
    match workflow
        .apply(VerifiedActor::Sales01, &command, NOW)
        .unwrap()
    {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    }
}
fn submit(workflow: &mut Workflow, artifact: &WorkingArtifact) -> HandoffSnapshot {
    let command = Command::Submit {
        task_id: SALES_TASK_ID,
        context: context(VerifiedActor::Sales01, 1),
        artifacts: vec![ArtifactSelection {
            artifact_id: artifact.id,
            revision: artifact.revision,
        }],
    };
    match workflow
        .apply(VerifiedActor::Sales01, &command, NOW)
        .unwrap()
    {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    }
}
#[test]
fn startup_profiles_are_closed_and_do_not_accept_arbitrary_identity() {
    assert_eq!(
        VerifiedActor::from_startup_profile("sales-01"),
        Ok(VerifiedActor::Sales01)
    );
    assert_eq!(
        VerifiedActor::from_startup_profile("office-01"),
        Ok(VerifiedActor::Office01)
    );
    for profile in ["poc", "production", "agent-01", "sales-01,office-01"] {
        assert_eq!(
            VerifiedActor::from_startup_profile(profile),
            Err(WorkError::Forbidden)
        );
    }
}
#[test]
fn downstream_cannot_list_or_read_private_drafts_by_known_id() {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    assert!(
        workflow
            .list_tasks(VerifiedActor::Office01, TaskView::Queue)
            .is_empty()
    );
    assert_eq!(
        workflow.detail(VerifiedActor::Office01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(
        workflow.artifact(VerifiedActor::Office01, artifact.id),
        Err(WorkError::WorkArtifactNotFound)
    );
}
#[test]
fn submission_freezes_bytes_closes_source_and_exposes_one_ready_recipient() {
    let mut workflow = Workflow::synthetic(Some(Uuid::now_v7()));
    let artifact = save(&mut workflow);
    let snapshot = submit(&mut workflow, &artifact);
    assert_eq!(workflow.source.state, TaskState::Completed);
    assert_eq!(workflow.next.as_ref().unwrap().state, TaskState::Ready);
    assert_eq!(snapshot.artifacts[0].value, artifact.value);
    assert_eq!(
        workflow.detail(VerifiedActor::Office01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(
        workflow.snapshot(VerifiedActor::Office01, snapshot.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    workflow
        .apply(
            VerifiedActor::Office01,
            &Command::Claim {
                task_id: OFFICE_TASK_ID,
                context: context(VerifiedActor::Office01, 0),
            },
            NOW,
        )
        .unwrap();
    assert_eq!(
        workflow
            .snapshot(VerifiedActor::Office01, snapshot.id)
            .unwrap(),
        snapshot
    );
    assert!(
        workflow
            .detail(VerifiedActor::Office01, OFFICE_TASK_ID)
            .unwrap()
            .working_artifacts
            .is_empty()
    );
    assert_eq!(
        workflow.artifact(VerifiedActor::Office01, artifact.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(workflow.history.len(), 2);
}
#[test]
fn stale_update_and_invalid_submission_leave_the_entire_workflow_unchanged() {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    let unchanged = workflow.clone();
    let stale = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: Some(artifact.id),
        context: context(VerifiedActor::Sales01, 0),
        value: TextValue {
            text: "stale".into(),
        },
    };
    assert_eq!(
        workflow.apply(VerifiedActor::Sales01, &stale, NOW),
        Err(WorkError::RevisionConflict)
    );
    let bad = Command::Submit {
        task_id: SALES_TASK_ID,
        context: context(VerifiedActor::Sales01, 1),
        artifacts: vec![ArtifactSelection {
            artifact_id: artifact.id,
            revision: 99,
        }],
    };
    assert_eq!(
        workflow.apply(VerifiedActor::Sales01, &bad, NOW),
        Err(WorkError::RevisionConflict)
    );
    assert_eq!(workflow, unchanged);
}
#[test]
fn completed_attempt_cannot_mutate_its_draft_or_snapshot() {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    let snapshot = submit(&mut workflow, &artifact);
    let unchanged = workflow.clone();
    let update = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: Some(artifact.id),
        context: context(VerifiedActor::Sales01, 2),
        value: TextValue {
            text: "changed".into(),
        },
    };
    assert_eq!(
        workflow.apply(VerifiedActor::Sales01, &update, NOW),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(workflow, unchanged);
    assert_eq!(
        workflow
            .snapshot(VerifiedActor::Sales01, snapshot.id)
            .unwrap(),
        snapshot
    );
}
#[test]
fn responsibility_and_utf8_bounds_are_checked_without_mutation() {
    let mut workflow = Workflow::synthetic(None);
    let unchanged = workflow.clone();
    let mut command = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: context(VerifiedActor::Sales01, 0),
        value: TextValue {
            text: "あ".repeat(MAX_TEXT_BYTES),
        },
    };
    assert_eq!(
        workflow.apply(VerifiedActor::Sales01, &command, NOW),
        Err(WorkError::ValidationFailed)
    );
    if let Command::SaveDraft { context, value, .. } = &mut command {
        context.acting_assignment_id = OFFICE_ASSIGNMENT_ID;
        value.text = "valid".into();
    }
    assert_eq!(
        workflow.apply(VerifiedActor::Sales01, &command, NOW),
        Err(WorkError::Forbidden)
    );
    assert_eq!(workflow, unchanged);
}
#[test]
fn context_and_queue_are_projections_of_the_same_work_item() {
    let workflow = Workflow::synthetic(None);
    let context = workflow.list_tasks(VerifiedActor::Sales01, TaskView::Context);
    let queue = workflow.list_tasks(VerifiedActor::Sales01, TaskView::Queue);
    assert_eq!(context[0], queue[0]);
}
#[test]
fn persisted_records_keep_attempt_assignment_and_submission_attribution_distinct() {
    let mut workflow = Workflow::synthetic(None);
    let source = serde_json::to_value(&workflow).unwrap();
    assert_eq!(source["source"]["attemptNumber"], 1);
    assert_eq!(
        source["source"]["actingAssignmentId"],
        SALES_ASSIGNMENT_ID.to_string()
    );
    assert_ne!(
        source["source"]["workAssignmentId"],
        source["source"]["actingAssignmentId"]
    );
    let artifact = save(&mut workflow);
    let snapshot = submit(&mut workflow, &artifact);
    let snapshot = serde_json::to_value(snapshot).unwrap();
    assert_eq!(snapshot["submittedBy"], "sales-01");
    assert_eq!(
        snapshot["actingAssignmentId"],
        SALES_ASSIGNMENT_ID.to_string()
    );
    assert_eq!(snapshot["submissionNumber"], 1);
    assert_eq!(snapshot["contextId"], CONTEXT_ID.to_string());
    let completed = serde_json::to_value(&workflow).unwrap();
    assert_eq!(completed["source"]["completedAt"], NOW);
}
