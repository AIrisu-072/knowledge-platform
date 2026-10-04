use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository};
use work_domain::*;
struct Unavailable;
impl WorkRepository for Unavailable {
    fn list_tasks(&self, _: VerifiedActor, _: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn task(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn artifact(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn snapshot(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn return_instruction(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn execute(&self, _: VerifiedActor, _: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn recover(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
}
fn app() -> axum::Router {
    work_api_http::router(Arc::new(Unavailable), VerifiedActor::Sales01)
}
#[tokio::test]
async fn startup_identity_is_returned_and_capabilities_are_explicitly_unavailable() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/organization/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["principalId"], "sales-01");
    assert_eq!(body["capabilities"]["fileUpload"], false);
}
#[tokio::test]
async fn identity_override_headers_and_query_are_rejected_instead_of_switching_actor() {
    for request in [
        Request::builder()
            .uri("/v1/organization/session")
            .header("x-principal-id", "office-01")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/v1/organization/session?principalId=office-01")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
#[tokio::test]
async fn unrecognized_body_fields_are_safe_problem_details() {
    let body = serde_json::json!({"operationId":Uuid::now_v7(), "expectedRevision":0,"actingAssignmentId":SALES_ASSIGNMENT_ID,"principalId":"office-01"});
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{SALES_TASK_ID}/claim"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/problem+json"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["code"], "VALIDATION_FAILED");
    assert!(!body.to_string().contains("office-01"));
}
#[tokio::test]
async fn repository_outage_is_not_an_empty_task_list() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/organization/tasks?view=queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["code"], "DEPENDENCY_UNAVAILABLE");
}
#[tokio::test]
async fn oversized_body_and_unknown_routes_use_closed_errors() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{SALES_TASK_ID}/claim"))
                .header("content-type", "application/json")
                .body(Body::from(" ".repeat(1_048_577)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/problem+json"
    );
}

#[tokio::test]
async fn return_transport_accepts_only_closed_command_and_has_instruction_read_route() {
    let valid = serde_json::json!({
        "operationId":Uuid::now_v7(),"expectedRevision":1,"actingAssignmentId":OFFICE_ASSIGNMENT_ID,
        "expectedAttemptId":OFFICE_ATTEMPT_ID,"previousSubmissionId":Uuid::now_v7(),
        "targetTaskId":SALES_TASK_ID,"transitionId":RETURN_TRANSITION_ID,"reason":"確認してください"
    });
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/return"))
                .header("content-type", "application/json")
                .body(Body::from(valid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    for field in ["principalId", "unexpected"] {
        let mut invalid = valid.clone();
        invalid[field] = serde_json::json!("office-01");
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/return"))
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let response = app()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/organization/return-instructions/{}",
                    Uuid::now_v7()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// Real domain state behind the transport, without a socket or a database. SQL
// atomicity and operation replay remain the separate opt-in PostgreSQL trial.
struct DomainFixture {
    workflow: std::sync::Mutex<Workflow>,
    outcomes: std::sync::Mutex<Vec<(Uuid, VerifiedActor, MutationResult)>>,
}
impl WorkRepository for DomainFixture {
    fn request_agent_execution(
        &self,
        actor: VerifiedActor,
        command: Command,
    ) -> WorkFuture<'_, work_application::AgentExecutionAcceptance> {
        Box::pin(async move {
            if let Ok(outcome) = self.recover(actor, command.context().operation_id).await {
                return Ok(work_application::AgentExecutionAcceptance {
                    outcome,
                    dispatch: false,
                });
            }
            let outcome = self.execute(actor, command).await?;
            Ok(work_application::AgentExecutionAcceptance {
                outcome,
                dispatch: true,
            })
        })
    }
    fn agent_execution(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async move { self.workflow.lock().unwrap().agent_execution(actor, id) })
    }
    fn agent_result(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, AgentResult> {
        Box::pin(async move { self.workflow.lock().unwrap().agent_result(actor, id) })
    }
    fn fail_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        code: AgentFailureCode,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async move {
            self.workflow.lock().unwrap().fail_agent_execution(
                actor,
                id,
                code,
                "2026-10-04T00:00:00Z",
            )
        })
    }

    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async move { Ok(self.workflow.lock().unwrap().list_tasks(actor, view)) })
    }
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async move { self.workflow.lock().unwrap().detail(actor, id) })
    }
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async move { self.workflow.lock().unwrap().artifact(actor, id) })
    }
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async move { self.workflow.lock().unwrap().snapshot(actor, id) })
    }
    fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async move { self.workflow.lock().unwrap().return_instruction(actor, id) })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            let result =
                self.workflow
                    .lock()
                    .unwrap()
                    .apply(actor, &command, "2026-10-04T00:00:00Z")?;
            self.outcomes.lock().unwrap().push((
                command.context().operation_id,
                actor,
                result.clone(),
            ));
            Ok(result)
        })
    }
    fn recover(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            let outcomes = self.outcomes.lock().unwrap();
            let (_, _, result) = outcomes
                .iter()
                .find(|(operation, owner, _)| *operation == id && *owner == actor)
                .ok_or(WorkError::WorkItemNotFound)?;
            self.workflow
                .lock()
                .unwrap()
                .authorize_recovery(actor, result)?;
            Ok(result.clone())
        })
    }
}
fn context(actor: VerifiedActor, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: actor.assignment_id(),
    }
}
async fn response_json(response: axum::response::Response) -> (StatusCode, serde_json::Value) {
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    (status, body)
}
#[tokio::test]
async fn return_http_closes_current_attempt_and_hides_rework_drafts_on_every_read_surface() {
    let fixture = Arc::new(DomainFixture {
        workflow: std::sync::Mutex::new(Workflow::synthetic(None)),
        outcomes: std::sync::Mutex::new(vec![]),
    });
    let first = fixture
        .execute(
            VerifiedActor::Sales01,
            Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: context(VerifiedActor::Sales01, 0),
                value: TextValue {
                    text: "OLD SUBMITTED TEXT".into(),
                },
            },
        )
        .await
        .unwrap();
    let artifact = match first {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    let submitted = fixture
        .execute(
            VerifiedActor::Sales01,
            Command::Submit {
                expected_attempt_id: None,
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 1),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: 0,
                }],
            },
        )
        .await
        .unwrap();
    let snapshot = match submitted {
        MutationResult::Submitted { snapshot, .. } => snapshot,
        _ => panic!(),
    };
    fixture
        .execute(
            VerifiedActor::Office01,
            Command::Claim {
                task_id: OFFICE_TASK_ID,
                context: context(VerifiedActor::Office01, 0),
            },
        )
        .await
        .unwrap();
    let office = work_api_http::router(fixture.clone(), VerifiedActor::Office01);
    let command = serde_json::json!({"operationId":Uuid::now_v7(), "expectedRevision":1,
        "actingAssignmentId":OFFICE_ASSIGNMENT_ID,"expectedAttemptId":OFFICE_ATTEMPT_ID,
        "previousSubmissionId":snapshot.id,"targetTaskId":SALES_TASK_ID,
        "transitionId":RETURN_TRANSITION_ID,"reason":"確認してください"});
    for (field, value, status) in [
        (
            "reason",
            serde_json::json!(" \t\n"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "reason",
            serde_json::json!("あ".repeat(2731)),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "expectedAttemptId",
            serde_json::json!(Uuid::now_v7()),
            StatusCode::CONFLICT,
        ),
        (
            "targetTaskId",
            serde_json::json!(Uuid::now_v7()),
            StatusCode::CONFLICT,
        ),
    ] {
        let before = fixture.workflow.lock().unwrap().clone();
        let mut invalid = command.clone();
        invalid[field] = value;
        let response = office
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/return"))
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (actual, body) = response_json(response).await;
        assert_eq!(actual, status);
        assert!(!body.to_string().contains("確認してください"));
        assert_eq!(*fixture.workflow.lock().unwrap(), before);
    }
    let response = office
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/return"))
                .header("content-type", "application/json")
                .body(Body::from(command.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, returned) = response_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(returned["kind"], "returned");
    assert_eq!(returned["nextTask"]["attemptNumber"], 2);
    let instruction = returned["returnInstruction"]["id"].as_str().unwrap();
    let sales = work_api_http::router(fixture.clone(), VerifiedActor::Sales01);
    let (status, read) = response_json(
        sales
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/organization/return-instructions/{instruction}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read, returned["returnInstruction"]);
    fixture
        .execute(
            VerifiedActor::Sales01,
            Command::Claim {
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 3),
            },
        )
        .await
        .unwrap();
    let save_context = context(VerifiedActor::Sales01, 4);
    let new_result = fixture
        .execute(
            VerifiedActor::Sales01,
            Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: save_context.clone(),
                value: TextValue {
                    text: "NEW PRIVATE REWORK".into(),
                },
            },
        )
        .await
        .unwrap();
    let artifact = match new_result {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    for path in [
        format!("/v1/organization/tasks/{SALES_TASK_ID}"),
        format!("/v1/organization/tasks/{SALES_TASK_ID}/working-artifacts"),
        format!("/v1/organization/working-artifacts/{}", artifact.id),
        format!("/v1/organization/operations/{}", save_context.operation_id),
    ] {
        let (status, body) = response_json(
            office
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.to_string().contains("NEW PRIVATE REWORK"));
    }
    for path in [
        "/v1/organization/tasks?view=context".to_string(),
        format!("/v1/organization/tasks/{OFFICE_TASK_ID}"),
        format!(
            "/v1/organization/operations/{}",
            command["operationId"].as_str().unwrap()
        ),
    ] {
        let (status, body) = response_json(
            office
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.to_string().contains("NEW PRIVATE REWORK"));
    }
}
#[tokio::test]
async fn evidence_and_finding_routes_are_closed_and_decision_requires_current_task_scope() {
    let common = serde_json::json!({"operationId":Uuid::now_v7(),"expectedRevision":0,"actingAssignmentId":SALES_ASSIGNMENT_ID,"expectedAttemptId":SALES_ATTEMPT_ID});
    let inputs = [
        (
            "evidence",
            serde_json::json!({"sourceRef":{"providerId":"document","resourceId":Uuid::now_v7(),"revisionId":Uuid::now_v7(),"versionId":Uuid::now_v7()},"authoritativeLocator":{"kind":"contentItem","contentItemId":Uuid::now_v7(),"representationId":Uuid::now_v7()},"relevantLocation":"原本"}),
        ),
        (
            "findings",
            serde_json::json!({"claim":"候補","evidenceRevisionRefs":[{"id":Uuid::now_v7(),"revision":1}]}),
        ),
    ];
    for (path, extra) in inputs {
        let mut body = common.clone();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let uri = format!("/v1/organization/tasks/{SALES_TASK_ID}/{path}");
        for method in ["GET", "POST"] {
            let response = app()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(&uri)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{method} {path}"
            );
        }
        body["createdBy"] = serde_json::json!("forged");
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let uri = format!("/v1/organization/findings/{}/decisions", Uuid::now_v7());
    let mut body = common;
    body.as_object_mut().unwrap().extend(serde_json::json!({"taskId":OFFICE_TASK_ID,"findingRevision":1,"decision":"accepted","evidenceRevisionRefs":[]}).as_object().unwrap().clone());
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    body.as_object_mut().unwrap().remove("taskId");
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
#[tokio::test]
async fn evidence_query_admission_precedes_repository_and_response_bytes_are_bounded() {
    for path in [
        format!("/v1/organization/tasks/{SALES_TASK_ID}/evidence?limit=15"),
        format!("/v1/organization/tasks/{SALES_TASK_ID}/findings?principalId=office-01"),
        format!(
            "/v1/organization/findings/{}/decisions?limit=101",
            Uuid::now_v7()
        ),
    ] {
        let response = app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid query must not reach unavailable provider"
        );
    }
    let mut workflow = Workflow::synthetic(Some(Uuid::now_v7()));
    workflow.input_resources[0].label = "PRIVATE-OVERSIZE".repeat(80_000);
    let repository = Arc::new(DomainFixture {
        workflow: std::sync::Mutex::new(workflow),
        outcomes: std::sync::Mutex::new(vec![]),
    });
    let response = work_api_http::router(repository, VerifiedActor::Sales01)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/organization/tasks/{SALES_TASK_ID}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = response_json(response).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "DEPENDENCY_UNAVAILABLE");
    assert!(!body.to_string().contains("PRIVATE-OVERSIZE"));
}

struct RecordingDispatch {
    calls: std::sync::atomic::AtomicUsize,
    reject: bool,
}
impl work_application::AgentDispatchPort for RecordingDispatch {
    fn dispatch(&self, _actor: VerifiedActor, _id: Uuid) -> Result<(), WorkError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.reject {
            Err(WorkError::DependencyUnavailable)
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn agent_http_is_closed_private_replay_safe_and_never_dispatches_from_reads() {
    let document = Uuid::from_u128(71);
    let mut workflow = Workflow::synthetic(Some(document));
    let evidence = Command::RegisterEvidence {
        task_id: SALES_TASK_ID,
        context: context(VerifiedActor::Sales01, 0),
        expected_attempt_id: SALES_ATTEMPT_ID,
        source: EvidenceSource {
            source_ref: SourceRef {
                provider_id: "document".into(),
                resource_id: document,
                revision_id: Uuid::from_u128(72),
                version_id: Uuid::from_u128(73),
            },
            authoritative_locator: AuthoritativeLocator {
                kind: "contentItem".into(),
                content_item_id: Uuid::from_u128(74),
                representation_id: Uuid::from_u128(75),
            },
        },
        relevant_location: "根拠".into(),
    };
    workflow
        .apply(VerifiedActor::Sales01, &evidence, "2026-10-04T00:00:00Z")
        .unwrap();
    let operation = Uuid::now_v7();
    let body = serde_json::json!({"operationId":operation,"expectedRevision":1,"actingAssignmentId":SALES_ASSIGNMENT_ID,"expectedAttemptId":SALES_ATTEMPT_ID,"purpose":"合成確認","evidenceRevisionRefs":[{"id":workflow.evidence[0].id,"revision":1}]});
    let fixture = Arc::new(DomainFixture {
        workflow: std::sync::Mutex::new(workflow),
        outcomes: std::sync::Mutex::new(vec![]),
    });
    let dispatch = Arc::new(RecordingDispatch {
        calls: std::sync::atomic::AtomicUsize::new(0),
        reject: false,
    });
    let app =
        work_api_http::router_with_agent(fixture.clone(), VerifiedActor::Sales01, dispatch.clone());
    let uri = format!("/v1/organization/tasks/{SALES_TASK_ID}/agent-executions");
    for field in [
        "requestedBy",
        "executedBy",
        "providerPrincipalBindings",
        "originExecutionId",
        "decision",
        "allowedTools",
    ] {
        let mut invalid = body.clone();
        invalid[field] = serde_json::json!("forged");
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(dispatch.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, result) = response_json(response).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(result["execution"]["status"], "queued");
        assert_eq!(result["execution"]["requestedBy"], "sales-01");
    }
    assert_eq!(dispatch.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    for (path, expected) in [
        (
            format!("/v1/organization/agent-executions/{operation}"),
            StatusCode::OK,
        ),
        (
            format!("/v1/organization/agent-executions/{operation}/result"),
            StatusCode::CONFLICT,
        ),
        (
            format!("/v1/organization/operations/{operation}"),
            StatusCode::OK,
        ),
    ] {
        let (status, result) = response_json(
            app.clone()
                .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, expected);
        if expected == StatusCode::CONFLICT {
            assert_eq!(result["code"], "AGENT_RESULT_NOT_READY");
        }
        let office = work_api_http::router_with_agent(
            fixture.clone(),
            VerifiedActor::Office01,
            dispatch.clone(),
        );
        let response = office
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(dispatch.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let cancel = serde_json::json!({"operationId":Uuid::now_v7(),"expectedRevision":2,"actingAssignmentId":SALES_ASSIGNMENT_ID,"expectedAttemptId":SALES_ATTEMPT_ID,"taskId":SALES_TASK_ID});
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/organization/agent-executions/{operation}/cancel"
                ))
                .header("content-type", "application/json")
                .body(Body::from(cancel.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, result) = response_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["execution"]["status"], "cancelled");
}

#[tokio::test]
async fn completion_transport_accepts_only_closed_complete_action() {
    let valid = serde_json::json!({"operationId":Uuid::now_v7(),"expectedRevision":1,
        "actingAssignmentId":OFFICE_ASSIGNMENT_ID,"expectedAttemptId":OFFICE_ATTEMPT_ID,
        "action":"complete","definitionActionId":"01900000-0000-7000-8000-000000000012"});
    let request = |body: serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/actions"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(
        app()
            .oneshot(request(valid.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    for action in ["hold", "resume", "submit", "COMPLETE"] {
        let mut invalid = valid.clone();
        invalid["action"] = serde_json::json!(action);
        let (status, body) = response_json(app().oneshot(request(invalid)).await.unwrap()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "VALIDATION_FAILED");
    }
    for field in ["principalId", "unexpected"] {
        let mut invalid = valid.clone();
        invalid[field] = serde_json::json!("office-01");
        assert_eq!(
            app().oneshot(request(invalid)).await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    for field in ["expectedAttemptId", "definitionActionId", "action"] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert_eq!(
            app().oneshot(request(invalid)).await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}

#[tokio::test]
async fn completion_http_validates_actor_occ_and_action_then_recovers_readonly_outcome() {
    let mut workflow = Workflow::synthetic(None);
    let saved = workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: context(VerifiedActor::Sales01, 0),
                value: TextValue {
                    text: "提出本文".into(),
                },
            },
            "2026-10-04T00:00:00Z",
        )
        .unwrap();
    let artifact = match saved {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::Submit {
                task_id: SALES_TASK_ID,
                context: context(VerifiedActor::Sales01, 1),
                expected_attempt_id: Some(SALES_ATTEMPT_ID),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: 0,
                }],
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
            },
            "2026-10-04T00:00:00Z",
        )
        .unwrap();
    workflow
        .apply(
            VerifiedActor::Office01,
            &Command::Claim {
                task_id: OFFICE_TASK_ID,
                context: context(VerifiedActor::Office01, 0),
            },
            "2026-10-04T00:00:00Z",
        )
        .unwrap();
    let before = workflow.clone();
    let fixture = Arc::new(DomainFixture {
        workflow: std::sync::Mutex::new(workflow),
        outcomes: std::sync::Mutex::new(vec![]),
    });
    let office = work_api_http::router(fixture.clone(), VerifiedActor::Office01);
    let sales = work_api_http::router(fixture.clone(), VerifiedActor::Sales01);
    let operation = Uuid::now_v7();
    let valid = serde_json::json!({"operationId":operation,"expectedRevision":1,"actingAssignmentId":OFFICE_ASSIGNMENT_ID,"expectedAttemptId":OFFICE_ATTEMPT_ID,"definitionActionId":COMPLETE_ACTION_ID,"action":"complete"});
    let request = |body: serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}/actions"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    for (field, value, code) in [
        (
            "expectedRevision",
            serde_json::json!(0),
            "REVISION_CONFLICT",
        ),
        (
            "expectedAttemptId",
            serde_json::json!(Uuid::now_v7()),
            "REVISION_CONFLICT",
        ),
        (
            "definitionActionId",
            serde_json::json!(RETURN_TRANSITION_ID),
            "HANDOFF_NOT_READY",
        ),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        let (status, body) =
            response_json(office.clone().oneshot(request(bad)).await.unwrap()).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], code);
        assert_eq!(*fixture.workflow.lock().unwrap(), before);
    }
    let mut other = valid.clone();
    other["actingAssignmentId"] = serde_json::json!(SALES_ASSIGNMENT_ID);
    assert_eq!(
        sales
            .clone()
            .oneshot(request(other))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let (status, outcome) = response_json(
        office
            .clone()
            .oneshot(request(valid.clone()))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["kind"], "completed");
    let (status, restored) = response_json(
        office
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/organization/operations/{operation}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored, outcome);
    assert_eq!(
        sales
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/organization/operations/{operation}"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let (status, detail) = response_json(
        office
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/organization/tasks/{OFFICE_TASK_ID}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["state"], "completed");
    assert_eq!(detail["canComplete"], false);
    let mut terminal = valid;
    terminal["operationId"] = serde_json::json!(Uuid::now_v7());
    terminal["expectedRevision"] = detail["revision"].clone();
    let (status, body) = response_json(office.oneshot(request(terminal)).await.unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "HANDOFF_NOT_READY");
    assert_eq!(fixture.workflow.lock().unwrap().snapshots, before.snapshots);
}
