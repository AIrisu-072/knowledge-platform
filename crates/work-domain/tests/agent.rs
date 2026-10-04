use uuid::Uuid;
use work_domain::*;
const NOW: &str = "2026-10-04T13:00:00Z";
const ACTOR: VerifiedActor = VerifiedActor::Sales01;
fn context(w: &Workflow) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: w.source.revision,
        acting_assignment_id: ACTOR.assignment_id(),
    }
}
fn fixture() -> Workflow {
    let mut w = Workflow::synthetic(Some(Uuid::from_u128(71)));
    let c = Command::RegisterEvidence {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
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
        relevant_location: "合成根拠".into(),
    };
    w.apply(ACTOR, &c, NOW).unwrap();
    w
}
fn request(w: &mut Workflow) -> AgentExecution {
    let c = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "根拠参照を候補にまとめる".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: w.evidence[0].id,
            revision: 1,
        }],
    };
    match w.apply(ACTOR, &c, NOW).unwrap() {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => panic!(),
    }
}
fn output() -> AgentFindingOutput {
    AgentFindingOutput {
        summary: "合成実行・本文分析なし".into(),
        claim: "人間が原本を確認してください".into(),
        uncertainty: vec!["本文分析なし・実LLM/MCP通信なし".into()],
    }
}
#[test]
fn agent_private_context_cancel_fences_late_output() {
    let mut w = fixture();
    let e = request(&mut w);
    assert_eq!(
        w.agent_execution(VerifiedActor::Office01, e.id),
        Err(WorkError::WorkItemNotFound)
    );
    let ctx = w.start_agent_execution(ACTOR, e.id, NOW).unwrap().unwrap();
    assert!(w.start_agent_execution(ACTOR, e.id, NOW).unwrap().is_none());
    let cancel = Command::CancelAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        execution_id: e.id,
    };
    w.apply(ACTOR, &cancel, NOW).unwrap();
    let before = w.clone();
    assert!(w.finish_agent_execution(&ctx, output(), NOW).is_err());
    assert_eq!(w, before);
    assert!(w.findings.is_empty());
    assert_eq!(
        w.fail_agent_execution(ACTOR, e.id, AgentFailureCode::CommitOutcomeUnknown, NOW)
            .unwrap()
            .status,
        AgentExecutionStatus::Cancelled
    );
}
#[test]
fn agent_finish_atomic_trusted_provenance_no_human_decision_and_cancel_preserves_success() {
    let mut w = fixture();
    let e = request(&mut w);
    let ctx = w.start_agent_execution(ACTOR, e.id, NOW).unwrap().unwrap();
    let done = w.finish_agent_execution(&ctx, output(), NOW).unwrap();
    assert_eq!(done.status, AgentExecutionStatus::Succeeded);
    assert_eq!(
        w.fail_agent_execution(ACTOR, e.id, AgentFailureCode::CommitOutcomeUnknown, NOW)
            .unwrap()
            .status,
        AgentExecutionStatus::Succeeded
    );
    assert_eq!(w.findings.len(), 1);
    assert!(w.decisions.is_empty());
    assert_eq!(w.findings[0].author, "organization-synthetic/agent-01");
    assert_eq!(w.findings[0].origin_execution_id, Some(e.id));
    let result = w.agent_result(ACTOR, e.id).unwrap();
    assert!(result.simulated);
    assert!(!result.body_analyzed);
    assert!(!result.live_llm);
    assert!(!result.mcp_wire_executed);
    let f = w.findings[0].clone();
    let decision = Command::RecordDecision {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        finding_id: f.id,
        finding_revision: 1,
        decision: DecisionKind::Accepted,
        adopted_claim: None,
        reason: None,
        evidence_revision_refs: f.evidence_revision_refs.clone(),
        supersedes_decision_id: None,
    };
    w.apply(ACTOR, &decision, NOW).unwrap();
    assert_eq!(w.agent_result(ACTOR, e.id).unwrap(), result);
    let cancel = Command::CancelAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        execution_id: e.id,
    };
    w.apply(ACTOR, &cancel, NOW).unwrap();
    assert_eq!(
        w.agent_execution(ACTOR, e.id).unwrap().status,
        AgentExecutionStatus::Succeeded
    );
    assert_eq!(w.findings.len(), 1);
}
#[test]
fn agent_context_mutation_and_invalid_output_roll_back() {
    let mut w = fixture();
    let e = request(&mut w);
    let ctx = w.start_agent_execution(ACTOR, e.id, NOW).unwrap().unwrap();
    let mut bad = output();
    bad.uncertainty.clear();
    let before = w.clone();
    assert!(w.finish_agent_execution(&ctx, bad, NOW).is_err());
    assert_eq!(w, before);
    w.source.revision += 1;
    let before = w.clone();
    assert!(w.finish_agent_execution(&ctx, output(), NOW).is_err());
    assert_eq!(w, before);
}
#[test]
fn agent_bounds_and_interruption_never_restart() {
    let mut w = fixture();
    let e = request(&mut w);
    let before = w.clone();
    let c = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "x".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: w.evidence[0].id,
            revision: 1,
        }],
    };
    assert!(w.apply(ACTOR, &c, NOW).is_err());
    assert_eq!(w, before);
    assert_eq!(
        w.interrupt_agent_executions(VerifiedActor::Office01, NOW)
            .unwrap(),
        0
    );
    assert_eq!(w.interrupt_agent_executions(ACTOR, NOW).unwrap(), 1);
    assert_eq!(
        w.agent_execution(ACTOR, e.id).unwrap().status,
        AgentExecutionStatus::OutcomeUnknown
    );
    assert!(w.start_agent_execution(ACTOR, e.id, NOW).unwrap().is_none());
}
#[test]
fn agent_request_utf8_ref_quota_and_assignment_bounds_are_atomic() {
    let mut w = fixture();
    let reference = RevisionRef {
        id: w.evidence[0].id,
        revision: 1,
    };
    for (purpose, refs) in [
        (" ".to_owned(), vec![reference.clone()]),
        ("あ".repeat(2731), vec![reference.clone()]),
        ("ok".to_owned(), vec![]),
        ("ok".to_owned(), vec![reference.clone(); 17]),
        ("ok".to_owned(), vec![reference.clone(); 2]),
    ] {
        let c = Command::RequestAgentExecution {
            task_id: SALES_TASK_ID,
            context: context(&w),
            expected_attempt_id: SALES_ATTEMPT_ID,
            purpose,
            evidence_revision_refs: refs,
        };
        let before = w.clone();
        assert_eq!(w.apply(ACTOR, &c, NOW), Err(WorkError::ValidationFailed));
        assert_eq!(w, before);
    }
    for _ in 0..MAX_AGENT_EXECUTIONS {
        let e = request(&mut w);
        w.fail_agent_execution(ACTOR, e.id, AgentFailureCode::ProviderDenied, NOW)
            .unwrap();
    }
    let c = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "ok".into(),
        evidence_revision_refs: vec![reference],
    };
    let before = w.clone();
    assert_eq!(w.apply(ACTOR, &c, NOW), Err(WorkError::ValidationFailed));
    assert_eq!(w, before);
}
#[test]
fn context_transition_terminates_old_execution_and_new_attempt_is_not_blocked() {
    let mut w = fixture();
    let draft = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: context(&w),
        value: TextValue {
            text: "提出".into(),
        },
    };
    let artifact = match w.apply(ACTOR, &draft, NOW).unwrap() {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    let e = request(&mut w);
    let ctx = w.start_agent_execution(ACTOR, e.id, NOW).unwrap().unwrap();
    let submit = Command::Submit {
        task_id: SALES_TASK_ID,
        context: context(&w),
        artifacts: vec![ArtifactSelection {
            artifact_id: artifact.id,
            revision: artifact.revision,
        }],
        expected_attempt_id: Some(SALES_ATTEMPT_ID),
        evidence_revision_refs: vec![RevisionRef {
            id: w.evidence[0].id,
            revision: 1,
        }],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
    };
    let snapshot = match w.apply(ACTOR, &submit, NOW).unwrap() {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    };
    assert_eq!(
        w.agent_execution(ACTOR, e.id).unwrap().status,
        AgentExecutionStatus::Failed
    );
    assert!(w.finish_agent_execution(&ctx, output(), NOW).is_err());
    w.apply(
        VerifiedActor::Office01,
        &Command::Claim {
            task_id: OFFICE_TASK_ID,
            context: CommandContext {
                operation_id: Uuid::now_v7(),
                expected_revision: 0,
                acting_assignment_id: OFFICE_ASSIGNMENT_ID,
            },
        },
        NOW,
    )
    .unwrap();
    w.apply(
        VerifiedActor::Office01,
        &Command::Return {
            task_id: OFFICE_TASK_ID,
            context: CommandContext {
                operation_id: Uuid::now_v7(),
                expected_revision: 1,
                acting_assignment_id: OFFICE_ASSIGNMENT_ID,
            },
            expected_attempt_id: OFFICE_ATTEMPT_ID,
            previous_submission_id: snapshot.id,
            target_task_id: SALES_TASK_ID,
            transition_id: RETURN_TRANSITION_ID,
            reason: "再確認".into(),
        },
        NOW,
    )
    .unwrap();
    assert_eq!(
        w.agent_execution(ACTOR, e.id),
        Err(WorkError::WorkItemNotFound)
    );
    let claim = Command::Claim {
        task_id: SALES_TASK_ID,
        context: context(&w),
    };
    w.apply(ACTOR, &claim, NOW).unwrap();
    assert!(
        w.detail(ACTOR, SALES_TASK_ID)
            .unwrap()
            .task
            .can_request_agent
    );
    assert!(
        w.detail(ACTOR, SALES_TASK_ID)
            .unwrap()
            .agent_execution_ids
            .is_empty()
    );
}
#[test]
fn legacy_human_finding_json_has_no_new_origin_or_digest_field() {
    let mut w = fixture();
    let c = Command::RegisterFinding {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        claim: "人間候補".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: w.evidence[0].id,
            revision: 1,
        }],
        supersedes_finding_id: None,
    };
    let f = match w.apply(ACTOR, &c, NOW).unwrap() {
        MutationResult::FindingRegistered { finding, .. } => finding,
        _ => panic!(),
    };
    assert!(
        serde_json::to_value(f)
            .unwrap()
            .get("originExecutionId")
            .is_none()
    );
}
