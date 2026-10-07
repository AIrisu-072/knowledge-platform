//! Multiple synthetic WorkContexts, derived Attention and WorkViewProfile over the
//! same WorkContext × WorkItem records (U2). Authorization stays U1's.
use std::collections::BTreeSet;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_domain::*;

const T0: &str = "2026-10-07T09:00:00Z";
fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}
fn later(hours: i64) -> String {
    (at(T0) + Duration::hours(hours)).format(&Rfc3339).unwrap()
}
fn ctx(acting: Uuid, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: acting,
    }
}
fn fixture(workflow_id: Uuid) -> &'static ContextFixture {
    context_fixture(workflow_id).unwrap()
}
fn seeded(workflow_id: Uuid, policy: &OrganizationPolicy, now: &str) -> Workflow {
    Workflow::from_fixture(fixture(workflow_id), None, at(T0))
        .unwrap()
        .with_authority(policy.clone(), at(now))
}
fn reattach(w: &Workflow, policy: &OrganizationPolicy, now: &str) -> Workflow {
    w.clone().with_authority(policy.clone(), at(now))
}
fn row(w: &Workflow, actor: VerifiedActor, view: TaskView, id: Uuid) -> Option<TaskSummary> {
    w.list_tasks(actor, view)
        .into_iter()
        .find(|item| item.id == id)
}
fn kinds(attention: &[Attention]) -> Vec<AttentionKind> {
    attention.iter().map(|value| value.kind).collect()
}
/// Sales claims, saves and submits the fixture's first step.
fn submit_sales(w: &mut Workflow, now: &str) {
    let id = w.source.id;
    if w.source.state == TaskState::Ready {
        w.apply(
            VerifiedActor::Sales01,
            &Command::Claim {
                task_id: id,
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
            },
            now,
        )
        .unwrap();
    }
    let MutationResult::DraftSaved { artifact, .. } = w
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: id,
                artifact_id: None,
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
                value: TextValue {
                    text: "合成の非公開メモ".into(),
                },
            },
            now,
        )
        .unwrap()
    else {
        panic!()
    };
    w.apply(
        VerifiedActor::Sales01,
        &Command::Submit {
            task_id: id,
            context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
            expected_attempt_id: Some(w.source.attempt_id),
            artifacts: vec![ArtifactSelection {
                artifact_id: artifact.id,
                revision: artifact.revision,
            }],
            evidence_revision_refs: vec![],
            finding_revision_refs: vec![],
            decision_revision_refs: vec![],
        },
        now,
    )
    .unwrap();
}

#[test]
fn fixtures_create_independent_contexts_with_their_own_steps_and_due_instants() {
    // The existing context keeps its exact stored JSON.
    let existing = Workflow::from_fixture(fixture(WORKFLOW_ID), Some(Uuid::nil()), at(T0)).unwrap();
    assert_eq!(
        serde_json::to_value(&existing).unwrap(),
        serde_json::to_value(Workflow::synthetic(Some(Uuid::nil()))).unwrap()
    );
    let b = Workflow::from_fixture(fixture(CONTEXT_B_WORKFLOW_ID), None, at(T0)).unwrap();
    let c = Workflow::from_fixture(fixture(CONTEXT_C_WORKFLOW_ID), None, at(T0)).unwrap();
    for (w, due) in [(&b, later(6)), (&c, later(-1))] {
        assert_eq!(w.validate_integrity(), Ok(()));
        assert_eq!(w.source.state, TaskState::Ready);
        assert_eq!(w.source.assignee, None);
        assert_eq!(w.source.due_at.as_deref(), Some(due.as_str()));
        assert_eq!(w.source.workflow_instance_id, w.id);
    }
    assert_eq!(b.definition_version_id, REVIEW_DEFINITION_VERSION_ID);
    let ids: BTreeSet<_> = [&existing, &b, &c]
        .iter()
        .flat_map(|w| [w.id, w.context_id, w.source.id, w.source.attempt_id])
        .collect();
    assert_eq!(ids.len(), 12, "every instance owns distinct identities");
    // A stored instance whose shape diverges from its fixture is refused.
    let mut forged = b.clone();
    forged.definition_version_id = HOLD_RESUME_DEFINITION_VERSION_ID;
    assert_eq!(
        forged.validate_integrity(),
        Err(WorkError::IntegrityViolation)
    );
    let mut unknown = b.clone();
    unknown.id = Uuid::now_v7();
    assert_eq!(
        unknown.validate_integrity(),
        Err(WorkError::IntegrityViolation)
    );
}

#[test]
fn the_review_step_needs_the_reviewing_role_and_keeps_existing_operations() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut w, T0);
    let review = w.next.clone().unwrap();
    assert_eq!(
        (review.id, review.step_id, review.work_type_id, review.state),
        (
            CONTEXT_B_REVIEW_TASK_ID,
            REVIEW_STEP_ID,
            REVIEW_WORK_TYPE_ID,
            TaskState::Ready
        )
    );
    // Processing staff never see the review queue; reviewers do.
    assert!(row(&w, VerifiedActor::Office01, TaskView::Queue, review.id).is_none());
    let queued = row(&w, VerifiedActor::Review01, TaskView::Queue, review.id).unwrap();
    assert!(queued.can_claim);
    assert_eq!(queued.title, "審査内容確認");
    assert_eq!(queued.work_type_label, "審査内容確認");
    assert_eq!(queued.required_role_id, Some(ROLE_REVIEWING_ID));
    assert_eq!(
        row(&w, VerifiedActor::MultiRole01, TaskView::Queue, review.id)
            .unwrap()
            .claim_assignment_id,
        Some(MULTI_ROLE_REVIEW_ASSIGNMENT_ID)
    );
    w.apply(
        VerifiedActor::Review01,
        &Command::Claim {
            task_id: review.id,
            context: ctx(REVIEW_ASSIGNMENT_ID, review.revision),
        },
        T0,
    )
    .unwrap();
    let detail = w.detail(VerifiedActor::Review01, review.id).unwrap();
    assert!(detail.task.can_return && detail.task.can_complete && detail.task.can_hold);
    assert_eq!(
        detail
            .task
            .return_transition
            .map(|value| value.target_task_id),
        Some(CONTEXT_B_SALES_TASK_ID)
    );
    // The other context is untouched and its tasks stay separate identities.
    let other = seeded(WORKFLOW_ID, &policy, T0);
    assert!(other.detail(VerifiedActor::Review01, review.id).is_err());
}

#[test]
fn attention_is_derived_from_records_and_never_a_lifecycle_state() {
    let policy = OrganizationPolicy::synthetic();
    // Due soon only within the explicit WorkType lead; overdue at and after the instant.
    let b = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    let soon = row(
        &b,
        VerifiedActor::Sales01,
        TaskView::Queue,
        CONTEXT_B_SALES_TASK_ID,
    )
    .unwrap();
    assert_eq!(kinds(&soon.attention), [AttentionKind::DueSoon]);
    assert_eq!(soon.attention[0].due_at.as_deref(), Some(later(6).as_str()));
    assert_eq!(soon.state, TaskState::Ready);
    let overdue = reattach(&b, &policy, &later(6));
    assert_eq!(
        kinds(
            &row(
                &overdue,
                VerifiedActor::Sales01,
                TaskView::Queue,
                CONTEXT_B_SALES_TASK_ID
            )
            .unwrap()
            .attention
        ),
        [AttentionKind::Overdue]
    );
    let c = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    assert_eq!(
        kinds(
            &row(
                &c,
                VerifiedActor::Sales01,
                TaskView::Context,
                CONTEXT_C_SALES_TASK_ID
            )
            .unwrap()
            .attention
        ),
        [AttentionKind::Overdue]
    );
    // The existing fixture has no due instant and therefore no due attention.
    let a = seeded(WORKFLOW_ID, &policy, T0);
    assert!(
        row(&a, VerifiedActor::Sales01, TaskView::Context, SALES_TASK_ID)
            .unwrap()
            .attention
            .is_empty()
    );

    // A manager's assignment is newly assigned for the assignee only, until acknowledged.
    let mut assigned = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    let MutationResult::Assigned { assignment, .. } = assigned
        .apply(
            VerifiedActor::Approver01,
            &Command::Assign {
                task_id: CONTEXT_B_SALES_TASK_ID,
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                expected_attempt_id: assigned.source.attempt_id,
                assignee: VerifiedActor::Sales01,
                assignee_responsibility_id: SALES_ASSIGNMENT_ID,
                reason: "担当の割当".into(),
            },
            T0,
        )
        .unwrap()
    else {
        panic!()
    };
    let own = row(
        &assigned,
        VerifiedActor::Sales01,
        TaskView::Context,
        CONTEXT_B_SALES_TASK_ID,
    )
    .unwrap();
    assert_eq!(
        kinds(&own.attention),
        [AttentionKind::NewlyAssigned, AttentionKind::DueSoon]
    );
    assert_eq!(own.attention[0].source_id, Some(assignment.id));
    let managed = row(
        &assigned,
        VerifiedActor::Approver01,
        TaskView::Queue,
        CONTEXT_B_SALES_TASK_ID,
    )
    .unwrap();
    assert_eq!(kinds(&managed.attention), [AttentionKind::DueSoon]);
    // Acknowledgment: own current period only; never another's or an old one.
    assert!(
        assigned
            .acknowledge(
                VerifiedActor::Sales01,
                CONTEXT_B_SALES_TASK_ID,
                assignment.id
            )
            .is_ok()
    );
    assert_eq!(
        assigned.acknowledge(
            VerifiedActor::Sales01,
            CONTEXT_B_SALES_TASK_ID,
            Uuid::now_v7()
        ),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(
        assigned.acknowledge(
            VerifiedActor::Approver01,
            CONTEXT_B_SALES_TASK_ID,
            assignment.id
        ),
        Err(WorkError::WorkItemNotFound)
    );
    let mut seen = assigned.clone();
    seen.attach_acknowledgements(VerifiedActor::Sales01, BTreeSet::from([assignment.id]));
    let acknowledged = row(
        &seen,
        VerifiedActor::Sales01,
        TaskView::Context,
        CONTEXT_B_SALES_TASK_ID,
    )
    .unwrap();
    assert_eq!(kinds(&acknowledged.attention), [AttentionKind::DueSoon]);
    assert_eq!(
        acknowledged.state,
        TaskState::Active,
        "acknowledging is not completion"
    );
    // Another actor's acknowledgment set never applies.
    let mut foreign = assigned.clone();
    foreign.attach_acknowledgements(VerifiedActor::Office01, BTreeSet::from([assignment.id]));
    assert_eq!(
        kinds(
            &row(
                &foreign,
                VerifiedActor::Sales01,
                TaskView::Context,
                CONTEXT_B_SALES_TASK_ID
            )
            .unwrap()
            .attention
        ),
        [AttentionKind::NewlyAssigned, AttentionKind::DueSoon]
    );

    // A returned attempt: the reason source is disclosed to its assignee only.
    let mut returned = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut returned, T0);
    let office = returned.next.clone().unwrap();
    returned
        .apply(
            VerifiedActor::Office01,
            &Command::Claim {
                task_id: office.id,
                context: ctx(OFFICE_ASSIGNMENT_ID, office.revision),
            },
            T0,
        )
        .unwrap();
    let office = returned.next.clone().unwrap();
    let MutationResult::Returned {
        return_instruction, ..
    } = returned
        .apply(
            VerifiedActor::Office01,
            &Command::Return {
                task_id: office.id,
                context: ctx(OFFICE_ASSIGNMENT_ID, office.revision),
                expected_attempt_id: office.attempt_id,
                previous_submission_id: office.handoff_snapshot_id.unwrap(),
                target_task_id: CONTEXT_C_SALES_TASK_ID,
                transition_id: RETURN_TRANSITION_ID,
                reason: "住所の記載を確認してください".into(),
            },
            T0,
        )
        .unwrap()
    else {
        panic!()
    };
    let ready = row(
        &returned,
        VerifiedActor::Sales01,
        TaskView::Queue,
        CONTEXT_C_SALES_TASK_ID,
    )
    .unwrap();
    assert_eq!(kinds(&ready.attention), [AttentionKind::Returned]);
    assert_eq!(ready.attention[0].source_id, None);
    assert_eq!(
        ready.due_at, None,
        "a returned attempt gets no invented due instant"
    );
    returned
        .apply(
            VerifiedActor::Sales01,
            &Command::Claim {
                task_id: CONTEXT_C_SALES_TASK_ID,
                context: ctx(SALES_ASSIGNMENT_ID, returned.source.revision),
            },
            T0,
        )
        .unwrap();
    let attention = returned
        .attention(VerifiedActor::Sales01, CONTEXT_C_SALES_TASK_ID)
        .unwrap();
    assert_eq!(kinds(&attention.items), [AttentionKind::Returned]);
    assert_eq!(attention.items[0].source_id, Some(return_instruction.id));
    // Completed work carries no attention; an unrelated actor learns nothing.
    let closed = row(
        &returned,
        VerifiedActor::Office01,
        TaskView::Queue,
        office.id,
    )
    .unwrap();
    assert_eq!(
        (closed.state, closed.attention.len()),
        (TaskState::Completed, 0)
    );
    assert_eq!(
        returned.attention(VerifiedActor::Delegate01, CONTEXT_C_SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
}

#[test]
fn context_identity_is_disclosed_only_to_owner_unit_readers_and_current_assignees() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    // The owner unit's sales role reads identity, progress and history.
    let sales = w.context_view(VerifiedActor::Sales01, None).unwrap();
    assert_eq!(sales.title, "合成依頼C・住所変更届");
    assert_eq!(sales.kind, WorkContextKind::Request);
    assert!(sales.can_read_history);
    assert_eq!(sales.progress.as_ref().unwrap().len(), 1);
    assert_eq!(sales.attention_count, 1);
    assert_eq!(
        row(
            &w,
            VerifiedActor::Sales01,
            TaskView::Queue,
            CONTEXT_C_SALES_TASK_ID
        )
        .unwrap()
        .context_title
        .as_deref(),
        Some("合成依頼C・住所変更届")
    );
    submit_sales(&mut w, T0);
    let progress = w
        .context_view(VerifiedActor::Sales01, None)
        .unwrap()
        .progress
        .unwrap();
    assert_eq!(
        progress
            .iter()
            .map(|item| (item.step_label.as_str(), item.state, item.assigned))
            .collect::<Vec<_>>(),
        [
            ("営業内容整理", TaskState::Completed, true),
            ("事務内容確認", TaskState::Ready, false)
        ]
    );
    // An eligible-only processor sees a generic queue row and no context.
    let office_task = CONTEXT_C_OFFICE_TASK_ID;
    let eligible = row(&w, VerifiedActor::Office01, TaskView::Queue, office_task).unwrap();
    assert!(eligible.can_claim);
    assert_eq!(eligible.context_title, None);
    assert!(w.context_view(VerifiedActor::Office01, None).is_none());
    assert_eq!(
        w.context_history(VerifiedActor::Office01),
        Err(WorkError::WorkContextNotFound)
    );
    // A manager administers assignment without customer identity.
    assert!(w.context_view(VerifiedActor::Approver01, None).is_none());
    assert_eq!(
        row(&w, VerifiedActor::Approver01, TaskView::Queue, office_task)
            .unwrap()
            .context_title,
        None
    );
    // Once assigned, the processor reads its context identity but not progress or history.
    w.apply(
        VerifiedActor::Office01,
        &Command::Claim {
            task_id: office_task,
            context: ctx(OFFICE_ASSIGNMENT_ID, w.next.as_ref().unwrap().revision),
        },
        T0,
    )
    .unwrap();
    let office = w.context_view(VerifiedActor::Office01, None).unwrap();
    assert_eq!(office.title, "合成依頼C・住所変更届");
    assert_eq!(office.progress, None);
    assert!(!office.can_read_history);
    assert_eq!(office.own_task_ids, [office_task]);
    assert_eq!(
        w.context_history(VerifiedActor::Office01),
        Err(WorkError::Forbidden)
    );
    // A selected scope narrows the projection to that responsibility only.
    assert!(
        w.context_view(VerifiedActor::Office01, Some(Uuid::now_v7()))
            .is_none()
    );
    assert_eq!(
        w.context_history(VerifiedActor::Sales01)
            .unwrap()
            .entries
            .len(),
        w.history.len()
    );
}

#[test]
fn profiles_follow_the_role_override_then_the_unit_default() {
    let policy = OrganizationPolicy::synthetic();
    let profile = |actor| {
        policy
            .responsibilities(actor, at(T0))
            .into_iter()
            .map(|value| (value.role_label, value.work_view_profile_id))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        profile(VerifiedActor::Sales01),
        [("営業".into(), PROFILE_SALES_CONTEXT_ID)]
    );
    assert_eq!(
        profile(VerifiedActor::Office01),
        [("事務処理".into(), PROFILE_OFFICE_QUEUE_ID)]
    );
    assert_eq!(
        profile(VerifiedActor::Review01),
        [("審査".into(), PROFILE_REVIEW_QUEUE_ID)]
    );
    assert_eq!(
        profile(VerifiedActor::MultiRole01),
        [
            ("事務処理".into(), PROFILE_OFFICE_QUEUE_ID),
            ("審査".into(), PROFILE_REVIEW_QUEUE_ID)
        ]
    );
    let profiles = work_view_profiles();
    assert_eq!(
        profiles
            .iter()
            .map(|value| (value.key.as_str(), value.archetype, value.initial_module))
            .collect::<Vec<_>>(),
        [
            (
                "sales-context",
                WorkViewArchetype::Context,
                ContextModule::Document
            ),
            (
                "office-queue",
                WorkViewArchetype::Queue,
                ContextModule::Document
            ),
            (
                "review-queue",
                WorkViewArchetype::Queue,
                ContextModule::Evidence
            ),
        ]
    );
    // Presentation only: the same reviewer projection is unchanged by profile choice.
    assert_eq!(
        serde_json::to_value(&profiles[2].modules[0]).unwrap(),
        serde_json::json!({"module": "evidence", "presentation": "prominent"})
    );
}

#[test]
fn records_resolve_to_their_owning_instance() {
    let policy = OrganizationPolicy::synthetic();
    let mut b = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    let c = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut b, T0);
    let snapshot = b.snapshots[0].id;
    assert!(b.owns(WorkTarget::Snapshot(snapshot)) && !c.owns(WorkTarget::Snapshot(snapshot)));
    assert!(b.owns(WorkTarget::Task(CONTEXT_B_REVIEW_TASK_ID)));
    assert!(
        c.owns(WorkTarget::Task(CONTEXT_C_OFFICE_TASK_ID)),
        "planned next step"
    );
    assert!(!c.owns(WorkTarget::Task(CONTEXT_B_SALES_TASK_ID)));
    assert!(
        b.owns(WorkTarget::Context(CONTEXT_B_ID)) && !b.owns(WorkTarget::Context(CONTEXT_C_ID))
    );
    assert!(b.owns(WorkTarget::Artifact(b.artifacts[0].id)));
}

fn claim_next(w: &mut Workflow, actor: VerifiedActor, acting: Uuid) {
    let next = w.next.clone().unwrap();
    w.apply(
        actor,
        &Command::Claim {
            task_id: next.id,
            context: ctx(acting, next.revision),
        },
        T0,
    )
    .unwrap();
}
fn return_next(w: &mut Workflow, actor: VerifiedActor, acting: Uuid) -> ReturnInstruction {
    let next = w.next.clone().unwrap();
    let MutationResult::Returned {
        return_instruction, ..
    } = w
        .apply(
            actor,
            &Command::Return {
                task_id: next.id,
                context: ctx(acting, next.revision),
                expected_attempt_id: next.attempt_id,
                previous_submission_id: next.handoff_snapshot_id.unwrap(),
                target_task_id: w.source.id,
                transition_id: RETURN_TRANSITION_ID,
                reason: "記載を確認してください".into(),
            },
            T0,
        )
        .unwrap()
    else {
        panic!()
    };
    return_instruction
}

#[test]
fn a_returned_review_context_resubmits_to_a_new_review_attempt_and_completes() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut w, T0);
    claim_next(&mut w, VerifiedActor::Review01, REVIEW_ASSIGNMENT_ID);
    return_next(&mut w, VerifiedActor::Review01, REVIEW_ASSIGNMENT_ID);
    submit_sales(&mut w, T0);
    let review = w.next.clone().unwrap();
    assert_eq!(
        (review.id, review.attempt_number, review.state),
        (CONTEXT_B_REVIEW_TASK_ID, 2, TaskState::Ready)
    );
    claim_next(&mut w, VerifiedActor::Review01, REVIEW_ASSIGNMENT_ID);
    let detail = w
        .detail(VerifiedActor::Review01, CONTEXT_B_REVIEW_TASK_ID)
        .unwrap();
    w.apply(
        VerifiedActor::Review01,
        &Command::Complete {
            task_id: CONTEXT_B_REVIEW_TASK_ID,
            context: ctx(REVIEW_ASSIGNMENT_ID, detail.task.revision),
            expected_attempt_id: detail.task.attempt_id,
            definition_action_id: detail.task.completion_action_id.unwrap(),
        },
        T0,
    )
    .unwrap();
    assert_eq!(w.next.unwrap().state, TaskState::Completed);
}

#[test]
fn returned_attention_belongs_only_to_the_attempt_the_return_targeted() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut w, T0);
    claim_next(&mut w, VerifiedActor::Office01, OFFICE_ASSIGNMENT_ID);
    let instruction = return_next(&mut w, VerifiedActor::Office01, OFFICE_ASSIGNMENT_ID);
    assert_eq!(
        kinds(
            &row(
                &w,
                VerifiedActor::Sales01,
                TaskView::Queue,
                CONTEXT_C_SALES_TASK_ID
            )
            .unwrap()
            .attention
        ),
        [AttentionKind::Returned]
    );
    submit_sales(&mut w, T0);
    // The re-received next attempt carries the return reference for comparison,
    // but it was not itself created by the return.
    let office = w.next.clone().unwrap();
    assert_eq!(office.return_instruction_id, Some(instruction.id));
    assert!(
        row(&w, VerifiedActor::Office01, TaskView::Queue, office.id)
            .unwrap()
            .attention
            .is_empty()
    );
    assert!(
        w.attention(VerifiedActor::Office01, office.id)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn attention_is_visible_exactly_where_the_list_row_is() {
    let mut policy = OrganizationPolicy::synthetic();
    // A delegation narrowed to queue.read alone never lists a claimable row.
    policy
        .apply(
            VerifiedActor::Sales01,
            &PolicyCommand::CreateDelegation {
                context: ctx(SALES_ASSIGNMENT_ID, 0),
                source_assignment_id: SALES_ASSIGNMENT_ID,
                recipient: VerifiedActor::Delegate01,
                actions: vec![PolicyAction::QueueRead],
                valid_from: None,
                valid_until: later(48),
                reason: "閲覧のみ".into(),
            },
            T0,
        )
        .unwrap();
    let w = seeded(CONTEXT_C_WORKFLOW_ID, &policy, T0);
    assert!(
        w.list_tasks(VerifiedActor::Delegate01, TaskView::Queue)
            .is_empty()
    );
    assert!(
        w.list_tasks(VerifiedActor::Delegate01, TaskView::Context)
            .is_empty()
    );
    assert_eq!(
        w.attention(VerifiedActor::Delegate01, CONTEXT_C_SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert!(
        w.attention(VerifiedActor::Sales01, CONTEXT_C_SALES_TASK_ID)
            .is_ok()
    );
}

#[test]
fn due_attention_boundaries_and_owner_unit_context_scope() {
    let policy = OrganizationPolicy::synthetic();
    let due = |now: &str| {
        let w = seeded(CONTEXT_B_WORKFLOW_ID, &policy, now);
        kinds(
            &row(
                &w,
                VerifiedActor::Sales01,
                TaskView::Queue,
                CONTEXT_B_SALES_TASK_ID,
            )
            .unwrap()
            .attention,
        )
    };
    // Lead 24h before a due instant of T0+6h: due soon from T0-18h, overdue at T0+6h.
    let before_lead = (at(T0) - Duration::hours(18) - Duration::seconds(1))
        .format(&Rfc3339)
        .unwrap();
    let lead = (at(T0) - Duration::hours(18)).format(&Rfc3339).unwrap();
    let just_before = (at(T0) + Duration::hours(6) - Duration::seconds(1))
        .format(&Rfc3339)
        .unwrap();
    assert!(due(&before_lead).is_empty());
    assert_eq!(due(&lead), [AttentionKind::DueSoon]);
    assert_eq!(due(&just_before), [AttentionKind::DueSoon]);
    assert_eq!(due(&later(6)), [AttentionKind::Overdue]);
    // context.read over another unit never discloses this owner unit's context.
    let mut other = OrganizationPolicy::synthetic();
    other.units[1].role_ids.push(ROLE_SALES_ID);
    let mut grant = other.role_assignments[0].clone();
    grant.id = Uuid::now_v7();
    grant.principal = VerifiedActor::Delegate01;
    grant.unit_id = UNIT_OFFICE_ID;
    other.role_assignments.push(grant);
    let w = seeded(CONTEXT_C_WORKFLOW_ID, &other, T0);
    assert!(w.context_view(VerifiedActor::Delegate01, None).is_none());
    assert!(
        w.list_tasks(VerifiedActor::Delegate01, TaskView::Context)
            .iter()
            .all(|item| item.context_title.is_none())
    );
    assert!(w.context_view(VerifiedActor::Sales01, None).is_some());
}

#[test]
fn context_identity_follows_open_assignment_and_is_never_on_generic_rows_or_replays() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = seeded(CONTEXT_B_WORKFLOW_ID, &policy, T0);
    submit_sales(&mut w, T0);
    let review_id = CONTEXT_B_REVIEW_TASK_ID;
    // An eligible-only reviewer and a manager see neither the title nor the opaque context ID.
    let queued = row(&w, VerifiedActor::Review01, TaskView::Queue, review_id).unwrap();
    assert_eq!((queued.context_id, queued.context_title), (None, None));
    let managed = row(&w, VerifiedActor::Approver01, TaskView::Queue, review_id).unwrap();
    assert_eq!((managed.context_id, managed.context_title), (None, None));
    // The owner unit's context reader keeps both.
    let sales = row(&w, VerifiedActor::Sales01, TaskView::Context, review_id).unwrap();
    assert_eq!(sales.context_id, Some(CONTEXT_B_ID));
    let next = w.next.clone().unwrap();
    let claimed = w
        .apply(
            VerifiedActor::Review01,
            &Command::Claim {
                task_id: review_id,
                context: ctx(REVIEW_ASSIGNMENT_ID, next.revision),
            },
            T0,
        )
        .unwrap();
    let MutationResult::Claimed { task } = &claimed else {
        panic!()
    };
    assert_eq!(task.context_id, Some(CONTEXT_B_ID));
    assert!(task.context_title.is_some());
    assert!(w.context_view(VerifiedActor::Review01, None).is_some());
    // Once the reviewer's attempt closes, its row and context keep no identity.
    return_next(&mut w, VerifiedActor::Review01, REVIEW_ASSIGNMENT_ID);
    let closed = row(&w, VerifiedActor::Review01, TaskView::Queue, review_id).unwrap();
    assert_eq!(closed.state, TaskState::Completed);
    assert_eq!((closed.context_id, closed.context_title), (None, None));
    assert!(w.context_view(VerifiedActor::Review01, None).is_none());
    // A stored receipt stays the committed receipt (§9); recovery still needs a
    // currently readable target record.
    assert_eq!(
        w.authorize_recovery(VerifiedActor::Review01, &claimed),
        Ok(())
    );
    assert_eq!(
        w.authorize_recovery(VerifiedActor::Office01, &claimed),
        Err(WorkError::WorkItemNotFound)
    );
}
