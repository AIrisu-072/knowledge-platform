use super::*;
use std::collections::BTreeSet;
pub const MAX_REFERENCES: usize = 100;
/// Narrow Browser PoC complete-collection profile; not a general paginated API.
pub const MAX_VISIBLE_RECORDS: usize = 16;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionRef {
    pub id: Uuid,
    pub revision: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRef {
    pub provider_id: String,
    pub resource_id: Uuid,
    pub revision_id: Uuid,
    pub version_id: Uuid,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoritativeLocator {
    pub kind: String,
    pub content_item_id: Uuid,
    pub representation_id: Uuid,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceSource {
    pub source_ref: SourceRef,
    pub authoritative_locator: AuthoritativeLocator,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRecord {
    pub id: Uuid,
    pub revision: i64,
    pub context_id: Uuid,
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub created_by: String,
    pub acting_assignment_id: Uuid,
    pub origin: String,
    #[serde(flatten)]
    pub source: EvidenceSource,
    pub uncertainty: Vec<String>,
    pub conflict_references: Vec<RevisionRef>,
    pub relevant_location_verified: bool,
    pub policy_disposition: String,
    pub relevant_location: String,
    pub fragment_omission_reason: String,
    pub coverage: String,
    pub retrieved_at: String,
    pub recorded_at: String,
    pub provider_checked_at: String,
    pub visibility: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_execution_id: Option<Uuid>,
    pub id: Uuid,
    pub revision: i64,
    pub context_id: Uuid,
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub uncertainty: Vec<String>,
    pub conflicts: Vec<RevisionRef>,
    pub author: String,
    pub acting_assignment_id: Uuid,
    pub claim: String,
    pub evidence_revision_refs: Vec<RevisionRef>,
    pub supersedes_finding_id: Option<Uuid>,
    pub visibility: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Accepted,
    Modified,
    Rejected,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanDecision {
    pub id: Uuid,
    pub revision: i64,
    pub context_id: Uuid,
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub finding_id: Uuid,
    pub finding_revision: i64,
    pub decision: DecisionKind,
    pub adopted_claim: Option<String>,
    pub reason: Option<String>,
    pub evidence_revision_refs: Vec<RevisionRef>,
    pub human_principal: String,
    pub acting_assignment_id: Uuid,
    pub created_at: String,
    pub supersedes_decision_id: Option<Uuid>,
    pub visibility: String,
}
pub(super) fn bounded_text(value: &str) -> Result<(), WorkError> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(WorkError::ValidationFailed);
    }
    Ok(())
}
pub fn validate_refs(refs: &[RevisionRef]) -> Result<(), WorkError> {
    let mut seen = BTreeSet::new();
    if refs.len() > MAX_REFERENCES || refs.iter().any(|r| r.revision != 1 || !seen.insert(r.id)) {
        return Err(WorkError::ValidationFailed);
    }
    Ok(())
}
pub(super) struct FindingInput<'a> {
    pub claim: &'a str,
    pub evidence_revision_refs: &'a [RevisionRef],
    pub supersedes_finding_id: Option<Uuid>,
}
impl Workflow {
    fn record_visible(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
        attempt_id: Uuid,
        reference: &RevisionRef,
        member: fn(&HandoffSnapshot) -> &[RevisionRef],
    ) -> bool {
        if self
            .item(task_id)
            .is_ok_and(|item| item.attempt_id == attempt_id && self.can_read(actor, item))
        {
            return true;
        }
        // Immutable explicitly submitted membership remains readable only under
        // current snapshot authority. New recipients still must claim first;
        // unrelated old private records never gain this path.
        self.snapshots
            .iter()
            .any(|s| member(s).contains(reference) && self.snapshot(actor, s.id).is_ok())
    }
    pub fn evidence_record(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<EvidenceRecord, WorkError> {
        let e = self
            .evidence
            .iter()
            .find(|e| e.id == id)
            .ok_or(WorkError::EvidenceNotFound)?;
        if e.context_id != self.context_id
            || !self.record_visible(
                actor,
                e.task_id,
                e.attempt_id,
                &RevisionRef {
                    id: e.id,
                    revision: e.revision,
                },
                |s| &s.evidence_revision_refs,
            )
        {
            return Err(WorkError::EvidenceNotFound);
        }
        Ok(e.clone())
    }
    pub fn finding(&self, actor: VerifiedActor, id: Uuid) -> Result<Finding, WorkError> {
        let f = self
            .findings
            .iter()
            .find(|f| f.id == id)
            .ok_or(WorkError::FindingNotFound)?;
        if f.context_id != self.context_id
            || !self.record_visible(
                actor,
                f.task_id,
                f.attempt_id,
                &RevisionRef {
                    id: f.id,
                    revision: f.revision,
                },
                |s| &s.finding_revision_refs,
            )
        {
            return Err(WorkError::FindingNotFound);
        }
        self.resolve_evidence(actor, &f.evidence_revision_refs)?;
        Ok(f.clone())
    }
    pub fn decision(&self, actor: VerifiedActor, id: Uuid) -> Result<HumanDecision, WorkError> {
        let d = self
            .decisions
            .iter()
            .find(|d| d.id == id)
            .ok_or(WorkError::FindingNotFound)?;
        if d.context_id != self.context_id
            || !self.record_visible(
                actor,
                d.task_id,
                d.attempt_id,
                &RevisionRef {
                    id: d.id,
                    revision: d.revision,
                },
                |s| &s.decision_revision_refs,
            )
        {
            return Err(WorkError::FindingNotFound);
        }
        self.finding(actor, d.finding_id)?;
        self.resolve_evidence(actor, &d.evidence_revision_refs)?;
        Ok(d.clone())
    }
    pub fn list_evidence(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
    ) -> Result<Vec<EvidenceRecord>, WorkError> {
        self.detail(actor, task_id)?;
        let item = self.item(task_id)?;
        self.evidence
            .iter()
            .filter(|e| {
                self.record_in_task(
                    item,
                    e.task_id,
                    e.attempt_id,
                    &RevisionRef {
                        id: e.id,
                        revision: e.revision,
                    },
                    |s| &s.evidence_revision_refs,
                )
            })
            .map(|e| self.evidence_record(actor, e.id))
            .collect::<Result<_, _>>()
    }
    pub fn list_findings(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
    ) -> Result<Vec<Finding>, WorkError> {
        self.detail(actor, task_id)?;
        let item = self.item(task_id)?;
        self.findings
            .iter()
            .filter(|f| {
                self.record_in_task(
                    item,
                    f.task_id,
                    f.attempt_id,
                    &RevisionRef {
                        id: f.id,
                        revision: f.revision,
                    },
                    |s| &s.finding_revision_refs,
                )
            })
            .map(|f| self.finding(actor, f.id))
            .collect()
    }
    fn record_in_task(
        &self,
        item: &WorkItem,
        task: Uuid,
        attempt: Uuid,
        r: &RevisionRef,
        member: fn(&HandoffSnapshot) -> &[RevisionRef],
    ) -> bool {
        (item.id == task && item.attempt_id == attempt)
            || item.handoff_snapshot_id.is_some_and(|id| {
                self.snapshots
                    .iter()
                    .any(|s| s.id == id && member(s).contains(r))
            })
    }
    pub fn list_decisions(
        &self,
        actor: VerifiedActor,
        finding_id: Uuid,
    ) -> Result<Vec<HumanDecision>, WorkError> {
        self.finding(actor, finding_id)?;
        Ok(self
            .decisions
            .iter()
            .filter(|d| d.finding_id == finding_id)
            .filter_map(|d| self.decision(actor, d.id).ok())
            .collect())
    }
    pub fn resolve_evidence(
        &self,
        actor: VerifiedActor,
        refs: &[RevisionRef],
    ) -> Result<Vec<EvidenceRecord>, WorkError> {
        validate_refs(refs)?;
        refs.iter()
            .map(|r| {
                let e = self.evidence_record(actor, r.id)?;
                if e.revision != r.revision {
                    return Err(WorkError::EvidenceNotFound);
                }
                Ok(e)
            })
            .collect()
    }
    pub fn snapshot_evidence(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<Vec<EvidenceRecord>, WorkError> {
        let s = self.snapshot(actor, id)?;
        // Historical snapshot scope is explicitly authorized; do not grant direct record access.
        s.evidence_revision_refs
            .iter()
            .map(|r| {
                self.evidence
                    .iter()
                    .find(|e| {
                        e.id == r.id && e.revision == r.revision && e.context_id == s.context_id
                    })
                    .cloned()
                    .ok_or(WorkError::IntegrityViolation)
            })
            .collect()
    }
    pub fn result_evidence(
        &self,
        actor: VerifiedActor,
        result: &MutationResult,
    ) -> Result<Vec<EvidenceRecord>, WorkError> {
        self.authorize_recovery(actor, result)?;
        match result {
            MutationResult::AgentExecutionRequested { execution, .. }
            | MutationResult::AgentExecutionCancelled { execution, .. } => {
                self.resolve_evidence(actor, &execution.evidence_revision_refs)
            }
            MutationResult::EvidenceRegistered { evidence, .. } => {
                Ok(vec![self.evidence_record(actor, evidence.id)?])
            }
            MutationResult::FindingRegistered { finding, .. } => {
                self.resolve_evidence(actor, &finding.evidence_revision_refs)
            }
            MutationResult::DecisionRecorded { decision, .. } => {
                let mut refs = self
                    .finding(actor, decision.finding_id)?
                    .evidence_revision_refs;
                for r in &decision.evidence_revision_refs {
                    if !refs.contains(r) {
                        refs.push(r.clone());
                    }
                }
                self.resolve_evidence(actor, &refs)
            }
            MutationResult::Submitted { snapshot, .. } => {
                self.snapshot_evidence(actor, snapshot.id)
            }
            _ => Ok(vec![]),
        }
    }
    pub fn validate_selection(
        &self,
        actor: VerifiedActor,
        task: Uuid,
        ev: &[RevisionRef],
        findings: &[RevisionRef],
        decisions: &[RevisionRef],
    ) -> Result<(), WorkError> {
        validate_refs(ev)?;
        validate_refs(findings)?;
        validate_refs(decisions)?;
        if ev.len() + findings.len() + decisions.len() > MAX_REFERENCES {
            return Err(WorkError::ValidationFailed);
        }
        let item = self.item(task)?;
        self.resolve_evidence(actor, ev)?;
        for r in ev {
            let e = self.evidence_record(actor, r.id)?;
            if !self.record_in_task(item, e.task_id, e.attempt_id, r, |s| {
                &s.evidence_revision_refs
            }) {
                return Err(WorkError::EvidenceNotFound);
            }
        }
        for r in findings {
            let f = self.finding(actor, r.id)?;
            if f.revision != r.revision
                || !self.record_in_task(item, f.task_id, f.attempt_id, r, |s| {
                    &s.finding_revision_refs
                })
            {
                return Err(WorkError::FindingNotFound);
            }
            if f.evidence_revision_refs.iter().any(|e| !ev.contains(e)) {
                return Err(WorkError::HandoffNotReady);
            }
        }
        for r in decisions {
            let d = self.decision(actor, r.id)?;
            if d.revision != r.revision
                || !self.record_in_task(item, d.task_id, d.attempt_id, r, |s| {
                    &s.decision_revision_refs
                })
            {
                return Err(WorkError::FindingNotFound);
            }
            if !findings.contains(&RevisionRef {
                id: d.finding_id,
                revision: d.finding_revision,
            }) || d.evidence_revision_refs.iter().any(|e| !ev.contains(e))
            {
                return Err(WorkError::HandoffNotReady);
            }
        }
        Ok(())
    }
    pub(super) fn next_record_revision(&mut self, task: Uuid) -> Result<(), WorkError> {
        let item = if task == self.source.id {
            &mut self.source
        } else {
            self.next.as_mut().ok_or(WorkError::WorkItemNotFound)?
        };
        item.revision = item
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        Ok(())
    }
    pub(super) fn create_finding(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
        input: FindingInput<'_>,
        origin_execution_id: Option<Uuid>,
        now: &str,
    ) -> Result<Finding, WorkError> {
        let FindingInput {
            claim,
            evidence_revision_refs,
            supersedes_finding_id,
        } = input;
        let item = self.item(task_id)?;
        bounded_text(claim)?;
        if evidence_revision_refs.is_empty() {
            return Err(WorkError::ValidationFailed);
        }
        self.resolve_evidence(actor, evidence_revision_refs)?;
        for r in evidence_revision_refs {
            let e = self.evidence_record(actor, r.id)?;
            if !self.record_in_task(item, e.task_id, e.attempt_id, r, |s| {
                &s.evidence_revision_refs
            }) {
                return Err(WorkError::EvidenceNotFound);
            }
        }
        if let Some(id) = supersedes_finding_id {
            let f = self.finding(actor, id)?;
            if f.task_id != task_id || f.attempt_id != item.attempt_id {
                return Err(WorkError::FindingNotFound);
            }
        }
        if self.list_findings(actor, task_id)?.len() >= MAX_VISIBLE_RECORDS {
            return Err(WorkError::ValidationFailed);
        }
        Ok(Finding {
            uncertainty: vec![],
            conflicts: vec![],
            id: Uuid::now_v7(),
            revision: 1,
            context_id: self.context_id,
            task_id,
            attempt_id: item.attempt_id,
            author: if origin_execution_id.is_some() {
                SYNTHETIC_EXECUTOR.into()
            } else {
                actor.principal_id().into()
            },
            origin_execution_id,
            // The attempt's recorded responsibility; Human commands are bound to it.
            acting_assignment_id: item
                .acting_assignment_id
                .ok_or(WorkError::IntegrityViolation)?,
            claim: claim.to_owned(),
            evidence_revision_refs: evidence_revision_refs.to_vec(),
            supersedes_finding_id,
            visibility: "work_item_private".into(),
            created_at: now.into(),
        })
    }
    pub(super) fn apply_evidence(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        let task_id = command.task_id();
        let item = self.item(task_id)?.clone();
        if item.state != TaskState::Active {
            return Err(WorkError::HandoffNotReady);
        }
        let expected = match command {
            Command::RegisterEvidence {
                expected_attempt_id,
                ..
            }
            | Command::RegisterFinding {
                expected_attempt_id,
                ..
            }
            | Command::RecordDecision {
                expected_attempt_id,
                ..
            } => *expected_attempt_id,
            _ => return Err(WorkError::IntegrityViolation),
        };
        if expected != item.attempt_id {
            return Err(WorkError::RevisionConflict);
        }
        match command {
            Command::RegisterEvidence {
                source,
                relevant_location,
                ..
            } => {
                bounded_text(relevant_location)?;
                if source.source_ref.provider_id != "document"
                    || source.authoritative_locator.kind != "contentItem"
                    || !self.input_resources.iter().any(|i| {
                        i.kind == "document" && i.document_id == source.source_ref.resource_id
                    })
                {
                    return Err(WorkError::EvidenceNotFound);
                }
                if self.list_evidence(actor, task_id)?.len() >= MAX_VISIBLE_RECORDS {
                    return Err(WorkError::ValidationFailed);
                }
                let evidence = EvidenceRecord {
                    uncertainty: vec![],
                    conflict_references: vec![],
                    relevant_location_verified: false,
                    policy_disposition: "reference_only".into(),
                    id: Uuid::now_v7(),
                    revision: 1,
                    context_id: self.context_id,
                    task_id,
                    attempt_id: item.attempt_id,
                    created_by: actor.principal_id().into(),
                    acting_assignment_id: command.context().acting_assignment_id,
                    origin: "human".into(),
                    source: source.clone(),
                    relevant_location: relevant_location.clone(),
                    fragment_omission_reason: "not_retained".into(),
                    coverage: "unknown".into(),
                    retrieved_at: now.into(),
                    recorded_at: now.into(),
                    provider_checked_at: now.into(),
                    visibility: "work_item_private".into(),
                };
                self.evidence.push(evidence.clone());
                self.next_record_revision(task_id)?;
                Ok(MutationResult::EvidenceRegistered {
                    task: self.summary(actor, self.item(task_id)?),
                    evidence,
                })
            }
            Command::RegisterFinding {
                claim,
                evidence_revision_refs,
                supersedes_finding_id,
                ..
            } => {
                let finding = self.create_finding(
                    actor,
                    task_id,
                    FindingInput {
                        claim,
                        evidence_revision_refs,
                        supersedes_finding_id: *supersedes_finding_id,
                    },
                    None,
                    now,
                )?;
                self.findings.push(finding.clone());
                self.next_record_revision(task_id)?;
                Ok(MutationResult::FindingRegistered {
                    task: self.summary(actor, self.item(task_id)?),
                    finding,
                })
            }
            Command::RecordDecision {
                finding_id,
                finding_revision,
                decision,
                adopted_claim,
                reason,
                evidence_revision_refs,
                supersedes_decision_id,
                ..
            } => {
                let f = self.finding(actor, *finding_id)?;
                if f.revision != *finding_revision
                    || !self.record_in_task(
                        &item,
                        f.task_id,
                        f.attempt_id,
                        &RevisionRef {
                            id: f.id,
                            revision: f.revision,
                        },
                        |s| &s.finding_revision_refs,
                    )
                {
                    return Err(WorkError::FindingNotFound);
                }
                if *decision == DecisionKind::Modified && adopted_claim.is_none() {
                    return Err(WorkError::ValidationFailed);
                }
                if let Some(claim) = adopted_claim {
                    bounded_text(claim)?;
                }
                if *decision != DecisionKind::Modified && adopted_claim.is_some() {
                    return Err(WorkError::ValidationFailed);
                }
                if let Some(reason) = reason {
                    bounded_text(reason)?;
                }
                self.resolve_evidence(actor, evidence_revision_refs)?;
                for r in evidence_revision_refs {
                    let e = self.evidence_record(actor, r.id)?;
                    if !self.record_in_task(&item, e.task_id, e.attempt_id, r, |s| {
                        &s.evidence_revision_refs
                    }) {
                        return Err(WorkError::EvidenceNotFound);
                    }
                }
                if let Some(id) = supersedes_decision_id {
                    let d = self.decision(actor, *id)?;
                    if d.task_id != task_id
                        || d.attempt_id != item.attempt_id
                        || d.finding_id != *finding_id
                        || d.finding_revision != *finding_revision
                    {
                        return Err(WorkError::FindingNotFound);
                    }
                }
                if self
                    .decisions
                    .iter()
                    .filter(|d| d.attempt_id == item.attempt_id)
                    .count()
                    >= MAX_VISIBLE_RECORDS
                    || self.list_decisions(actor, *finding_id)?.len() >= MAX_VISIBLE_RECORDS
                {
                    return Err(WorkError::ValidationFailed);
                }
                let d = HumanDecision {
                    id: Uuid::now_v7(),
                    revision: 1,
                    context_id: self.context_id,
                    task_id,
                    attempt_id: item.attempt_id,
                    finding_id: *finding_id,
                    finding_revision: *finding_revision,
                    decision: *decision,
                    adopted_claim: adopted_claim.clone(),
                    reason: reason.clone(),
                    evidence_revision_refs: evidence_revision_refs.clone(),
                    human_principal: actor.principal_id().into(),
                    acting_assignment_id: command.context().acting_assignment_id,
                    created_at: now.into(),
                    supersedes_decision_id: *supersedes_decision_id,
                    visibility: "work_item_private".into(),
                };
                self.decisions.push(d.clone());
                self.next_record_revision(task_id)?;
                Ok(MutationResult::DecisionRecorded {
                    task: self.summary(actor, self.item(task_id)?),
                    decision: d,
                })
            }
            _ => Err(WorkError::IntegrityViolation),
        }
    }
}
