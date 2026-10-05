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
        expected_attempt_id: None,
        evidence_revision_refs: vec![],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
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
        expected_attempt_id: None,
        evidence_revision_refs: vec![],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
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

fn received_workflow() -> (Workflow, HandoffSnapshot) {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    let snapshot = submit(&mut workflow, &artifact);
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
    (workflow, snapshot)
}
fn return_command(workflow: &Workflow, snapshot: &HandoffSnapshot) -> Command {
    serde_json::from_value(serde_json::json!({
        "kind":"return", "task_id":OFFICE_TASK_ID,
        "context":context(VerifiedActor::Office01, workflow.next.as_ref().unwrap().revision),
        "expected_attempt_id":workflow.next.as_ref().unwrap().attempt_id,
        "previous_submission_id":snapshot.id, "target_task_id":SALES_TASK_ID,
        "transition_id":"01900000-0000-7000-8000-000000000010", "reason":"記載の確認をお願いします"
    }))
    .expect("fixed return command must be supported")
}
#[test]
fn return_creates_ready_sales_attempt_without_reopening_completed_history() {
    let (mut workflow, snapshot) = received_workflow();
    let old_source = serde_json::to_value(&workflow.source).unwrap();
    let old_snapshot = serde_json::to_value(&snapshot).unwrap();
    let command = return_command(&workflow, &snapshot);
    let result = workflow
        .apply(VerifiedActor::Office01, &command, NOW)
        .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["kind"], "returned");
    assert_eq!(result["nextTask"]["id"], SALES_TASK_ID.to_string());
    assert_eq!(result["nextTask"]["attemptNumber"], 2);
    assert_eq!(workflow.source.state, TaskState::Ready);
    assert_eq!(workflow.source.revision, 3);
    assert_ne!(workflow.source.attempt_id, SALES_ATTEMPT_ID);
    assert_eq!(workflow.next.as_ref().unwrap().state, TaskState::Completed);
    let stored = serde_json::to_value(&workflow).unwrap();
    assert_eq!(stored["completedAttempts"][0], old_source);
    assert_eq!(
        serde_json::to_value(&workflow.snapshots[0]).unwrap(),
        old_snapshot
    );
    assert_eq!(
        result["returnInstruction"]["previousSubmissionId"],
        snapshot.id.to_string()
    );
    assert_eq!(
        result["returnInstruction"]["sourceAttemptId"],
        OFFICE_ATTEMPT_ID.to_string()
    );
    assert_eq!(
        result["returnInstruction"]["targetAttemptId"],
        workflow.source.attempt_id.to_string()
    );
    assert_eq!(result["returnInstruction"]["returnedBy"], "office-01");
    assert!(!result.to_string().contains("営業の非公開メモ"));
}
#[test]
fn rejected_return_preserves_every_record_for_bad_causality_and_reason() {
    let (workflow, snapshot) = received_workflow();
    let original = return_command(&workflow, &snapshot);
    for (field, value, expected) in [
        (
            "expected_attempt_id",
            serde_json::json!(Uuid::now_v7()),
            WorkError::RevisionConflict,
        ),
        (
            "previous_submission_id",
            serde_json::json!(Uuid::now_v7()),
            WorkError::HandoffNotReady,
        ),
        (
            "target_task_id",
            serde_json::json!(Uuid::now_v7()),
            WorkError::HandoffNotReady,
        ),
        (
            "transition_id",
            serde_json::json!(Uuid::now_v7()),
            WorkError::HandoffNotReady,
        ),
        (
            "reason",
            serde_json::json!(" \n\t"),
            WorkError::ValidationFailed,
        ),
        (
            "reason",
            serde_json::json!("あ".repeat(2731)),
            WorkError::ValidationFailed,
        ),
    ] {
        let mut command = serde_json::to_value(&original).unwrap();
        command[field] = value;
        let command = serde_json::from_value(command).unwrap();
        let mut candidate = workflow.clone();
        assert_eq!(
            candidate.apply(VerifiedActor::Office01, &command, NOW),
            Err(expected),
            "{field}"
        );
        assert_eq!(candidate, workflow, "{field}");
    }
    let mut stale = serde_json::to_value(&original).unwrap();
    stale["context"]["expectedRevision"] = serde_json::json!(0);
    let mut candidate = workflow.clone();
    assert_eq!(
        candidate.apply(
            VerifiedActor::Office01,
            &serde_json::from_value(stale).unwrap(),
            NOW
        ),
        Err(WorkError::RevisionConflict)
    );
    assert_eq!(candidate, workflow);
}
#[test]
fn legacy_definition_remains_forward_only_and_old_json_roundtrips() {
    let (mut workflow, snapshot) = received_workflow();
    workflow.definition_version_id = DEFINITION_VERSION_ID;
    let mut legacy = serde_json::to_value(&workflow).unwrap();
    legacy.as_object_mut().unwrap().remove("completedAttempts");
    legacy.as_object_mut().unwrap().remove("returnInstructions");
    let decoded: Workflow = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.definition_version_id, DEFINITION_VERSION_ID);
    let command = return_command(&decoded, &snapshot);
    let mut candidate = decoded.clone();
    assert_eq!(
        candidate.apply(VerifiedActor::Office01, &command, NOW),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(candidate, decoded);
}

#[test]
fn rework_drafts_remain_current_attempt_private_and_resubmit_archives_office() {
    let (mut workflow, first_snapshot) = received_workflow();
    let original_snapshot_bytes = serde_json::to_vec(&first_snapshot).unwrap();
    let original_return = return_command(&workflow, &first_snapshot);
    let return_context = original_return.context().clone();
    let returned_result = workflow
        .apply(VerifiedActor::Office01, &original_return, NOW)
        .unwrap();
    let returned = serde_json::to_value(&returned_result).unwrap();
    let instruction_id =
        serde_json::from_value(returned["returnInstruction"]["id"].clone()).unwrap();
    let office_completed = workflow.next.clone().unwrap();
    assert!(workflow.list_tasks(VerifiedActor::Sales01, TaskView::Queue)[0].can_claim);
    assert_eq!(
        workflow.detail(VerifiedActor::Sales01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(
        workflow.artifact(
            VerifiedActor::Sales01,
            first_snapshot.artifacts[0].artifact_id
        ),
        Err(WorkError::WorkArtifactNotFound)
    );
    workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Claim {
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 3),
            },
            NOW,
        )
        .unwrap();
    let detail = workflow
        .detail(VerifiedActor::Sales01, SALES_TASK_ID)
        .unwrap();
    assert_eq!(detail.task.return_instruction_id, Some(instruction_id));
    assert!(!detail.task.can_submit);
    assert!(detail.working_artifacts.is_empty());
    assert_eq!(
        workflow
            .return_instruction(VerifiedActor::Sales01, instruction_id)
            .unwrap()
            .reason,
        "記載の確認をお願いします"
    );
    let save_command = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: context(VerifiedActor::Sales01, 4),
        value: TextValue {
            text: "NEW PRIVATE REWORK".into(),
        },
    };
    let saved = workflow
        .apply(VerifiedActor::Sales01, &save_command, NOW)
        .unwrap();
    let artifact = match &saved {
        MutationResult::DraftSaved { artifact, .. } => artifact.clone(),
        _ => panic!(),
    };
    assert_eq!(
        workflow.artifact(VerifiedActor::Office01, artifact.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        workflow.authorize_recovery(VerifiedActor::Office01, &saved),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        workflow.detail(VerifiedActor::Office01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert!(
        !serde_json::to_string(&workflow.list_tasks(VerifiedActor::Office01, TaskView::Context))
            .unwrap()
            .contains("NEW PRIVATE REWORK")
    );
    assert!(
        !serde_json::to_string(
            &workflow
                .detail(VerifiedActor::Office01, OFFICE_TASK_ID)
                .unwrap()
        )
        .unwrap()
        .contains("NEW PRIVATE REWORK")
    );
    let snapshot = match workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Submit {
                expected_attempt_id: None,
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 5),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: 0,
                }],
            },
            NOW,
        )
        .unwrap()
    {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    };
    assert_eq!(snapshot.submission_number, 2);
    assert_ne!(snapshot.id, first_snapshot.id);
    assert_eq!(snapshot.source_attempt_id, workflow.source.attempt_id);
    assert_eq!(workflow.next.as_ref().unwrap().attempt_number, 2);
    assert_eq!(workflow.next.as_ref().unwrap().revision, 3);
    assert_eq!(workflow.completed_attempts[1], office_completed);
    // Current private-read authorization and immutable result replay are distinct.
    assert_eq!(
        workflow.authorize_command(VerifiedActor::Office01, &original_return),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(return_context.authorize(VerifiedActor::Office01), Ok(()));
    assert_eq!(
        workflow.authorize_recovery(VerifiedActor::Office01, &returned_result),
        Ok(())
    );

    assert_eq!(
        serde_json::to_vec(&workflow.snapshots[0]).unwrap(),
        original_snapshot_bytes
    );
    assert_eq!(
        workflow.snapshot(VerifiedActor::Office01, snapshot.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        workflow
            .snapshot(VerifiedActor::Office01, first_snapshot.id)
            .unwrap(),
        first_snapshot
    );
    workflow
        .apply(
            VerifiedActor::Office01,
            &Command::Claim {
                task_id: OFFICE_TASK_ID,
                context: context(VerifiedActor::Office01, 3),
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
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap()["previousSubmissionId"],
        first_snapshot.id.to_string()
    );
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap()["returnInstructionId"],
        instruction_id.to_string()
    );
    assert_eq!(
        workflow.artifact(VerifiedActor::Office01, artifact.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    workflow.validate_integrity().unwrap();
}
#[test]
fn old_ledger_results_decode_missing_additive_summary_fields() {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    let result = workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Submit {
                expected_attempt_id: None,
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 1),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: artifact.revision,
                }],
            },
            NOW,
        )
        .unwrap();
    let mut legacy = serde_json::to_value(&result).unwrap();
    for field in ["task", "nextTask"] {
        for added in [
            "attemptNumber",
            "canReturn",
            "canComplete",
            "completionActionId",
            "returnInstructionId",
            "returnTransition",
        ] {
            legacy[field].as_object_mut().unwrap().remove(added);
        }
    }
    let snapshot_bytes = serde_json::to_vec(&legacy["snapshot"]).unwrap();
    let decoded: MutationResult = serde_json::from_value(legacy).unwrap();
    match decoded {
        MutationResult::Submitted {
            task,
            snapshot,
            next_task,
        } => {
            assert_eq!(task.attempt_number, 1);
            assert_eq!(next_task.attempt_number, 1);
            assert!(!task.can_return);
            assert!(!task.can_complete);
            assert_eq!(task.completion_action_id, None);
            assert_eq!(task.return_instruction_id, None);
            assert_eq!(
                serde_json::to_vec(&serde_json::to_value(snapshot).unwrap()).unwrap(),
                snapshot_bytes
            );
        }
        _ => panic!(),
    }
}
#[test]
fn corrupt_attempt_number_or_current_pointer_cannot_commit() {
    let (mut workflow, snapshot) = received_workflow();
    workflow
        .apply(
            VerifiedActor::Office01,
            &return_command(&workflow, &snapshot),
            NOW,
        )
        .unwrap();
    workflow.source.attempt_id = SALES_ATTEMPT_ID;
    let before = workflow.clone();
    assert_eq!(
        workflow.apply(
            VerifiedActor::Sales01,
            &Command::Claim {
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 3)
            },
            NOW
        ),
        Err(WorkError::IntegrityViolation)
    );
    assert_eq!(workflow, before);
}

#[test]
fn draft_limit_is_per_attempt_and_prior_artifacts_cannot_be_selected_again() {
    let mut workflow = Workflow::synthetic(None);
    let mut artifacts = vec![];
    for revision in 0..MAX_ARTIFACTS {
        match workflow
            .apply(
                VerifiedActor::Sales01,
                &Command::SaveDraft {
                    task_id: SALES_TASK_ID,
                    artifact_id: None,
                    context: context(VerifiedActor::Sales01, revision as i64),
                    value: TextValue {
                        text: "submitted".into(),
                    },
                },
                NOW,
            )
            .unwrap()
        {
            MutationResult::DraftSaved { artifact, .. } => artifacts.push(artifact),
            _ => panic!(),
        }
    }
    let snapshot = match workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Submit {
                expected_attempt_id: None,
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, MAX_ARTIFACTS as i64),
                artifacts: artifacts
                    .iter()
                    .map(|artifact| ArtifactSelection {
                        artifact_id: artifact.id,
                        revision: 0,
                    })
                    .collect(),
            },
            NOW,
        )
        .unwrap()
    {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    };
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
    workflow
        .apply(
            VerifiedActor::Office01,
            &return_command(&workflow, &snapshot),
            NOW,
        )
        .unwrap();
    workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Claim {
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, MAX_ARTIFACTS as i64 + 2),
            },
            NOW,
        )
        .unwrap();
    let revision = workflow.source.revision;
    let before = workflow.clone();
    assert_eq!(
        workflow.apply(
            VerifiedActor::Sales01,
            &Command::Submit {
                expected_attempt_id: None,
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, revision),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifacts[0].id,
                    revision: 0
                }]
            },
            NOW
        ),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(workflow, before);
    assert!(
        !workflow
            .detail(VerifiedActor::Sales01, SALES_TASK_ID)
            .unwrap()
            .task
            .can_submit
    );
    workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: context(VerifiedActor::Sales01, revision),
                value: TextValue {
                    text: "new attempt draft".into(),
                },
            },
            NOW,
        )
        .unwrap();
    assert!(
        workflow
            .detail(VerifiedActor::Sales01, SALES_TASK_ID)
            .unwrap()
            .task
            .can_submit
    );
}

#[test]
fn operation_context_requires_valid_identity_revision_and_current_responsibility() {
    let valid = context(VerifiedActor::Office01, 1);
    assert_eq!(valid.authorize(VerifiedActor::Office01), Ok(()));
    assert_eq!(
        valid.authorize(VerifiedActor::Sales01),
        Err(WorkError::Forbidden)
    );
    let mut invalid = valid.clone();
    invalid.operation_id = Uuid::nil();
    assert_eq!(
        invalid.authorize(VerifiedActor::Office01),
        Err(WorkError::ValidationFailed)
    );
    invalid = valid;
    invalid.expected_revision = -1;
    assert_eq!(
        invalid.authorize(VerifiedActor::Office01),
        Err(WorkError::ValidationFailed)
    );
}

fn complete_command(workflow: &Workflow) -> Command {
    let item = workflow.next.as_ref().unwrap();
    serde_json::from_value(serde_json::json!({
        "kind":"complete", "task_id":OFFICE_TASK_ID,
        "context":context(VerifiedActor::Office01, item.revision),
        "expected_attempt_id":item.attempt_id,
        "definition_action_id":"01900000-0000-7000-8000-000000000012"
    }))
    .expect("definition-bound completion command must be supported")
}
#[test]
fn completion_closes_only_final_attempt_without_rewriting_submissions_or_creating_work() {
    let (mut workflow, snapshot) = received_workflow();
    let before = workflow.clone();
    let hints = serde_json::to_value(
        workflow
            .detail(VerifiedActor::Office01, OFFICE_TASK_ID)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(hints["canComplete"], true);
    assert_eq!(
        hints["completionActionId"],
        "01900000-0000-7000-8000-000000000012"
    );
    let result = workflow
        .apply(VerifiedActor::Office01, &complete_command(&workflow), NOW)
        .unwrap();
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["kind"], "completed");
    assert_eq!(json.as_object().unwrap().len(), 2);
    assert_eq!(json["task"]["state"], "completed");
    for key in [
        "canClaim",
        "canEdit",
        "canSubmit",
        "canReturn",
        "canComplete",
        "canRegisterEvidence",
        "canRegisterFinding",
        "canRecordDecision",
        "canRequestAgent",
    ] {
        assert_eq!(json["task"][key], false, "{key}");
    }
    assert!(json["task"]["completionActionId"].is_null());
    let office = workflow.next.as_ref().unwrap();
    assert_eq!(office.attempt_id, before.next.as_ref().unwrap().attempt_id);
    assert_eq!(
        office.attempt_number,
        before.next.as_ref().unwrap().attempt_number
    );
    assert_eq!(office.revision, before.next.as_ref().unwrap().revision + 1);
    assert_eq!(office.completed_at.as_deref(), Some(NOW));
    assert_eq!(workflow.source, before.source);
    assert_eq!(workflow.snapshots, before.snapshots);
    assert_eq!(workflow.completed_attempts, before.completed_attempts);
    assert_eq!(workflow.return_instructions, before.return_instructions);
    assert_eq!(workflow.artifacts, before.artifacts);
    assert_eq!(workflow.history.last().unwrap().kind, "completed");
    assert_eq!(
        workflow
            .snapshot(VerifiedActor::Office01, snapshot.id)
            .unwrap(),
        snapshot
    );
    assert_eq!(
        workflow.authorize_recovery(VerifiedActor::Office01, &result),
        Ok(())
    );
    assert_eq!(
        workflow.authorize_recovery(VerifiedActor::Sales01, &result),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(
        workflow.artifact(VerifiedActor::Office01, workflow.artifacts[0].id),
        Err(WorkError::WorkArtifactNotFound)
    );
}
#[test]
fn completion_rejects_wrong_action_attempt_revision_actor_and_terminal_without_mutation() {
    let (workflow, _) = received_workflow();
    let original = complete_command(&workflow);
    for (field, value, error) in [
        (
            "definition_action_id",
            serde_json::json!(RETURN_TRANSITION_ID),
            WorkError::HandoffNotReady,
        ),
        (
            "expected_attempt_id",
            serde_json::json!(Uuid::now_v7()),
            WorkError::RevisionConflict,
        ),
        (
            "context",
            serde_json::json!(context(VerifiedActor::Office01, 0)),
            WorkError::RevisionConflict,
        ),
        (
            "context",
            serde_json::json!(context(VerifiedActor::Sales01, 1)),
            WorkError::Forbidden,
        ),
    ] {
        let mut wire = serde_json::to_value(&original).unwrap();
        wire[field] = value;
        let command = serde_json::from_value(wire).unwrap();
        let mut candidate = workflow.clone();
        assert_eq!(
            candidate.apply(VerifiedActor::Office01, &command, NOW),
            Err(error),
            "{field}"
        );
        assert_eq!(candidate, workflow);
    }
    let mut wrong_actor = serde_json::to_value(&original).unwrap();
    wrong_actor["context"] = serde_json::json!(context(VerifiedActor::Sales01, 1));
    let mut candidate = workflow.clone();
    assert_eq!(
        candidate.apply(
            VerifiedActor::Sales01,
            &serde_json::from_value(wrong_actor).unwrap(),
            NOW
        ),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(candidate, workflow);
    candidate
        .apply(VerifiedActor::Office01, &original, NOW)
        .unwrap();
    let terminal = candidate.clone();
    assert_eq!(
        candidate.apply(VerifiedActor::Office01, &complete_command(&candidate), NOW),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(candidate, terminal);
}
#[test]
fn completion_never_bypasses_forward_submission_or_changes_older_definition_versions() {
    let (workflow, _) = received_workflow();
    for version in [DEFINITION_VERSION_ID, RETURN_DEFINITION_VERSION_ID] {
        let mut legacy = workflow.clone();
        legacy.definition_version_id = version;
        let before = legacy.clone();
        let hints = serde_json::to_value(
            legacy
                .detail(VerifiedActor::Office01, OFFICE_TASK_ID)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hints["canComplete"], false);
        assert!(hints["completionActionId"].is_null());
        assert_eq!(
            legacy.apply(VerifiedActor::Office01, &complete_command(&legacy), NOW),
            Err(WorkError::HandoffNotReady)
        );
        assert_eq!(legacy, before);
    }
    let mut sales = Workflow::synthetic(None);
    let before = sales.clone();
    let mut command = serde_json::to_value(complete_command(&workflow)).unwrap();
    command["task_id"] = serde_json::json!(SALES_TASK_ID);
    command["expected_attempt_id"] = serde_json::json!(SALES_ATTEMPT_ID);
    command["context"] = serde_json::json!(context(VerifiedActor::Sales01, 0));
    assert_eq!(
        sales.apply(
            VerifiedActor::Sales01,
            &serde_json::from_value(command).unwrap(),
            NOW
        ),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(sales, before);
}
#[test]
fn completion_hints_do_not_authorize_shared_progress_or_unclaimed_queue() {
    let mut workflow = Workflow::synthetic(None);
    let artifact = save(&mut workflow);
    submit(&mut workflow, &artifact);
    for (actor, view) in [
        (VerifiedActor::Sales01, TaskView::Context),
        (VerifiedActor::Office01, TaskView::Queue),
    ] {
        let summary = workflow
            .list_tasks(actor, view)
            .into_iter()
            .find(|t| t.id == OFFICE_TASK_ID)
            .unwrap();
        let summary = serde_json::to_value(summary).unwrap();
        assert_eq!(summary["canComplete"], false);
        assert!(summary["completionActionId"].is_null());
    }
    let before = workflow.clone();
    assert_eq!(
        workflow.apply(VerifiedActor::Office01, &complete_command(&workflow), NOW),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(workflow, before);
}
#[test]
fn completion_invalidates_running_agent_and_rejects_late_candidate_output() {
    let (mut workflow, _) = received_workflow();
    workflow.input_resources.push(InputResourceRef {
        kind: "document".into(),
        document_id: Uuid::from_u128(71),
        label: "合成入力".into(),
    });
    let register = Command::RegisterEvidence {
        task_id: OFFICE_TASK_ID,
        context: context(VerifiedActor::Office01, 1),
        expected_attempt_id: OFFICE_ATTEMPT_ID,
        source: EvidenceSource {
            source_ref: SourceRef {
                provider_id: "document".into(),
                resource_id: Uuid::from_u128(71),
                revision_id: Uuid::from_u128(72),
                version_id: Uuid::from_u128(73),
            },
            authoritative_locator: AuthoritativeLocator {
                kind: "contentItem".into(),
                content_item_id: Uuid::from_u128(74),
                representation_id: Uuid::from_u128(75),
            },
        },
        relevant_location: "完了前の根拠".into(),
    };
    let evidence = match workflow
        .apply(VerifiedActor::Office01, &register, NOW)
        .unwrap()
    {
        MutationResult::EvidenceRegistered { evidence, .. } => evidence,
        _ => panic!(),
    };
    let request = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: context(VerifiedActor::Office01, 2),
        expected_attempt_id: OFFICE_ATTEMPT_ID,
        purpose: "合成確認".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: evidence.id,
            revision: 1,
        }],
    };
    let execution = match workflow
        .apply(VerifiedActor::Office01, &request, NOW)
        .unwrap()
    {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => panic!(),
    };
    let running = workflow
        .start_agent_execution(VerifiedActor::Office01, execution.id, NOW)
        .unwrap()
        .unwrap();
    workflow
        .apply(VerifiedActor::Office01, &complete_command(&workflow), NOW)
        .unwrap();
    let done = workflow.clone();
    assert_eq!(
        workflow
            .agent_execution(VerifiedActor::Office01, execution.id)
            .unwrap()
            .status,
        AgentExecutionStatus::Failed
    );
    assert_eq!(
        workflow.finish_agent_execution(
            &running,
            AgentFindingOutput {
                summary: "合成実行".into(),
                claim: "遅い出力".into(),
                uncertainty: vec!["本文分析なし".into()]
            },
            NOW
        ),
        Err(WorkError::WorkContextStale)
    );
    assert_eq!(workflow, done);
    assert!(workflow.findings.is_empty());
}
