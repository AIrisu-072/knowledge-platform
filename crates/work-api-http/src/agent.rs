use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AgentRequestBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    purpose: String,
    evidence_revision_refs: Vec<RevisionRef>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AgentCancelBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    task_id: Uuid,
}
pub(super) async fn request_agent_execution(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<AgentRequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<MutationResult>), Problem> {
    let body = json_body(body)?;
    let task_id = path_id(path)?;
    let dispatcher = state
        .agent_dispatch
        .ok_or(WorkError::DependencyUnavailable)?;
    let accepted = state
        .repository
        .request_agent_execution(
            state.actor,
            Command::RequestAgentExecution {
                task_id,
                context: CommandContext {
                    operation_id: body.operation_id,
                    expected_revision: body.expected_revision,
                    acting_assignment_id: body.acting_assignment_id,
                },
                expected_attempt_id: body.expected_attempt_id,
                purpose: body.purpose,
                evidence_revision_refs: body.evidence_revision_refs,
            },
        )
        .await?;
    if accepted.dispatch {
        let MutationResult::AgentExecutionRequested { execution, .. } = &accepted.outcome else {
            return Err(WorkError::IntegrityViolation.into());
        };
        if let Err(error) = dispatcher.dispatch(state.actor, execution.id) {
            state
                .repository
                .fail_agent_execution(
                    state.actor,
                    execution.id,
                    AgentFailureCode::DependencyUnavailable,
                )
                .await?;
            return Err(error.into());
        }
    }
    Ok((StatusCode::ACCEPTED, Json(accepted.outcome)))
}
pub(super) async fn agent_execution(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<AgentExecution>, Problem> {
    Ok(Json(
        state
            .repository
            .agent_execution(state.actor, path_id(path)?)
            .await?,
    ))
}
/// Private draft candidate under the execution's current scope; read-only.
pub(super) async fn generated_artifact(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<GeneratedArtifact>, Problem> {
    Ok(Json(
        state
            .repository
            .generated_artifact(state.actor, path_id(path)?)
            .await?,
    ))
}
/// Typed proposal; there is deliberately no route that executes it.
pub(super) async fn suggested_action(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<SuggestedAction>, Problem> {
    Ok(Json(
        state
            .repository
            .suggested_action(state.actor, path_id(path)?)
            .await?,
    ))
}
pub(super) async fn agent_result(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<AgentResult>, Problem> {
    Ok(Json(
        state
            .repository
            .agent_result(state.actor, path_id(path)?)
            .await?,
    ))
}
pub(super) async fn cancel_agent_execution(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<AgentCancelBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let execution_id = path_id(path)?;
    Ok(Json(
        state
            .repository
            .execute(
                state.actor,
                Command::CancelAgentExecution {
                    task_id: body.task_id,
                    context: CommandContext {
                        operation_id: body.operation_id,
                        expected_revision: body.expected_revision,
                        acting_assignment_id: body.acting_assignment_id,
                    },
                    expected_attempt_id: body.expected_attempt_id,
                    execution_id,
                },
            )
            .await?,
    ))
}
