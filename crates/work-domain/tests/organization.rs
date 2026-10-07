//! Organization policy: multiple principals, concurrent roles, bounded delegation,
//! reassignment and revocation connected to the existing two-step Work fixture.
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_domain::*;

const T0: &str = "2026-10-07T09:00:00Z";
const T1: &str = "2026-10-07T10:00:00Z";
const T2: &str = "2026-10-07T12:00:00Z";
const T3: &str = "2026-10-07T13:00:00Z";
fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}
fn ctx(acting: Uuid, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: acting,
    }
}
fn attach(workflow: &Workflow, policy: &OrganizationPolicy, now: &str) -> Workflow {
    workflow.clone().with_authority(policy.clone(), at(now))
}
/// Sales private draft submitted; the office attempt is ready and unassigned.
fn received() -> Workflow {
    let mut w = Workflow::synthetic(None);
    let saved = w
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: ctx(SALES_ASSIGNMENT_ID, 0),
                value: TextValue {
                    text: "営業の非公開メモ".into(),
                },
            },
            T0,
        )
        .unwrap();
    let MutationResult::DraftSaved { artifact, .. } = saved else {
        panic!()
    };
    w.apply(
        VerifiedActor::Sales01,
        &Command::Submit {
            task_id: SALES_TASK_ID,
            context: ctx(SALES_ASSIGNMENT_ID, 1),
            expected_attempt_id: Some(SALES_ATTEMPT_ID),
            artifacts: vec![ArtifactSelection {
                artifact_id: artifact.id,
                revision: artifact.revision,
            }],
            evidence_revision_refs: vec![],
            finding_revision_refs: vec![],
            decision_revision_refs: vec![],
        },
        T0,
    )
    .unwrap();
    w
}
fn office(w: &Workflow) -> &WorkItem {
    w.next.as_ref().unwrap()
}
fn claim(acting: Uuid, w: &Workflow) -> Command {
    Command::Claim {
        task_id: OFFICE_TASK_ID,
        context: ctx(acting, office(w).revision),
    }
}
fn delegate(
    policy: &mut OrganizationPolicy,
    actor: VerifiedActor,
    acting: Uuid,
    source: Uuid,
    actions: &[PolicyAction],
    until: &str,
) -> Result<Delegation, WorkError> {
    let revision = policy.revision;
    match policy.apply(
        actor,
        &PolicyCommand::CreateDelegation {
            context: ctx(acting, revision),
            source_assignment_id: source,
            recipient: VerifiedActor::Delegate01,
            actions: actions.to_vec(),
            valid_from: None,
            valid_until: until.into(),
            reason: "休暇中の代理".into(),
        },
        T0,
    )? {
        MutationResult::DelegationCreated { delegation, .. } => Ok(delegation),
        other => panic!("{other:?}"),
    }
}
const PROCESSING: [PolicyAction; 5] = [
    PolicyAction::QueueRead,
    PolicyAction::WorkRead,
    PolicyAction::WorkClaim,
    PolicyAction::WorkEdit,
    PolicyAction::WorkComplete,
];

#[test]
fn synthetic_policy_has_formal_assignments_concurrent_roles_and_no_generic_admin() {
    let policy = OrganizationPolicy::synthetic();
    assert_eq!(policy.validate_integrity(), Ok(()));
    let now = at(T0);
    let roles = |actor| {
        policy
            .responsibilities(actor, now)
            .into_iter()
            .map(|value| value.role_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(roles(VerifiedActor::Sales01), [ROLE_SALES_ID]);
    assert_eq!(roles(VerifiedActor::Office01), [ROLE_PROCESSING_ID]);
    assert_eq!(roles(VerifiedActor::Review01), [ROLE_REVIEWING_ID]);
    assert_eq!(
        roles(VerifiedActor::MultiRole01),
        [ROLE_PROCESSING_ID, ROLE_REVIEWING_ID]
    );
    assert_eq!(
        roles(VerifiedActor::Approver01),
        [ROLE_APPROVING_ID, ROLE_MANAGEMENT_ID]
    );
    assert!(roles(VerifiedActor::Delegate01).is_empty());
    let managers: Vec<_> = VerifiedActor::ALL
        .into_iter()
        .filter(|actor| policy.can_manage(*actor, now))
        .collect();
    assert_eq!(managers, [VerifiedActor::Approver01]);
    // A responsibility ID belongs only to its principal.
    assert!(
        policy
            .responsibility(VerifiedActor::Sales01, OFFICE_ASSIGNMENT_ID, now)
            .is_none()
    );
    // Public spelling vs unchanged legacy digest encoding.
    let value = serde_json::to_value(&policy.role_assignments[5]).unwrap();
    assert_eq!(value["principal"], "multi-role-01");
    assert_eq!(
        serde_json::to_value(VerifiedActor::MultiRole01).unwrap(),
        json!("multi-role01")
    );
    assert_eq!(
        serde_json::to_value(PolicyAction::WorkClaim).unwrap(),
        json!("work.claim")
    );
    for profile in ["review-01", "approver-01", "multi-role-01", "delegate-01"] {
        assert!(VerifiedActor::from_startup_profile(profile).is_ok());
    }
    for profile in ["agent-01", "admin", "multi-role01"] {
        assert_eq!(
            VerifiedActor::from_startup_profile(profile),
            Err(WorkError::Forbidden)
        );
    }
}

#[test]
fn eligibility_queue_and_minimal_projection_follow_roles_not_principal_names() {
    let policy = OrganizationPolicy::synthetic();
    let w = attach(&received(), &policy, T0);
    let queue = |actor| w.list_tasks(actor, TaskView::Queue);
    // Eligible-only colleagues see the same item with a claim hint and no assignee.
    for actor in [VerifiedActor::Office01, VerifiedActor::MultiRole01] {
        let items = queue(actor);
        assert_eq!(items.len(), 1, "{actor:?}");
        assert_eq!(items[0].id, OFFICE_TASK_ID);
        assert!(items[0].can_claim);
        assert_eq!(items[0].required_role_id, Some(ROLE_PROCESSING_ID));
        assert!(items[0].assignment.is_none());
        assert!(!items[0].can_assign);
    }
    assert_eq!(
        queue(VerifiedActor::MultiRole01)[0].claim_assignment_id,
        Some(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID)
    );
    // A role without the step segment sees nothing; neither does an unassigned principal.
    assert!(queue(VerifiedActor::Review01).is_empty());
    assert!(queue(VerifiedActor::Delegate01).is_empty());
    assert_eq!(
        w.detail(VerifiedActor::MultiRole01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    // The selected reviewing responsibility of the multi-role principal is a separate scope.
    assert!(
        w.list_tasks_in(
            VerifiedActor::MultiRole01,
            TaskView::Queue,
            Some(MULTI_ROLE_REVIEW_ASSIGNMENT_ID)
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        w.list_tasks_in(
            VerifiedActor::Office01,
            TaskView::Queue,
            Some(MULTI_ROLE_REVIEW_ASSIGNMENT_ID)
        ),
        Err(WorkError::Forbidden)
    );
    // Management sees open items with the assignment hint, never a claim.
    let managed = w
        .list_tasks_in(
            VerifiedActor::Approver01,
            TaskView::Queue,
            Some(APPROVER_MANAGEMENT_ASSIGNMENT_ID),
        )
        .unwrap();
    assert_eq!(managed.len(), 1);
    assert!(managed[0].can_assign && !managed[0].can_claim);
    assert_eq!(
        w.detail(VerifiedActor::Approver01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
}

#[test]
fn concurrent_claims_on_the_same_revision_yield_one_assignment() {
    let policy = OrganizationPolicy::synthetic();
    let base = attach(&received(), &policy, T0);
    let office_claim = claim(OFFICE_ASSIGNMENT_ID, &base);
    let multi_claim = claim(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID, &base);
    let mut committed = base.clone();
    committed
        .apply(VerifiedActor::MultiRole01, &multi_claim, T0)
        .unwrap();
    // The loser evaluated the same revision: it never becomes a second assignee.
    let before = committed.clone();
    assert_eq!(
        committed.apply(VerifiedActor::Office01, &office_claim, T0),
        Err(WorkError::RevisionConflict)
    );
    assert_eq!(committed, before);
    let mut retry = claim(OFFICE_ASSIGNMENT_ID, &committed);
    if let Command::Claim { context, .. } = &mut retry {
        context.expected_revision = office(&committed).revision;
    }
    assert_eq!(
        committed.apply(VerifiedActor::Office01, &retry, T0),
        Err(WorkError::WorkAssignmentConflict)
    );
    let item = office(&committed);
    assert_eq!(item.assignee, Some(VerifiedActor::MultiRole01));
    assert_eq!(
        item.acting_assignment_id,
        Some(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID)
    );
    assert!(
        committed
            .detail(VerifiedActor::MultiRole01, OFFICE_TASK_ID)
            .is_ok()
    );
    assert_eq!(
        committed.detail(VerifiedActor::Office01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(committed.assignments.len(), 1);
    // A claim under a responsibility of the wrong role is not a disclosure path.
    let mut wrong = attach(&received(), &policy, T0);
    let before = wrong.clone();
    assert_eq!(
        wrong.apply(
            VerifiedActor::MultiRole01,
            &claim(MULTI_ROLE_REVIEW_ASSIGNMENT_ID, &before),
            T0
        ),
        Err(WorkError::Forbidden)
    );
    assert_eq!(wrong, before);
}

#[test]
fn delegation_is_bounded_and_expiry_or_revocation_ends_access_on_the_next_check() {
    let mut policy = OrganizationPolicy::synthetic();
    // Narrowing only: privilege administration and actions outside the role are refused.
    for actions in [
        vec![PolicyAction::WorkAssign],
        vec![PolicyAction::OrganizationManage],
        vec![PolicyAction::ContextRead],
        vec![],
    ] {
        let before = policy.clone();
        assert_eq!(
            delegate(
                &mut policy,
                VerifiedActor::Office01,
                OFFICE_ASSIGNMENT_ID,
                OFFICE_ASSIGNMENT_ID,
                &actions,
                T2,
            ),
            Err(WorkError::ValidationFailed),
            "{actions:?}"
        );
        assert_eq!(policy, before);
    }
    // Only the holder may delegate a formal assignment.
    assert_eq!(
        delegate(
            &mut policy,
            VerifiedActor::Sales01,
            SALES_ASSIGNMENT_ID,
            OFFICE_ASSIGNMENT_ID,
            &PROCESSING,
            T2
        ),
        Err(WorkError::Forbidden)
    );
    let delegation = delegate(
        &mut policy,
        VerifiedActor::Office01,
        OFFICE_ASSIGNMENT_ID,
        OFFICE_ASSIGNMENT_ID,
        &PROCESSING,
        T2,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&delegation).unwrap()["recipient"],
        "delegate-01"
    );
    // No nested delegation: a delegation is never a delegation source.
    assert_eq!(
        delegate(
            &mut policy,
            VerifiedActor::Delegate01,
            delegation.id,
            delegation.id,
            &PROCESSING,
            T2
        ),
        Err(WorkError::Forbidden)
    );
    let responsibilities = policy.responsibilities(VerifiedActor::Delegate01, at(T1));
    assert_eq!(responsibilities.len(), 1);
    assert_eq!(responsibilities[0].kind, ResponsibilityKind::Delegation);
    assert_eq!(responsibilities[0].delegator, Some(VerifiedActor::Office01));
    assert!(!responsibilities[0].allows(PolicyAction::WorkSubmit));

    let mut w = attach(&received(), &policy, T1);
    let items = w.list_tasks(VerifiedActor::Delegate01, TaskView::Queue);
    assert_eq!(items[0].claim_assignment_id, Some(delegation.id));
    w.apply(VerifiedActor::Delegate01, &claim(delegation.id, &w), T1)
        .unwrap();
    let detail = w.detail(VerifiedActor::Delegate01, OFFICE_TASK_ID).unwrap();
    assert_eq!(
        detail.task.assignment.as_ref().unwrap().acting_kind,
        Some(ResponsibilityKind::Delegation)
    );
    // Narrowed: the delegate cannot use actions the delegation omitted.
    assert!(!detail.task.can_return && !detail.task.can_hold);
    // Expiry at the exclusive end: the next evaluation hides private content.
    let expired = attach(&w, &policy, T2);
    assert_eq!(
        expired.detail(VerifiedActor::Delegate01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert!(
        expired
            .list_tasks(VerifiedActor::Delegate01, TaskView::Queue)
            .is_empty()
    );
    let managed = expired.list_tasks(VerifiedActor::Approver01, TaskView::Queue);
    let view = managed[0].assignment.as_ref().unwrap();
    assert_eq!(view.principal_id, "delegate-01");
    assert!(!view.responsibility_effective);
    // Explicit revocation is immediate even before the scheduled end.
    let mut revoked = policy.clone();
    revoked
        .apply(
            VerifiedActor::Office01,
            &PolicyCommand::RevokeDelegation {
                context: ctx(OFFICE_ASSIGNMENT_ID, revoked.revision),
                delegation_id: delegation.id,
                reason: "復帰".into(),
            },
            T1,
        )
        .unwrap();
    assert_eq!(
        attach(&w, &revoked, T1).detail(VerifiedActor::Delegate01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    // Revoking the delegator's own source assignment ends the delegation too.
    let mut source_revoked = policy.clone();
    source_revoked
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::RevokeRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, source_revoked.revision),
                assignment_id: OFFICE_ASSIGNMENT_ID,
                reason: "異動".into(),
            },
            T1,
        )
        .unwrap();
    assert!(
        source_revoked
            .responsibilities(VerifiedActor::Delegate01, at(T1))
            .is_empty()
    );
}

#[test]
fn reassignment_moves_attempt_private_work_and_revokes_the_old_assignee() {
    let mut policy = OrganizationPolicy::synthetic();
    // Manager grants a second sales responsibility for a synthetic colleague.
    let created = policy
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::CreateRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                principal: VerifiedActor::Review01,
                role_id: ROLE_SALES_ID,
                unit_id: UNIT_SALES_ID,
                valid_from: None,
                valid_until: Some(T3.into()),
                reason: "営業応援".into(),
            },
            T0,
        )
        .unwrap();
    let MutationResult::RoleAssignmentCreated { assignment, .. } = created else {
        panic!()
    };
    let mut w = Workflow::synthetic(None).with_authority(policy.clone(), at(T1));
    let MutationResult::DraftSaved { artifact, .. } = w
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: ctx(SALES_ASSIGNMENT_ID, 0),
                value: TextValue {
                    text: "引継ぎ前の非公開文案".into(),
                },
            },
            T1,
        )
        .unwrap()
    else {
        panic!()
    };
    let assign = |assignee, responsibility, revision| Command::Assign {
        task_id: SALES_TASK_ID,
        context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, revision),
        expected_attempt_id: SALES_ATTEMPT_ID,
        assignee,
        assignee_responsibility_id: responsibility,
        reason: "担当者の不在".into(),
    };
    // Non-managers cannot assign; the assignee must hold the step's role.
    let before = w.clone();
    let mut forged = assign(VerifiedActor::Review01, assignment.id, 1);
    if let Command::Assign { context, .. } = &mut forged {
        context.acting_assignment_id = SALES_ASSIGNMENT_ID;
    }
    assert_eq!(
        w.apply(VerifiedActor::Sales01, &forged, T1),
        Err(WorkError::Forbidden)
    );
    assert_eq!(
        w.apply(
            VerifiedActor::Approver01,
            &assign(VerifiedActor::Review01, REVIEW_ASSIGNMENT_ID, 1),
            T1
        ),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(w, before);
    let result = w
        .apply(
            VerifiedActor::Approver01,
            &assign(VerifiedActor::Review01, assignment.id, 1),
            T1,
        )
        .unwrap();
    let MutationResult::Assigned {
        task,
        assignment: record,
    } = result
    else {
        panic!()
    };
    assert_eq!(task.assignment.unwrap().principal_id, "review-01");
    assert_eq!(record.assigned_by, Some(VerifiedActor::Approver01));
    assert_eq!(
        record.manager_assignment_id,
        Some(APPROVER_MANAGEMENT_ASSIGNMENT_ID)
    );
    assert_eq!(w.history.last().unwrap().kind, "assigned");
    // Same attempt and private draft: the new assignee reads it, the old one cannot.
    assert_eq!(w.source.attempt_id, SALES_ATTEMPT_ID);
    assert_eq!(
        w.artifact(VerifiedActor::Review01, artifact.id)
            .unwrap()
            .value,
        artifact.value
    );
    assert_eq!(
        w.artifact(VerifiedActor::Sales01, artifact.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        w.detail(VerifiedActor::Sales01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    // Old commands under the previous assignment are refused, not replayed.
    let before = w.clone();
    assert_eq!(
        w.apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: Some(artifact.id),
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
                value: TextValue {
                    text: "書換え".into(),
                },
            },
            T1,
        ),
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(w, before);
    // A scheduled end of the formal assignment hides the work at the next check.
    assert_eq!(
        attach(&w, &policy, T3).detail(VerifiedActor::Review01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    // Attribution: the claim-less fixture period is not rewritten; the new one is open.
    assert_eq!(w.assignments.len(), 1);
    assert!(w.assignments[0].ended_at.is_none());
}

#[test]
fn role_assignment_revocation_ends_access_without_rewriting_history() {
    let mut policy = OrganizationPolicy::synthetic();
    let mut w = attach(&received(), &policy, T0);
    w.apply(
        VerifiedActor::Office01,
        &claim(OFFICE_ASSIGNMENT_ID, &w),
        T0,
    )
    .unwrap();
    // Non-managers and the acting management assignment itself are refused.
    let before = policy.clone();
    let revoke = |acting, id, revision| PolicyCommand::RevokeRoleAssignment {
        context: ctx(acting, revision),
        assignment_id: id,
        reason: "異動".into(),
    };
    assert_eq!(
        policy.apply(
            VerifiedActor::Office01,
            &revoke(OFFICE_ASSIGNMENT_ID, OFFICE_ASSIGNMENT_ID, 0),
            T1
        ),
        Err(WorkError::Forbidden)
    );
    assert_eq!(
        policy.apply(
            VerifiedActor::Approver01,
            &revoke(
                APPROVER_MANAGEMENT_ASSIGNMENT_ID,
                APPROVER_MANAGEMENT_ASSIGNMENT_ID,
                0
            ),
            T1
        ),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(
        policy.apply(
            VerifiedActor::Approver01,
            &revoke(APPROVER_MANAGEMENT_ASSIGNMENT_ID, OFFICE_ASSIGNMENT_ID, 7),
            T1
        ),
        Err(WorkError::RevisionConflict)
    );
    assert_eq!(policy, before);
    let result = policy
        .apply(
            VerifiedActor::Approver01,
            &revoke(APPROVER_MANAGEMENT_ASSIGNMENT_ID, OFFICE_ASSIGNMENT_ID, 0),
            T1,
        )
        .unwrap();
    // The receipt carries the resulting policy revision for the next OCC check.
    assert_eq!(serde_json::to_value(&result).unwrap()["policyRevision"], 1);
    assert_eq!(policy.revision, 1);
    let revoked = attach(&w, &policy, T1);
    assert_eq!(
        revoked.detail(VerifiedActor::Office01, OFFICE_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    assert!(
        revoked
            .list_tasks(VerifiedActor::Office01, TaskView::Context)
            .is_empty()
    );
    // The work is not silently released; the manager sees an ended responsibility.
    assert_eq!(office(&revoked).assignee, Some(VerifiedActor::Office01));
    let managed = revoked.list_tasks(VerifiedActor::Approver01, TaskView::Queue);
    assert!(
        !managed[0]
            .assignment
            .as_ref()
            .unwrap()
            .responsibility_effective
    );
    // Revocation is recorded; the original record is kept and not deleted.
    let record = policy
        .role_assignments
        .iter()
        .find(|value| value.id == OFFICE_ASSIGNMENT_ID)
        .unwrap();
    assert_eq!(record.revoked_by, Some(VerifiedActor::Approver01));
    assert_eq!(record.valid_from, "2026-10-01T00:00:00Z");
    // A second revocation of the same record is refused.
    assert_eq!(
        policy.apply(
            VerifiedActor::Approver01,
            &revoke(APPROVER_MANAGEMENT_ASSIGNMENT_ID, OFFICE_ASSIGNMENT_ID, 1),
            T1
        ),
        Err(WorkError::ValidationFailed)
    );
}

#[test]
fn role_assignment_creation_validates_scope_time_and_duplicates() {
    let mut policy = OrganizationPolicy::synthetic();
    let create =
        |principal, role_id, unit_id, from: Option<&str>, until: Option<&str>, revision| {
            PolicyCommand::CreateRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, revision),
                principal,
                role_id,
                unit_id,
                valid_from: from.map(Into::into),
                valid_until: until.map(Into::into),
                reason: "応援".into(),
            }
        };
    let before = policy.clone();
    for command in [
        // Role not offered by the unit.
        create(
            VerifiedActor::Delegate01,
            ROLE_SALES_ID,
            UNIT_OFFICE_ID,
            None,
            None,
            0,
        ),
        // Inverted and already-ended validity.
        create(
            VerifiedActor::Delegate01,
            ROLE_PROCESSING_ID,
            UNIT_OFFICE_ID,
            Some(T2),
            Some(T1),
            0,
        ),
        create(
            VerifiedActor::Delegate01,
            ROLE_PROCESSING_ID,
            UNIT_OFFICE_ID,
            Some("2026-10-01T00:00:00Z"),
            Some("2026-10-02T00:00:00Z"),
            0,
        ),
        // Duplicate of an effective identical formal assignment.
        create(
            VerifiedActor::Office01,
            ROLE_PROCESSING_ID,
            UNIT_OFFICE_ID,
            None,
            None,
            0,
        ),
    ] {
        assert_eq!(
            policy.apply(VerifiedActor::Approver01, &command, T0),
            Err(WorkError::ValidationFailed),
            "{command:?}"
        );
    }
    assert_eq!(
        policy.apply(
            VerifiedActor::Sales01,
            &PolicyCommand::CreateRoleAssignment {
                context: ctx(SALES_ASSIGNMENT_ID, 0),
                principal: VerifiedActor::Sales01,
                role_id: ROLE_MANAGEMENT_ID,
                unit_id: UNIT_APPROVAL_ID,
                valid_from: None,
                valid_until: None,
                reason: "自己昇格".into(),
            },
            T0
        ),
        Err(WorkError::Forbidden)
    );
    assert_eq!(policy, before);
    // A future-dated assignment is recorded but not yet effective.
    policy
        .apply(
            VerifiedActor::Approver01,
            &create(
                VerifiedActor::Delegate01,
                ROLE_PROCESSING_ID,
                UNIT_OFFICE_ID,
                Some(T2),
                Some(T3),
                0,
            ),
            T0,
        )
        .unwrap();
    assert!(
        policy
            .responsibilities(VerifiedActor::Delegate01, at(T1))
            .is_empty()
    );
    assert_eq!(
        policy
            .responsibilities(VerifiedActor::Delegate01, at(T2))
            .len(),
        1
    );
    // The holder sees only own records; the manager sees all.
    assert_eq!(
        policy
            .view(VerifiedActor::Delegate01, at(T2))
            .unwrap()
            .role_assignments
            .len(),
        1
    );
    let all = policy.view(VerifiedActor::Approver01, at(T2)).unwrap();
    assert!(all.can_manage);
    assert_eq!(all.role_assignments.len(), 8);
}

fn return_to_sales(w: &mut Workflow, now: &str) -> ReturnInstruction {
    let item = office(w).clone();
    let returned = w
        .apply(
            VerifiedActor::Office01,
            &Command::Return {
                task_id: OFFICE_TASK_ID,
                context: ctx(OFFICE_ASSIGNMENT_ID, item.revision),
                expected_attempt_id: item.attempt_id,
                previous_submission_id: item.handoff_snapshot_id.unwrap(),
                target_task_id: SALES_TASK_ID,
                transition_id: RETURN_TRANSITION_ID,
                reason: "記載の確認をお願いします".into(),
            },
            now,
        )
        .unwrap();
    let MutationResult::Returned {
        return_instruction, ..
    } = returned
    else {
        panic!()
    };
    return_instruction
}

#[test]
fn returned_attempt_assignee_reads_its_instruction_and_prior_submission() {
    let mut policy = OrganizationPolicy::synthetic();
    let sales_scope = [
        PolicyAction::QueueRead,
        PolicyAction::WorkRead,
        PolicyAction::WorkClaim,
        PolicyAction::WorkEdit,
    ];
    let delegation = delegate(
        &mut policy,
        VerifiedActor::Sales01,
        SALES_ASSIGNMENT_ID,
        SALES_ASSIGNMENT_ID,
        &sales_scope,
        T3,
    )
    .unwrap();
    let mut w = attach(&received(), &policy, T1);
    w.apply(
        VerifiedActor::Office01,
        &claim(OFFICE_ASSIGNMENT_ID, &w),
        T1,
    )
    .unwrap();
    let submitted = office(&w).handoff_snapshot_id.unwrap();
    let instruction = return_to_sales(&mut w, T1);
    // Eligible-only rows never carry submission or return identifiers.
    let row = w
        .list_tasks(VerifiedActor::Delegate01, TaskView::Queue)
        .into_iter()
        .find(|item| item.id == SALES_TASK_ID)
        .unwrap();
    assert!(row.can_claim);
    assert_eq!(
        (row.return_instruction_id, row.handoff_snapshot_id),
        (None, None)
    );
    assert_eq!(
        w.return_instruction(VerifiedActor::Delegate01, instruction.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    // The delegate claims the returned attempt and receives its rework context.
    w.apply(
        VerifiedActor::Delegate01,
        &Command::Claim {
            task_id: SALES_TASK_ID,
            context: ctx(delegation.id, w.source.revision),
        },
        T1,
    )
    .unwrap();
    let detail = w.detail(VerifiedActor::Delegate01, SALES_TASK_ID).unwrap();
    assert_eq!(detail.task.return_instruction_id, Some(instruction.id));
    assert_eq!(detail.task.handoff_snapshot_id, Some(submitted));
    assert_eq!(
        w.return_instruction(VerifiedActor::Delegate01, instruction.id)
            .unwrap()
            .reason,
        "記載の確認をお願いします"
    );
    assert_eq!(
        w.snapshot(VerifiedActor::Delegate01, submitted).unwrap().id,
        submitted
    );
    // The returning office keeps its outbound instruction read-only.
    assert!(
        w.return_instruction(VerifiedActor::Office01, instruction.id)
            .is_ok()
    );
    // Revocation ends the delegate's rework access on the next check.
    let mut revoked = policy.clone();
    revoked
        .apply(
            VerifiedActor::Sales01,
            &PolicyCommand::RevokeDelegation {
                context: ctx(SALES_ASSIGNMENT_ID, revoked.revision),
                delegation_id: delegation.id,
                reason: "復帰".into(),
            },
            T1,
        )
        .unwrap();
    let w = attach(&w, &revoked, T1);
    assert_eq!(
        w.return_instruction(VerifiedActor::Delegate01, instruction.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        w.snapshot(VerifiedActor::Delegate01, submitted),
        Err(WorkError::WorkArtifactNotFound)
    );
}

#[test]
fn management_never_grants_itself_access_or_acts_in_a_delegators_name() {
    let mut policy = OrganizationPolicy::synthetic();
    let before = policy.clone();
    // No self-grant of any role, including a step role that would expose drafts.
    assert_eq!(
        policy.apply(
            VerifiedActor::Approver01,
            &PolicyCommand::CreateRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                principal: VerifiedActor::Approver01,
                role_id: ROLE_SALES_ID,
                unit_id: UNIT_SALES_ID,
                valid_from: None,
                valid_until: None,
                reason: "自己付与".into(),
            },
            T0,
        ),
        Err(WorkError::Forbidden)
    );
    // A manager never creates a delegation attributed to another holder.
    for recipient in [VerifiedActor::Approver01, VerifiedActor::Delegate01] {
        assert_eq!(
            policy.apply(
                VerifiedActor::Approver01,
                &PolicyCommand::CreateDelegation {
                    context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                    source_assignment_id: SALES_ASSIGNMENT_ID,
                    recipient,
                    actions: vec![PolicyAction::WorkRead],
                    valid_from: None,
                    valid_until: T2.into(),
                    reason: "代理".into(),
                },
                T0,
            ),
            Err(WorkError::Forbidden),
            "{recipient:?}"
        );
    }
    assert_eq!(policy, before);
    // A second manager who also holds the step role claims; it never self-assigns.
    policy
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::CreateRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                principal: VerifiedActor::MultiRole01,
                role_id: ROLE_MANAGEMENT_ID,
                unit_id: UNIT_APPROVAL_ID,
                valid_from: None,
                valid_until: None,
                reason: "管理応援".into(),
            },
            T0,
        )
        .unwrap();
    let manager = policy
        .responsibilities(VerifiedActor::MultiRole01, at(T1))
        .into_iter()
        .find(|value| value.role_id == ROLE_MANAGEMENT_ID)
        .unwrap();
    let mut w = attach(&received(), &policy, T1);
    let unchanged = w.clone();
    assert_eq!(
        w.apply(
            VerifiedActor::MultiRole01,
            &Command::Assign {
                task_id: OFFICE_TASK_ID,
                context: ctx(manager.id, office(&w).revision),
                expected_attempt_id: office(&w).attempt_id,
                assignee: VerifiedActor::MultiRole01,
                assignee_responsibility_id: MULTI_ROLE_PROCESSING_ASSIGNMENT_ID,
                reason: "自分へ".into(),
            },
            T1,
        ),
        Err(WorkError::Forbidden)
    );
    assert_eq!(w, unchanged);
    w.apply(
        VerifiedActor::MultiRole01,
        &claim(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID, &w),
        T1,
    )
    .unwrap();
}

#[test]
fn policy_reasons_and_record_bounds_keep_collections_readable() {
    let mut policy = OrganizationPolicy::synthetic();
    let create = |policy: &mut OrganizationPolicy, actor, acting, reason: &str| {
        let revision = policy.revision;
        policy.apply(
            actor,
            &PolicyCommand::CreateDelegation {
                context: ctx(acting, revision),
                source_assignment_id: acting,
                recipient: VerifiedActor::Delegate01,
                actions: vec![PolicyAction::WorkRead],
                valid_from: None,
                valid_until: T2.into(),
                reason: reason.into(),
            },
            T0,
        )
    };
    // Control characters would expand sixfold when JSON-escaped.
    let before = policy.clone();
    for reason in ["\u{1}理由", "理由\u{7f}", "理由\u{85}"] {
        assert_eq!(
            create(
                &mut policy,
                VerifiedActor::Office01,
                OFFICE_ASSIGNMENT_ID,
                reason
            ),
            Err(WorkError::ValidationFailed),
            "{reason:?}"
        );
    }
    assert_eq!(policy, before);
    // One holder exhausts only its own bound; others still delegate.
    for _ in 0..MAX_DELEGATIONS_PER_DELEGATOR {
        let MutationResult::DelegationCreated { delegation, .. } = create(
            &mut policy,
            VerifiedActor::Office01,
            OFFICE_ASSIGNMENT_ID,
            "休暇\n\t代理",
        )
        .unwrap() else {
            panic!()
        };
        let revision = policy.revision;
        policy
            .apply(
                VerifiedActor::Office01,
                &PolicyCommand::RevokeDelegation {
                    context: ctx(OFFICE_ASSIGNMENT_ID, revision),
                    delegation_id: delegation.id,
                    reason: "復帰".into(),
                },
                T0,
            )
            .unwrap();
    }
    assert_eq!(
        create(
            &mut policy,
            VerifiedActor::Office01,
            OFFICE_ASSIGNMENT_ID,
            "上限"
        ),
        Err(WorkError::ValidationFailed)
    );
    assert!(
        create(
            &mut policy,
            VerifiedActor::Sales01,
            SALES_ASSIGNMENT_ID,
            "別の委任者"
        )
        .is_ok()
    );
    // Worst case: every bounded record carries maximal escaping reasons.
    let mut full = OrganizationPolicy::synthetic();
    let quoted = "\"".repeat(MAX_POLICY_REASON_BYTES);
    let template = full.role_assignments[0].clone();
    while full.role_assignments.len() < MAX_ROLE_ASSIGNMENTS {
        let mut value = template.clone();
        value.id = Uuid::now_v7();
        value.reason = quoted.clone();
        value.valid_until = Some(T3.into());
        value.revoked_at = Some(T1.into());
        value.revoked_by = Some(VerifiedActor::Approver01);
        value.revoke_reason = Some(quoted.clone());
        full.role_assignments.push(value);
    }
    let delegation = Delegation {
        id: Uuid::nil(),
        source_assignment_id: OFFICE_ASSIGNMENT_ID,
        delegator: VerifiedActor::Office01,
        recipient: VerifiedActor::Delegate01,
        actions: PROCESSING.to_vec(),
        valid_from: T0.into(),
        valid_until: T3.into(),
        reason: quoted.clone(),
        created_by: VerifiedActor::Office01,
        created_at: T0.into(),
        revoked_at: Some(T1.into()),
        revoked_by: Some(VerifiedActor::Office01),
        revoke_reason: Some(quoted.clone()),
    };
    while full.delegations.len() < MAX_DELEGATIONS {
        let mut value = delegation.clone();
        value.id = Uuid::now_v7();
        full.delegations.push(value);
    }
    assert_eq!(full.validate_integrity(), Ok(()));
    let view = full.view(VerifiedActor::Approver01, at(T1)).unwrap();
    let page = |items: serde_json::Value| {
        serde_json::to_vec(&json!({"items": items, "nextCursor": null, "evaluatedAt": T1}))
            .unwrap()
            .len()
    };
    const RESPONSE_LIMIT: usize = 1024 * 1024;
    assert!(page(serde_json::to_value(&view.role_assignments).unwrap()) < RESPONSE_LIMIT / 2);
    assert!(page(serde_json::to_value(&view.delegations).unwrap()) < RESPONSE_LIMIT / 2);
}

#[test]
fn policy_history_is_never_backdated() {
    let mut policy = OrganizationPolicy::synthetic();
    let command = |from: &str, revision| PolicyCommand::CreateRoleAssignment {
        context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, revision),
        principal: VerifiedActor::Delegate01,
        role_id: ROLE_PROCESSING_ID,
        unit_id: UNIT_OFFICE_ID,
        valid_from: Some(from.into()),
        valid_until: None,
        reason: "応援".into(),
    };
    let before = policy.clone();
    assert_eq!(
        policy.apply(
            VerifiedActor::Approver01,
            &command("2026-10-07T08:00:00Z", 0),
            T0
        ),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(policy, before);
    // A small client clock lag starts the record at the trusted server instant.
    let MutationResult::RoleAssignmentCreated { assignment, .. } = policy
        .apply(
            VerifiedActor::Approver01,
            &command("2026-10-07T08:58:00Z", 0),
            T0,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(assignment.valid_from, T0);
    let revision = policy.revision;
    assert_eq!(
        policy.apply(
            VerifiedActor::Office01,
            &PolicyCommand::CreateDelegation {
                context: ctx(OFFICE_ASSIGNMENT_ID, revision),
                source_assignment_id: OFFICE_ASSIGNMENT_ID,
                recipient: VerifiedActor::Delegate01,
                actions: vec![PolicyAction::WorkRead],
                valid_from: Some("2026-10-01T00:00:00Z".into()),
                valid_until: T2.into(),
                reason: "遡及".into(),
            },
            T0,
        ),
        Err(WorkError::ValidationFailed)
    );
}

#[test]
fn a_workflow_without_an_attached_policy_grants_nothing() {
    let stored = serde_json::to_value(Workflow::synthetic(None)).unwrap();
    let w: Workflow = serde_json::from_value(stored).unwrap();
    for actor in VerifiedActor::ALL {
        assert!(
            w.list_tasks(actor, TaskView::Context).is_empty(),
            "{actor:?}"
        );
        assert!(w.list_tasks(actor, TaskView::Queue).is_empty(), "{actor:?}");
    }
    assert_eq!(
        w.detail(VerifiedActor::Sales01, SALES_TASK_ID),
        Err(WorkError::WorkItemNotFound)
    );
    let mut candidate = w.clone();
    assert!(
        candidate
            .apply(
                VerifiedActor::Sales01,
                &Command::SaveDraft {
                    task_id: SALES_TASK_ID,
                    artifact_id: None,
                    context: ctx(SALES_ASSIGNMENT_ID, 0),
                    value: TextValue { text: "x".into() },
                },
                T0,
            )
            .is_err()
    );
    assert_eq!(candidate, w);
}

#[test]
fn reassignment_reasons_and_periods_per_attempt_are_bounded() {
    let mut policy = OrganizationPolicy::synthetic();
    let MutationResult::RoleAssignmentCreated { assignment, .. } = policy
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::CreateRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, 0),
                principal: VerifiedActor::Review01,
                role_id: ROLE_SALES_ID,
                unit_id: UNIT_SALES_ID,
                valid_from: None,
                valid_until: None,
                reason: "営業応援".into(),
            },
            T0,
        )
        .unwrap()
    else {
        panic!()
    };
    let mut w = Workflow::synthetic(None).with_authority(policy, at(T1));
    let assign = |w: &Workflow, assignee, responsibility, reason: &str| Command::Assign {
        task_id: SALES_TASK_ID,
        context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, w.source.revision),
        expected_attempt_id: SALES_ATTEMPT_ID,
        assignee,
        assignee_responsibility_id: responsibility,
        reason: reason.into(),
    };
    for reason in ["a".repeat(MAX_POLICY_REASON_BYTES + 1), "不在\u{1}".into()] {
        let command = assign(&w, VerifiedActor::Review01, assignment.id, &reason);
        assert_eq!(
            w.clone().apply(VerifiedActor::Approver01, &command, T1),
            Err(WorkError::ValidationFailed)
        );
    }
    let targets = [
        (VerifiedActor::Review01, assignment.id),
        (VerifiedActor::Sales01, SALES_ASSIGNMENT_ID),
    ];
    for index in 0..MAX_ASSIGNMENTS_PER_ATTEMPT {
        let (assignee, responsibility) = targets[index % 2];
        let command = assign(&w, assignee, responsibility, "交代");
        w.apply(VerifiedActor::Approver01, &command, T1).unwrap();
    }
    let (assignee, responsibility) = targets[MAX_ASSIGNMENTS_PER_ATTEMPT % 2];
    let before = w.clone();
    assert_eq!(
        w.apply(
            VerifiedActor::Approver01,
            &assign(&w, assignee, responsibility, "交代"),
            T1
        ),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(w, before);
    // Once the assignee's responsibility ends, the bound never strands the attempt.
    let mut ended = policy_with(&assignment);
    let revision = ended.revision;
    let current = w.source.acting_assignment_id.unwrap();
    ended
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::RevokeRoleAssignment {
                context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, revision),
                assignment_id: current,
                reason: "異動".into(),
            },
            T1,
        )
        .unwrap();
    let mut w = attach(&w, &ended, T1);
    let replacement = if current == SALES_ASSIGNMENT_ID {
        (VerifiedActor::Review01, assignment.id)
    } else {
        (VerifiedActor::Sales01, SALES_ASSIGNMENT_ID)
    };
    w.apply(
        VerifiedActor::Approver01,
        &assign(&w, replacement.0, replacement.1, "責任終了後の交代"),
        T1,
    )
    .unwrap();
}
fn policy_with(assignment: &RoleAssignment) -> OrganizationPolicy {
    let mut policy = OrganizationPolicy::synthetic();
    policy.role_assignments.push(assignment.clone());
    policy
}
