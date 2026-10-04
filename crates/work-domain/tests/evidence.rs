use serde_json::{Value, json};
use uuid::Uuid;
use work_domain::*;
const NOW: &str = "2026-10-04T08:00:00Z";
const DOC: Uuid = Uuid::from_u128(71);
fn command(w: &Workflow, actor: VerifiedActor, kind: &str, extra: Value) -> Command {
    let item = if actor == VerifiedActor::Sales01 {
        &w.source
    } else {
        w.next.as_ref().unwrap()
    };
    let mut value = json!({"kind":kind,"task_id":item.id,"expected_attempt_id":item.attempt_id,"context":{"operationId":Uuid::now_v7(),"expectedRevision":item.revision,"actingAssignmentId":actor.assignment_id()}});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value)
        .expect("evidence command is part of the closed Work command union")
}
fn evidence(w: &mut Workflow) -> Value {
    let c = command(
        w,
        VerifiedActor::Sales01,
        "register_evidence",
        json!({"source":{"sourceRef":{"providerId":"document","resourceId":DOC,"revisionId":Uuid::from_u128(72),"versionId":Uuid::from_u128(73)},"authoritativeLocator":{"kind":"contentItem","contentItemId":Uuid::from_u128(74),"representationId":Uuid::from_u128(75)}},"relevant_location":"選択した原本の該当箇所"}),
    );
    serde_json::to_value(w.apply(VerifiedActor::Sales01, &c, NOW).unwrap()).unwrap()
}
fn reference(record: &Value) -> Value {
    json!({"id":record["id"],"revision":record["revision"]})
}
fn finding(w: &mut Workflow, ev: &Value) -> Value {
    let c = command(
        w,
        VerifiedActor::Sales01,
        "register_finding",
        json!({"claim":"根拠に基づく候補","evidence_revision_refs":[reference(ev)]}),
    );
    serde_json::to_value(w.apply(VerifiedActor::Sales01, &c, NOW).unwrap()).unwrap()["finding"]
        .clone()
}
#[test]
fn evidence_is_reference_only_human_stamped_and_private() {
    let mut w = Workflow::synthetic(Some(DOC));
    let result = evidence(&mut w);
    let e = &result["evidence"];
    assert_eq!(e["revision"], 1);
    assert_eq!(e["uncertainty"], json!([]));
    assert_eq!(e["conflictReferences"], json!([]));
    assert_eq!(e["relevantLocationVerified"], false);
    assert_eq!(e["policyDisposition"], "reference_only");
    assert_eq!(e["origin"], "human");
    assert_eq!(e["coverage"], "unknown");
    assert_eq!(e["fragmentOmissionReason"], "not_retained");
    assert_eq!(e["createdBy"], "sales-01");
    assert_eq!(e["attemptId"], SALES_ATTEMPT_ID.to_string());
    assert_eq!(e["contextId"], CONTEXT_ID.to_string());
    assert_eq!(result["task"]["canRegisterEvidence"], true);
    assert_eq!(result["task"]["revision"], 1);
    assert!(
        w.authorize_recovery(
            VerifiedActor::Office01,
            &serde_json::from_value(result).unwrap()
        )
        .is_err()
    );
}
#[test]
fn finding_requires_real_exact_support_and_modified_decision_requires_adopted_claim() {
    let mut w = Workflow::synthetic(Some(DOC));
    let ev = evidence(&mut w)["evidence"].clone();
    for refs in [
        json!([]),
        json!([{"id":Uuid::now_v7(),"revision":1}]),
        json!([reference(&ev), reference(&ev)]),
    ] {
        let c = command(
            &w,
            VerifiedActor::Sales01,
            "register_finding",
            json!({"claim":"candidate","evidence_revision_refs":refs}),
        );
        let before = w.clone();
        assert!(w.apply(VerifiedActor::Sales01, &c, NOW).is_err());
        assert_eq!(w, before);
    }
    let f = finding(&mut w, &ev);
    let c = command(
        &w,
        VerifiedActor::Sales01,
        "record_decision",
        json!({"finding_id":f["id"],"finding_revision":1,"decision":"modified","evidence_revision_refs":[reference(&ev)]}),
    );
    let before = w.clone();
    assert_eq!(
        w.apply(VerifiedActor::Sales01, &c, NOW),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(w, before);
    let mut c = serde_json::to_value(c).unwrap();
    c["adopted_claim"] = json!("人間が修正した採用文");
    let d = w
        .apply(
            VerifiedActor::Sales01,
            &serde_json::from_value(c).unwrap(),
            NOW,
        )
        .unwrap();
    let d = serde_json::to_value(d).unwrap();
    assert_eq!(d["decision"]["humanPrincipal"], "sales-01");
    assert_eq!(d["decision"]["findingId"], f["id"]);
    assert_eq!(serde_json::to_value(&w).unwrap()["findings"][0], f);
}
fn submit_selected(w: &mut Workflow, ev: &Value, f: &Value, d: Option<&Value>) -> HandoffSnapshot {
    let save = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: CommandContext {
            operation_id: Uuid::now_v7(),
            expected_revision: w.source.revision,
            acting_assignment_id: SALES_ASSIGNMENT_ID,
        },
        value: TextValue {
            text: "提出文案".into(),
        },
    };
    let a = match w.apply(VerifiedActor::Sales01, &save, NOW).unwrap() {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    let c = command(
        w,
        VerifiedActor::Sales01,
        "submit",
        json!({"artifacts":[{"artifactId":a.id,"revision":a.revision}],"evidence_revision_refs":[reference(ev)],"finding_revision_refs":[reference(f)],"decision_revision_refs":d.map(|d|vec![reference(d)]).unwrap_or_default()}),
    );
    match w.apply(VerifiedActor::Sales01, &c, NOW).unwrap() {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    }
}
fn claim(w: &mut Workflow, actor: VerifiedActor) {
    let item = if actor == VerifiedActor::Sales01 {
        &w.source
    } else {
        w.next.as_ref().unwrap()
    };
    let c = Command::Claim {
        task_id: item.id,
        context: CommandContext {
            operation_id: Uuid::now_v7(),
            expected_revision: item.revision,
            acting_assignment_id: actor.assignment_id(),
        },
    };
    w.apply(actor, &c, NOW).unwrap();
}
#[test]
fn selected_snapshot_closure_and_current_attempt_read_scope_preserve_private_records() {
    let mut w = Workflow::synthetic(Some(DOC));
    let ev = evidence(&mut w)["evidence"].clone();
    let private = evidence(&mut w)["evidence"].clone();
    let f = finding(&mut w, &ev);
    let decision = command(
        &w,
        VerifiedActor::Sales01,
        "record_decision",
        json!({"finding_id":f["id"],"finding_revision":1,"decision":"accepted","evidence_revision_refs":[]}),
    );
    let d = serde_json::to_value(w.apply(VerifiedActor::Sales01, &decision, NOW).unwrap()).unwrap()
        ["decision"]
        .clone();
    let parse = |v: &Value| serde_json::from_value(v.clone()).unwrap();
    assert_eq!(
        w.validate_selection(
            VerifiedActor::Sales01,
            SALES_TASK_ID,
            &[],
            &[parse(&reference(&f))],
            &[]
        ),
        Err(WorkError::HandoffNotReady)
    );
    assert_eq!(
        w.validate_selection(
            VerifiedActor::Sales01,
            SALES_TASK_ID,
            &[parse(&reference(&ev))],
            &[],
            &[parse(&reference(&d))]
        ),
        Err(WorkError::HandoffNotReady)
    );
    let snapshot = submit_selected(&mut w, &ev, &f, Some(&d));
    assert_eq!(snapshot.evidence_revision_refs.len(), 1);
    let eid: Uuid = serde_json::from_value(ev["id"].clone()).unwrap();
    let pid: Uuid = serde_json::from_value(private["id"].clone()).unwrap();
    let fid: Uuid = serde_json::from_value(f["id"].clone()).unwrap();
    assert!(w.evidence_record(VerifiedActor::Office01, eid).is_err());
    claim(&mut w, VerifiedActor::Office01);
    let office = w.detail(VerifiedActor::Office01, OFFICE_TASK_ID).unwrap();
    assert!(!office.task.can_edit);
    assert!(
        office.task.can_register_evidence
            && office.task.can_register_finding
            && office.task.can_record_decision
    );
    assert_eq!(
        w.list_evidence(VerifiedActor::Office01, OFFICE_TASK_ID)
            .unwrap()
            .len(),
        1
    );
    assert!(w.evidence_record(VerifiedActor::Office01, pid).is_err());
    let c = command(
        &w,
        VerifiedActor::Office01,
        "record_decision",
        json!({"finding_id":fid,"finding_revision":1,"decision":"rejected","reason":"照合が必要","evidence_revision_refs":[]}),
    );
    let office_decision = w.apply(VerifiedActor::Office01, &c, NOW).unwrap();
    let returned = command(
        &w,
        VerifiedActor::Office01,
        "return",
        json!({"previous_submission_id":snapshot.id,"target_task_id":SALES_TASK_ID,"transition_id":RETURN_TRANSITION_ID,"reason":"再確認"}),
    );
    w.apply(VerifiedActor::Office01, &returned, NOW).unwrap();
    assert!(w.evidence_record(VerifiedActor::Sales01, pid).is_err());
    assert!(
        w.authorize_recovery(VerifiedActor::Office01, &office_decision)
            .is_ok(),
        "completed current attempt retains readonly owner authority"
    );
    claim(&mut w, VerifiedActor::Sales01);
    assert_eq!(
        w.list_evidence(VerifiedActor::Sales01, SALES_TASK_ID)
            .unwrap()
            .len(),
        1
    );
    assert!(w.evidence_record(VerifiedActor::Sales01, pid).is_err());
    assert_eq!(
        w.snapshot(VerifiedActor::Office01, snapshot.id).unwrap(),
        snapshot
    );
    let second = submit_selected(&mut w, &ev, &f, Some(&d));
    assert_ne!(second.id, snapshot.id);
    assert!(
        w.authorize_recovery(VerifiedActor::Office01, &office_decision)
            .is_err(),
        "replaced office attempt private decision is hidden"
    );
    assert_eq!(
        w.evidence_record(VerifiedActor::Office01, eid).unwrap().id,
        eid,
        "historical explicitly selected membership remains authorized"
    );
}
#[test]
fn decision_supersedes_preserves_both_and_rejects_utf8_stale_and_cross_scope() {
    let mut w = Workflow::synthetic(Some(DOC));
    let e = evidence(&mut w)["evidence"].clone();
    let f = finding(&mut w, &e);
    let c = command(
        &w,
        VerifiedActor::Sales01,
        "record_decision",
        json!({"finding_id":f["id"],"finding_revision":1,"decision":"accepted","evidence_revision_refs":[]}),
    );
    let first = w.apply(VerifiedActor::Sales01, &c, NOW).unwrap();
    let first_json = serde_json::to_value(&first).unwrap();
    let mut c = command(
        &w,
        VerifiedActor::Sales01,
        "record_decision",
        json!({"finding_id":f["id"],"finding_revision":1,"decision":"modified","adopted_claim":"修正文","reason":"再確認","supersedes_decision_id":first_json["decision"]["id"],"evidence_revision_refs":[reference(&e)]}),
    );
    for (field, bad) in [
        ("adopted_claim", json!("あ".repeat(2731))),
        ("reason", json!("a".repeat(8193))),
        ("expected_attempt_id", json!(Uuid::now_v7())),
        ("finding_revision", json!(2)),
    ] {
        let mut v = serde_json::to_value(&c).unwrap();
        v[field] = bad;
        let before = w.clone();
        assert!(
            w.apply(
                VerifiedActor::Sales01,
                &serde_json::from_value(v).unwrap(),
                NOW
            )
            .is_err()
        );
        assert_eq!(w, before);
    }
    w.apply(VerifiedActor::Sales01, &c, NOW).unwrap();
    assert_eq!(
        serde_json::to_value(&w.decisions[0]).unwrap(),
        first_json["decision"]
    );
    assert_eq!(w.decisions.len(), 2);
    if let Command::RecordDecision { context, .. } = &mut c {
        context.expected_revision = w.source.revision;
    }
    let mut corrupt = w.clone();
    corrupt.evidence[0].context_id = Uuid::now_v7();
    assert!(corrupt.apply(VerifiedActor::Sales01, &c, NOW).is_err());
}
#[test]
fn registration_caps_the_complete_current_and_received_collection() {
    let mut w = Workflow::synthetic(Some(DOC));
    let e = evidence(&mut w)["evidence"].clone();
    let f = finding(&mut w, &e);
    let snapshot = submit_selected(&mut w, &e, &f, None);
    claim(&mut w, VerifiedActor::Office01);
    let payload = json!({"source":{"sourceRef":e["sourceRef"],"authoritativeLocator":e["authoritativeLocator"]},"relevant_location":"別の人間記載箇所"});
    for _ in 0..15 {
        let c = command(
            &w,
            VerifiedActor::Office01,
            "register_evidence",
            payload.clone(),
        );
        w.apply(VerifiedActor::Office01, &c, NOW).unwrap();
    }
    assert_eq!(
        w.list_evidence(VerifiedActor::Office01, OFFICE_TASK_ID)
            .unwrap()
            .len(),
        16
    );
    let before = w.clone();
    let c = command(&w, VerifiedActor::Office01, "register_evidence", payload);
    assert_eq!(
        w.apply(VerifiedActor::Office01, &c, NOW),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(w, before);
    for _ in 0..15 {
        let c = command(
            &w,
            VerifiedActor::Office01,
            "register_finding",
            json!({"claim":"確認候補","evidence_revision_refs":[reference(&e)]}),
        );
        w.apply(VerifiedActor::Office01, &c, NOW).unwrap();
    }
    assert_eq!(
        w.list_findings(VerifiedActor::Office01, OFFICE_TASK_ID)
            .unwrap()
            .len(),
        16
    );
    let before = w.clone();
    let c = command(
        &w,
        VerifiedActor::Office01,
        "register_finding",
        json!({"claim":"追加候補","evidence_revision_refs":[reference(&e)]}),
    );
    assert_eq!(
        w.apply(VerifiedActor::Office01, &c, NOW),
        Err(WorkError::ValidationFailed)
    );
    assert_eq!(w, before);
    assert_eq!(
        w.snapshot(VerifiedActor::Office01, snapshot.id).unwrap(),
        snapshot
    );
}
