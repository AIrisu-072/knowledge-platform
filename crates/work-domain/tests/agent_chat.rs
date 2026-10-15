use uuid::Uuid;
use work_domain::*;
const NOW: &str = "2026-10-07T09:00:00Z";
const ACTOR: VerifiedActor = VerifiedActor::Sales01;
fn context(w: &Workflow) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: w.source.revision,
        acting_assignment_id: ACTOR.assignment_id(),
    }
}
fn register(w: &mut Workflow, n: u128) -> RevisionRef {
    let c = Command::RegisterEvidence {
        task_id: SALES_TASK_ID,
        context: context(w),
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
                content_item_id: Uuid::from_u128(74 + n),
                representation_id: Uuid::from_u128(75),
            },
        },
        relevant_location: format!("合成根拠 {n}"),
    };
    match w.apply(ACTOR, &c, NOW).unwrap() {
        MutationResult::EvidenceRegistered { evidence, .. } => RevisionRef {
            id: evidence.id,
            revision: evidence.revision,
        },
        _ => panic!(),
    }
}
fn policy() -> OrganizationPolicy {
    let mut policy = OrganizationPolicy::synthetic();
    // A second synthetic sales holder so the source step can be reassigned.
    let mut second = policy
        .role_assignments
        .iter()
        .find(|value| value.id == SALES_ASSIGNMENT_ID)
        .unwrap()
        .clone();
    second.id = SECOND_SALES;
    second.principal = VerifiedActor::Delegate01;
    policy.role_assignments.push(second);
    policy
}
const SECOND_SALES: Uuid = Uuid::from_u128(0x0199_0000_0000_7000_8000_0000_0000_0991);
fn running(sources: usize) -> (Workflow, AgentDispatchContext, Vec<RevisionRef>) {
    let now =
        time::OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339).unwrap();
    let mut w = Workflow::synthetic(Some(Uuid::from_u128(71))).with_authority(policy(), now);
    let refs: Vec<_> = (0..sources).map(|n| register(&mut w, n as u128)).collect();
    let c = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(&w),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "根拠から確認メモの下書きを作る".into(),
        evidence_revision_refs: refs.clone(),
    };
    let MutationResult::AgentExecutionRequested { execution, .. } =
        w.apply(ACTOR, &c, NOW).unwrap()
    else {
        panic!()
    };
    let ctx = w
        .start_agent_execution(ACTOR, execution.id, NOW)
        .unwrap()
        .unwrap();
    (w, ctx, refs)
}
fn outcomes(refs: &[RevisionRef], uses: &[AgentSourceUse]) -> Vec<AgentSourceOutcome> {
    refs.iter()
        .zip(uses)
        .map(|(r, outcome)| AgentSourceOutcome {
            evidence_revision_ref: r.clone(),
            outcome: *outcome,
        })
        .collect()
}
fn structured(refs: &[RevisionRef]) -> AgentOutput {
    AgentOutput {
        summary: "合成実行：下書きと提案を作成".into(),
        uncertainty: vec!["本文分析なし".into()],
        source_outcomes: outcomes(refs, &vec![AgentSourceUse::Referenced; refs.len()]),
        finding: Some(AgentFindingCandidate {
            claim: "原本の該当箇所を人間が確認する必要がある".into(),
            evidence_revision_refs: refs.to_vec(),
        }),
        generated_artifacts: vec![GeneratedArtifactCandidate {
            title: "確認メモの下書き（合成）".into(),
            text: "【合成】確認メモの下書き".into(),
            source_revision_refs: refs.to_vec(),
        }],
        suggested_actions: vec![
            SuggestedActionCandidate {
                action: ProposedActionCandidate::ReviewFinding,
                rationale: "候補を確認して判断を記録してください".into(),
            },
            SuggestedActionCandidate {
                action: ProposedActionCandidate::UseGeneratedArtifact(0),
                rationale: "下書きを作業文案に使えます".into(),
            },
        ],
    }
}

#[test]
fn structured_result_records_private_candidates_and_typed_non_executable_suggestions() {
    let (mut w, ctx, refs) = running(1);
    let id = ctx.execution.id;
    let done = w
        .finish_agent_execution(&ctx, structured(&refs), NOW)
        .unwrap();
    assert_eq!(done.status, AgentExecutionStatus::Succeeded);
    let result = w.agent_result(ACTOR, id).unwrap();
    assert_eq!(result.finding_revision_refs.len(), 1);
    assert_eq!(result.generated_artifact_ids.len(), 1);
    assert_eq!(result.suggested_action_ids.len(), 2);
    assert_eq!(
        result.source_outcomes,
        outcomes(&refs, &[AgentSourceUse::Referenced])
    );
    assert!(result.simulated && !result.body_analyzed);
    let generated = w
        .generated_artifact(ACTOR, result.generated_artifact_ids[0])
        .unwrap();
    assert_eq!(generated.execution_id, id);
    assert_eq!(generated.schema_id, TEXT_SCHEMA_ID);
    assert_eq!(generated.value.text, "【合成】確認メモの下書き");
    assert_eq!(generated.author, SYNTHETIC_EXECUTOR);
    assert_eq!(generated.visibility, "agent_execution_private");
    assert!(generated.simulated);
    let review = w
        .suggested_action(ACTOR, result.suggested_action_ids[0])
        .unwrap();
    assert_eq!(
        review.action,
        ProposedAction::ReviewFinding {
            finding_revision_ref: result.finding_revision_refs[0].clone()
        }
    );
    assert_eq!(review.supporting_revision_refs, refs);
    let adopt = w
        .suggested_action(ACTOR, result.suggested_action_ids[1])
        .unwrap();
    assert_eq!(
        adopt.action,
        ProposedAction::UseGeneratedArtifact {
            generated_artifact_id: generated.id
        }
    );
    // Candidates are not Work artifacts and the Agent never records a Human act.
    assert!(w.artifacts.iter().all(|a| a.id != generated.id));
    assert!(w.decisions.is_empty());
    // Private to the requester under the same responsibility, like the execution.
    for other in [VerifiedActor::Office01, VerifiedActor::Approver01] {
        assert_eq!(
            w.generated_artifact(other, generated.id),
            Err(WorkError::WorkItemNotFound)
        );
        assert_eq!(
            w.suggested_action(other, review.id),
            Err(WorkError::WorkItemNotFound)
        );
    }
    w.validate_integrity().unwrap();
    // Stored JSON round-trips; legacy results without the new keys stay unchanged.
    let json = serde_json::to_value(&w).unwrap();
    let decoded = serde_json::from_value::<Workflow>(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), json);
    decoded.validate_integrity().unwrap();
    let encoded = serde_json::to_value(&result).unwrap();
    for key in [
        "generatedArtifactIds",
        "suggestedActionIds",
        "sourceOutcomes",
    ] {
        assert!(encoded.get(key).is_some(), "{key}");
    }
    let legacy = AgentResult {
        generated_artifact_ids: vec![],
        suggested_action_ids: vec![],
        source_outcomes: vec![],
        ..result.clone()
    };
    let encoded = serde_json::to_value(&legacy).unwrap();
    for key in [
        "generatedArtifactIds",
        "suggestedActionIds",
        "sourceOutcomes",
    ] {
        assert!(encoded.get(key).is_none(), "{key}");
    }
    // A stored pre-U4 workflow (no outcomes, no candidates) stays valid.
    let mut earlier = w.clone();
    earlier.generated_artifacts.clear();
    earlier.suggested_actions.clear();
    earlier.agent_executions[0].result = Some(legacy);
    earlier.validate_integrity().unwrap();
    let json = serde_json::to_value(&earlier).unwrap();
    assert!(json.get("generatedArtifacts").is_none());
    assert!(json.get("suggestedActions").is_none());
}

#[test]
fn partial_sources_are_explicit_and_candidates_cite_only_usable_sources() {
    let (w, ctx, refs) = running(2);
    let mut partial = structured(&refs[..1]);
    partial.source_outcomes = outcomes(
        &refs,
        &[AgentSourceUse::Referenced, AgentSourceUse::Unavailable],
    );
    let mut ok = w.clone();
    ok.finish_agent_execution(&ctx, partial.clone(), NOW)
        .unwrap();
    let result = ok.agent_result(ACTOR, ctx.execution.id).unwrap();
    assert_eq!(result.evidence_revision_refs, refs);
    assert_eq!(
        result.source_outcomes[1].outcome,
        AgentSourceUse::Unavailable
    );
    ok.validate_integrity().unwrap();
    let finding = ok.findings.last().unwrap();
    assert_eq!(finding.evidence_revision_refs, refs[..1].to_vec());

    let rejects = |output: AgentOutput| {
        let mut attempt = w.clone();
        let before = attempt.clone();
        assert!(
            attempt.finish_agent_execution(&ctx, output, NOW).is_err(),
            "accepted invalid output"
        );
        assert_eq!(attempt, before);
    };
    // Citing an unavailable source.
    let mut bad = partial.clone();
    bad.finding.as_mut().unwrap().evidence_revision_refs = refs.clone();
    rejects(bad);
    let mut bad = partial.clone();
    bad.generated_artifacts[0].source_revision_refs = vec![refs[1].clone()];
    rejects(bad);
    // Outcomes must cover the selection exactly and in order.
    let mut bad = partial.clone();
    bad.source_outcomes.pop();
    rejects(bad);
    let mut bad = partial.clone();
    bad.source_outcomes.reverse();
    rejects(bad);
    // Nothing usable is not a result.
    let mut bad = structured(&[]);
    bad.source_outcomes = outcomes(
        &refs,
        &[AgentSourceUse::Unavailable, AgentSourceUse::Unsupported],
    );
    bad.finding = None;
    bad.generated_artifacts.clear();
    bad.suggested_actions.clear();
    rejects(bad);
    // The simulated executor cannot claim body analysis.
    let mut bad = structured(&refs);
    bad.source_outcomes[0].outcome = AgentSourceUse::Analyzed;
    rejects(bad);
    // Bounds and the closed proposal vocabulary.
    let mut bad = structured(&refs);
    bad.uncertainty.clear();
    rejects(bad);
    let mut bad = structured(&refs);
    bad.generated_artifacts = vec![bad.generated_artifacts[0].clone(); 3];
    rejects(bad);
    let mut bad = structured(&refs);
    bad.generated_artifacts[0].title = "改行\nを含む".into();
    rejects(bad);
    let mut bad = structured(&refs);
    bad.generated_artifacts[0].title = "あ".repeat(67);
    rejects(bad);
    let mut bad = structured(&refs);
    bad.generated_artifacts[0].text = "x".repeat(8193);
    rejects(bad);
    let mut bad = structured(&refs);
    bad.suggested_actions[1].action = ProposedActionCandidate::UseGeneratedArtifact(1);
    rejects(bad);
    let mut bad = structured(&refs);
    bad.finding = None;
    rejects(bad);
    let mut bad = structured(&refs);
    bad.suggested_actions = vec![bad.suggested_actions[0].clone(); 5];
    rejects(bad);
    let mut bad = structured(&refs);
    bad.suggested_actions[0].rationale = "x".repeat(1025);
    rejects(bad);
    let mut bad = structured(&refs);
    bad.suggested_actions[1] = bad.suggested_actions[0].clone();
    rejects(bad);
    // A result without a Finding is allowed: a bare suggestion stays a suggestion.
    let mut bare = structured(&refs);
    bare.finding = None;
    bare.suggested_actions.remove(0);
    let mut ok = w.clone();
    ok.finish_agent_execution(&ctx, bare, NOW).unwrap();
    let result = ok.agent_result(ACTOR, ctx.execution.id).unwrap();
    assert!(result.finding_revision_refs.is_empty());
    assert!(ok.findings.is_empty());
    ok.validate_integrity().unwrap();
}

#[test]
fn candidates_follow_the_execution_read_rule_across_reassignment_and_new_attempts() {
    let (mut w, ctx, refs) = running(1);
    w.finish_agent_execution(&ctx, structured(&refs), NOW)
        .unwrap();
    let result = w.agent_result(ACTOR, ctx.execution.id).unwrap();
    let generated = result.generated_artifact_ids[0];
    let suggestion = result.suggested_action_ids[0];
    assert!(w.generated_artifact(ACTOR, generated).is_ok());
    // A stored record whose execution is no longer the requester's is hidden,
    // and integrity rejects candidates that no succeeded result lists.
    let mut tampered = w.clone();
    tampered
        .agent_executions
        .iter_mut()
        .for_each(|e| e.result.as_mut().unwrap().generated_artifact_ids.clear());
    assert_eq!(
        tampered.validate_integrity(),
        Err(WorkError::IntegrityViolation)
    );
    // A candidate record that no result lists, even with nothing referring to it.
    let mut tampered = w.clone();
    let mut stray = tampered.generated_artifacts[0].clone();
    stray.id = Uuid::now_v7();
    tampered.generated_artifacts.push(stray);
    assert_eq!(
        tampered.validate_integrity(),
        Err(WorkError::IntegrityViolation)
    );
    let mut tampered = w.clone();
    tampered.suggested_actions[1].action = ProposedAction::UseGeneratedArtifact {
        generated_artifact_id: Uuid::now_v7(),
    };
    assert_eq!(
        tampered.validate_integrity(),
        Err(WorkError::IntegrityViolation)
    );
    // Reassignment removes the previous requester's view of the candidates,
    // and the new assignee never inherits another principal's Agent records.
    w.apply(
        VerifiedActor::Approver01,
        &Command::Assign {
            task_id: SALES_TASK_ID,
            context: CommandContext {
                operation_id: Uuid::now_v7(),
                expected_revision: w.source.revision,
                acting_assignment_id: APPROVER_MANAGEMENT_ASSIGNMENT_ID,
            },
            expected_attempt_id: SALES_ATTEMPT_ID,
            assignee: VerifiedActor::Delegate01,
            assignee_responsibility_id: SECOND_SALES,
            reason: "合成の担当変更".into(),
        },
        NOW,
    )
    .unwrap();
    for actor in [ACTOR, VerifiedActor::Delegate01] {
        assert_eq!(
            w.generated_artifact(actor, generated),
            Err(WorkError::WorkItemNotFound)
        );
        assert_eq!(
            w.suggested_action(actor, suggestion),
            Err(WorkError::WorkItemNotFound)
        );
    }
    w.validate_integrity().unwrap();
    assert_eq!(
        w.generated_artifact(ACTOR, Uuid::now_v7()),
        Err(WorkError::WorkItemNotFound)
    );
}

#[test]
fn reviewer_candidates_stay_with_the_completed_attempt_after_a_return() {
    let (mut w, sales_ctx, refs) = running(1);
    w.finish_agent_execution(&sales_ctx, structured(&refs), NOW)
        .unwrap();
    let MutationResult::DraftSaved { artifact, .. } = w
        .apply(
            ACTOR,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: context(&w),
                value: TextValue {
                    text: "提出".into(),
                },
            },
            NOW,
        )
        .unwrap()
    else {
        panic!()
    };
    let MutationResult::Submitted { snapshot, .. } = w
        .apply(
            ACTOR,
            &Command::Submit {
                task_id: SALES_TASK_ID,
                context: context(&w),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: artifact.revision,
                }],
                expected_attempt_id: Some(SALES_ATTEMPT_ID),
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
            },
            NOW,
        )
        .unwrap()
    else {
        panic!()
    };
    let office = VerifiedActor::Office01;
    let office_ctx = |w: &Workflow| CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: w.next.as_ref().unwrap().revision,
        acting_assignment_id: OFFICE_ASSIGNMENT_ID,
    };
    w.apply(
        office,
        &Command::Claim {
            task_id: OFFICE_TASK_ID,
            context: office_ctx(&w),
        },
        NOW,
    )
    .unwrap();
    let MutationResult::EvidenceRegistered { evidence, .. } = w
        .apply(
            office,
            &Command::RegisterEvidence {
                task_id: OFFICE_TASK_ID,
                context: office_ctx(&w),
                expected_attempt_id: OFFICE_ATTEMPT_ID,
                source: w.evidence[0].source.clone(),
                relevant_location: "事務の根拠".into(),
            },
            NOW,
        )
        .unwrap()
    else {
        panic!()
    };
    let office_refs = vec![RevisionRef {
        id: evidence.id,
        revision: 1,
    }];
    let MutationResult::AgentExecutionRequested { execution, .. } = w
        .apply(
            office,
            &Command::RequestAgentExecution {
                task_id: OFFICE_TASK_ID,
                context: office_ctx(&w),
                expected_attempt_id: OFFICE_ATTEMPT_ID,
                purpose: "事務の確認".into(),
                evidence_revision_refs: office_refs.clone(),
            },
            NOW,
        )
        .unwrap()
    else {
        panic!()
    };
    let ctx = w
        .start_agent_execution(office, execution.id, NOW)
        .unwrap()
        .unwrap();
    w.finish_agent_execution(&ctx, structured(&office_refs), NOW)
        .unwrap();
    let generated = w
        .agent_result(office, execution.id)
        .unwrap()
        .generated_artifact_ids[0];
    w.apply(
        office,
        &Command::Return {
            task_id: OFFICE_TASK_ID,
            context: office_ctx(&w),
            expected_attempt_id: OFFICE_ATTEMPT_ID,
            previous_submission_id: snapshot.id,
            target_task_id: SALES_TASK_ID,
            transition_id: RETURN_TRANSITION_ID,
            reason: "再確認".into(),
        },
        NOW,
    )
    .unwrap();
    // The completed office attempt keeps its own records, like its execution.
    assert!(w.agent_execution(office, execution.id).is_ok());
    assert!(w.generated_artifact(office, generated).is_ok());
    // The returned sales attempt starts a new, empty chat; old records never attach.
    assert!(
        w.generated_artifact(ACTOR, w.generated_artifacts[0].id)
            .is_err()
    );
    w.validate_integrity().unwrap();
}
