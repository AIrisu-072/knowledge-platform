use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EvidenceBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    source_ref: SourceRef,
    authoritative_locator: AuthoritativeLocator,
    relevant_location: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FindingBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    claim: String,
    evidence_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    supersedes_finding_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DecisionBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    task_id: Uuid,
    finding_revision: i64,
    decision: DecisionKind,
    #[serde(default)]
    adopted_claim: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    evidence_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    supersedes_decision_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CollectionQuery {
    limit: Option<usize>,
    cursor: Option<String>,
}
fn collection_limit(
    query: Result<Query<CollectionQuery>, QueryRejection>,
) -> Result<usize, Problem> {
    let Query(query) = query.map_err(|_| Problem(WorkError::ValidationFailed))?;
    let limit = query.limit.unwrap_or(50);
    if !(16..=100).contains(&limit) {
        return Err(Problem(WorkError::ValidationFailed));
    }
    if query.cursor.is_some() {
        return Err(Problem(WorkError::CursorStale));
    }
    Ok(limit)
}
fn collection<T>(items: Vec<T>, limit: usize) -> Result<Json<Page<T>>, Problem> {
    if items.len() > limit {
        return Err(Problem(WorkError::ValidationFailed));
    }
    Ok(Json(Page {
        items,
        next_cursor: None,
    }))
}
pub(super) async fn list_evidence(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<CollectionQuery>, QueryRejection>,
) -> Result<Json<Page<EvidenceRecord>>, Problem> {
    let limit = collection_limit(query)?;
    collection(
        s.repository.list_evidence(s.actor, path_id(path)?).await?,
        limit,
    )
}
pub(super) async fn evidence(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<EvidenceRecord>, Problem> {
    Ok(Json(s.repository.evidence(s.actor, path_id(path)?).await?))
}
pub(super) async fn list_findings(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<CollectionQuery>, QueryRejection>,
) -> Result<Json<Page<Finding>>, Problem> {
    let limit = collection_limit(query)?;
    collection(
        s.repository.list_findings(s.actor, path_id(path)?).await?,
        limit,
    )
}
pub(super) async fn finding(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Finding>, Problem> {
    Ok(Json(s.repository.finding(s.actor, path_id(path)?).await?))
}
pub(super) async fn list_decisions(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<CollectionQuery>, QueryRejection>,
) -> Result<Json<Page<HumanDecision>>, Problem> {
    let limit = collection_limit(query)?;
    collection(
        s.repository.list_decisions(s.actor, path_id(path)?).await?,
        limit,
    )
}
pub(super) async fn register_evidence(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<EvidenceBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let b = json_body(body)?;
    let command = Command::RegisterEvidence {
        task_id: path_id(path)?,
        context: CommandContext {
            operation_id: b.operation_id,
            expected_revision: b.expected_revision,
            acting_assignment_id: b.acting_assignment_id,
        },
        expected_attempt_id: b.expected_attempt_id,
        source: EvidenceSource {
            source_ref: b.source_ref,
            authoritative_locator: b.authoritative_locator,
        },
        relevant_location: b.relevant_location,
    };
    Ok(Json(s.repository.execute(s.actor, command).await?))
}
pub(super) async fn register_finding(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<FindingBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let b = json_body(body)?;
    let command = Command::RegisterFinding {
        task_id: path_id(path)?,
        context: CommandContext {
            operation_id: b.operation_id,
            expected_revision: b.expected_revision,
            acting_assignment_id: b.acting_assignment_id,
        },
        expected_attempt_id: b.expected_attempt_id,
        claim: b.claim,
        evidence_revision_refs: b.evidence_revision_refs,
        supersedes_finding_id: b.supersedes_finding_id,
    };
    Ok(Json(s.repository.execute(s.actor, command).await?))
}
pub(super) async fn record_decision(
    State(s): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<DecisionBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let b = json_body(body)?;
    let command = Command::RecordDecision {
        task_id: b.task_id,
        context: CommandContext {
            operation_id: b.operation_id,
            expected_revision: b.expected_revision,
            acting_assignment_id: b.acting_assignment_id,
        },
        expected_attempt_id: b.expected_attempt_id,
        finding_id: path_id(path)?,
        finding_revision: b.finding_revision,
        decision: b.decision,
        adopted_claim: b.adopted_claim,
        reason: b.reason,
        evidence_revision_refs: b.evidence_revision_refs,
        supersedes_decision_id: b.supersedes_decision_id,
    };
    Ok(Json(s.repository.execute(s.actor, command).await?))
}
