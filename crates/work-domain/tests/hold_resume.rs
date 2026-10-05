use serde_json::json;
use uuid::Uuid;
use work_domain::*;
const NOW: &str = "2026-10-04T21:00:00Z";
const HOLD: Uuid = Uuid::from_u128(0x01900000000070008000000000000014);
const RESUME: Uuid = Uuid::from_u128(0x01900000000070008000000000000015);
fn context(actor: VerifiedActor, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: actor.assignment_id(),
    }
}
fn action(w: &Workflow, actor: VerifiedActor, task_id: Uuid, kind: &str) -> Command {
    let task = w.detail(actor, task_id).unwrap().task;
    serde_json::from_value(json!({"kind":kind,"task_id":task_id,"context":context(actor,task.revision),"expected_attempt_id":task.attempt_id,"definition_action_id":if kind=="hold" {HOLD} else {RESUME}})).expect("hold/resume are defined workflow commands")
}
fn hold(w: &mut Workflow, actor: VerifiedActor, task_id: Uuid) -> MutationResult {
    w.apply(actor, &action(w, actor, task_id, "hold"), NOW)
        .unwrap()
}
fn resume(w: &mut Workflow, actor: VerifiedActor, task_id: Uuid) -> MutationResult {
    w.apply(actor, &action(w, actor, task_id, "resume"), NOW)
        .unwrap()
}
fn saved() -> Workflow {
    let mut w = Workflow::synthetic(None);
    w.apply(
        VerifiedActor::Sales01,
        &Command::SaveDraft {
            task_id: SALES_TASK_ID,
            artifact_id: None,
            context: context(VerifiedActor::Sales01, 0),
            value: TextValue {
                text: "保存済み非公開本文".into(),
            },
        },
        NOW,
    )
    .unwrap();
    w
}
fn received() -> Workflow {
    let mut w = saved();
    w.apply(
        VerifiedActor::Sales01,
        &Command::Submit {
            task_id: SALES_TASK_ID,
            context: context(VerifiedActor::Sales01, 1),
            expected_attempt_id: Some(SALES_ATTEMPT_ID),
            artifacts: vec![ArtifactSelection {
                artifact_id: w.artifacts[0].id,
                revision: 0,
            }],
            evidence_revision_refs: vec![],
            finding_revision_refs: vec![],
            decision_revision_refs: vec![],
        },
        NOW,
    )
    .unwrap();
    w.apply(
        VerifiedActor::Office01,
        &Command::Claim {
            task_id: OFFICE_TASK_ID,
            context: context(VerifiedActor::Office01, 0),
        },
        NOW,
    )
    .unwrap();
    w
}
fn assert_preserved(before: &Workflow, after: &Workflow, task_id: Uuid) {
    let mut expected = before.clone();
    expected.revision = after.revision;
    expected.history = after.history.clone();
    let (old, new) = if task_id == SALES_TASK_ID {
        (&mut expected.source, &after.source)
    } else {
        (
            expected.next.as_mut().unwrap(),
            after.next.as_ref().unwrap(),
        )
    };
    old.state = new.state;
    old.revision = new.revision;
    assert_eq!(
        &expected, after,
        "only state/revisions/history may change without pending Agent work"
    );
}
#[test]
fn hold_resume_preserve_same_attempt_assignment_private_content_and_snapshots_for_both_roles() {
    for (mut w, actor, task_id) in [
        (saved(), VerifiedActor::Sales01, SALES_TASK_ID),
        (received(), VerifiedActor::Office01, OFFICE_TASK_ID),
    ] {
        let before = w.clone();
        let original = w.detail(actor, task_id).unwrap();
        let hints = serde_json::to_value(&original).unwrap();
        assert_eq!(hints["canHold"], true);
        assert_eq!(hints["holdActionId"], json!(HOLD));
        assert_eq!(hints["canResume"], false);
        let held = hold(&mut w, actor, task_id);
        let value = serde_json::to_value(&held).unwrap();
        assert_eq!(value["kind"], "held");
        assert_eq!(value["task"]["state"], "held");
        assert_eq!(value["task"]["canResume"], true);
        assert_eq!(value["task"]["resumeActionId"], json!(RESUME));
        for key in [
            "canHold",
            "canEdit",
            "canSubmit",
            "canClaim",
            "canComplete",
            "canReturn",
            "canRegisterEvidence",
            "canRegisterFinding",
            "canRecordDecision",
            "canRequestAgent",
        ] {
            assert_eq!(value["task"][key], false, "{key}");
        }
        assert_eq!(
            w.detail(actor, task_id).unwrap().working_artifacts,
            original.working_artifacts
        );
        assert_preserved(&before, &w, task_id);
        assert_eq!(w.authorize_recovery(actor, &held), Ok(()));
        let resumed = resume(&mut w, actor, task_id);
        let value = serde_json::to_value(&resumed).unwrap();
        assert_eq!(value["kind"], "resumed");
        assert_eq!(value["task"]["state"], "active");
        assert_eq!(value["task"]["canHold"], true);
        assert_eq!(value["task"]["canResume"], false);
        assert_eq!(value["task"]["revision"], original.task.revision + 2);
        assert_preserved(&before, &w, task_id);
        assert_eq!(
            w.history
                .iter()
                .rev()
                .take(2)
                .map(|h| h.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["resumed", "held"]
        );
        assert_eq!(w.authorize_recovery(actor, &held), Ok(()));
        assert_eq!(w.authorize_recovery(actor, &resumed), Ok(()));
        let other = if actor == VerifiedActor::Sales01 {
            VerifiedActor::Office01
        } else {
            VerifiedActor::Sales01
        };
        assert_eq!(
            w.authorize_recovery(other, &held),
            Err(WorkError::WorkItemNotFound)
        );
    }
}
#[test]
fn hold_resume_reject_wrong_action_attempt_revision_responsibility_actor_and_state_atomically() {
    let w = saved();
    let original =
        serde_json::to_value(action(&w, VerifiedActor::Sales01, SALES_TASK_ID, "hold")).unwrap();
    for (field, value, error) in [
        (
            "definition_action_id",
            json!(RESUME),
            WorkError::HandoffNotReady,
        ),
        (
            "expected_attempt_id",
            json!(Uuid::now_v7()),
            WorkError::RevisionConflict,
        ),
        (
            "context",
            json!(context(VerifiedActor::Sales01, 0)),
            WorkError::RevisionConflict,
        ),
        (
            "context",
            json!(context(VerifiedActor::Office01, 1)),
            WorkError::Forbidden,
        ),
    ] {
        let mut wire = original.clone();
        wire[field] = value;
        let mut candidate = w.clone();
        assert_eq!(
            candidate.apply(
                VerifiedActor::Sales01,
                &serde_json::from_value(wire).unwrap(),
                NOW
            ),
            Err(error),
            "{field}"
        );
        assert_eq!(candidate, w);
    }
    let mut wire = original;
    wire["context"] = json!(context(VerifiedActor::Office01, 1));
    let mut candidate = w.clone();
    assert_eq!(
        candidate.apply(
            VerifiedActor::Office01,
            &serde_json::from_value(wire).unwrap(),
            NOW
        ),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(candidate, w);
    assert_eq!(
        candidate.apply(
            VerifiedActor::Sales01,
            &action(&w, VerifiedActor::Sales01, SALES_TASK_ID, "resume"),
            NOW
        ),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(candidate, w);
    hold(&mut candidate, VerifiedActor::Sales01, SALES_TASK_ID);
    let held = candidate.clone();
    assert_eq!(
        candidate.apply(
            VerifiedActor::Sales01,
            &action(&candidate, VerifiedActor::Sales01, SALES_TASK_ID, "hold"),
            NOW
        ),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(candidate, held);
    let resume_command = action(&candidate, VerifiedActor::Sales01, SALES_TASK_ID, "resume");
    for (field, value, error) in [
        (
            "definition_action_id",
            json!(HOLD),
            WorkError::HandoffNotReady,
        ),
        (
            "expected_attempt_id",
            json!(Uuid::now_v7()),
            WorkError::RevisionConflict,
        ),
        (
            "context",
            json!(context(VerifiedActor::Sales01, 1)),
            WorkError::RevisionConflict,
        ),
    ] {
        let mut wire = serde_json::to_value(&resume_command).unwrap();
        wire[field] = value;
        assert_eq!(
            candidate.apply(
                VerifiedActor::Sales01,
                &serde_json::from_value(wire).unwrap(),
                NOW
            ),
            Err(error)
        );
        assert_eq!(candidate, held);
    }
}
#[test]
fn held_task_rejects_all_content_and_workflow_mutations_without_changing_private_state() {
    for (mut w, actor, task_id) in [
        (saved(), VerifiedActor::Sales01, SALES_TASK_ID),
        (received(), VerifiedActor::Office01, OFFICE_TASK_ID),
    ] {
        hold(&mut w, actor, task_id);
        let before = w.clone();
        let task = w.detail(actor, task_id).unwrap().task;
        let ctx = context(actor, task.revision);
        let commands = vec![
            Command::SaveDraft {
                task_id,
                artifact_id: None,
                context: ctx.clone(),
                value: TextValue {
                    text: "保存不可".into(),
                },
            },
            Command::Submit {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: Some(task.attempt_id),
                artifacts: vec![],
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
            },
            Command::Complete {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                definition_action_id: COMPLETE_ACTION_ID,
            },
            Command::Return {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                previous_submission_id: Uuid::now_v7(),
                target_task_id: SALES_TASK_ID,
                transition_id: RETURN_TRANSITION_ID,
                reason: "戻せない".into(),
            },
            Command::RequestAgentExecution {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                purpose: "保留中".into(),
                evidence_revision_refs: vec![],
            },
            Command::CancelAgentExecution {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                execution_id: Uuid::now_v7(),
            },
            Command::RegisterEvidence {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                source: EvidenceSource {
                    source_ref: SourceRef {
                        provider_id: "document".into(),
                        resource_id: Uuid::now_v7(),
                        revision_id: Uuid::now_v7(),
                        version_id: Uuid::now_v7(),
                    },
                    authoritative_locator: AuthoritativeLocator {
                        kind: "contentItem".into(),
                        content_item_id: Uuid::now_v7(),
                        representation_id: Uuid::now_v7(),
                    },
                },
                relevant_location: "保留中".into(),
            },
            Command::RegisterFinding {
                task_id,
                context: ctx.clone(),
                expected_attempt_id: task.attempt_id,
                claim: "保留中".into(),
                evidence_revision_refs: vec![],
                supersedes_finding_id: None,
            },
            Command::RecordDecision {
                task_id,
                context: ctx,
                expected_attempt_id: task.attempt_id,
                finding_id: Uuid::now_v7(),
                finding_revision: 1,
                decision: DecisionKind::Rejected,
                adopted_claim: None,
                reason: Some("保留中".into()),
                evidence_revision_refs: vec![],
                supersedes_decision_id: None,
            },
        ];
        for command in commands {
            assert_eq!(
                w.apply(actor, &command, NOW),
                Err(WorkError::HandoffNotReady),
                "{command:?}"
            );
            assert_eq!(w, before);
        }
    }
}
#[test]
fn hold_resume_do_not_upgrade_old_definitions_or_authorize_ready_terminal_shared_views() {
    let original = saved();
    for version in [
        DEFINITION_VERSION_ID,
        RETURN_DEFINITION_VERSION_ID,
        COMPLETE_DEFINITION_VERSION_ID,
    ] {
        let mut w = original.clone();
        w.definition_version_id = version;
        let before = w.clone();
        let summary =
            serde_json::to_value(w.detail(VerifiedActor::Sales01, SALES_TASK_ID).unwrap()).unwrap();
        for key in ["canHold", "canResume"] {
            assert_eq!(summary[key], false);
        }
        assert!(summary["holdActionId"].is_null());
        assert!(summary["resumeActionId"].is_null());
        assert_eq!(
            w.apply(
                VerifiedActor::Sales01,
                &action(&w, VerifiedActor::Sales01, SALES_TASK_ID, "hold"),
                NOW
            ),
            Err(WorkError::HandoffNotReady)
        );
        assert_eq!(w, before);
    }
    let mut w = received();
    for state in [TaskState::Ready, TaskState::Completed] {
        w.next.as_mut().unwrap().state = state;
        let before = w.clone();
        let command = action(&w, VerifiedActor::Office01, OFFICE_TASK_ID, "hold");
        assert_eq!(
            w.apply(VerifiedActor::Office01, &command, NOW),
            Err(WorkError::HandoffNotReady)
        );
        assert_eq!(w, before);
    }
    let shared =
        serde_json::to_value(received().list_tasks(VerifiedActor::Sales01, TaskView::Context))
            .unwrap();
    let office = shared
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == json!(OFFICE_TASK_ID))
        .unwrap();
    assert_eq!(office["canHold"], false);
    assert_eq!(office["canResume"], false);
}

#[test]
fn hold_resume_require_the_defined_step_and_current_assignment() {
    for case in 0..3 {
        let mut w = saved();
        // The exact fixture step and current assignment bind the definition action.
        match case {
            0 => w.source.step_id = OFFICE_STEP_ID,
            1 => w.source.acting_assignment_id = Some(OFFICE_ASSIGNMENT_ID),
            _ => w.source.work_assignment_id = None,
        }
        let before = w.clone();
        assert_eq!(
            w.apply(
                VerifiedActor::Sales01,
                &action(&w, VerifiedActor::Sales01, SALES_TASK_ID, "hold"),
                NOW
            ),
            Err(WorkError::HandoffNotReady),
            "{case}"
        );
        assert_eq!(w, before);
    }
}
