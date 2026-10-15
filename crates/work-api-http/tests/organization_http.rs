//! Organization transport: session responsibilities, policy commands, projection
//! scope and assignment, over an in-memory repository using the real domain.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository};
use work_domain::*;

const NOW: &str = "2026-10-07T09:00:00Z";
struct Memory {
    state: Mutex<(Workflow, OrganizationPolicy)>,
}
impl Memory {
    fn received() -> Arc<Self> {
        let mut workflow = Workflow::synthetic(None);
        let MutationResult::DraftSaved { artifact, .. } = workflow
            .apply(
                VerifiedActor::Sales01,
                &Command::SaveDraft {
                    task_id: SALES_TASK_ID,
                    artifact_id: None,
                    context: ctx(SALES_ASSIGNMENT_ID, 0),
                    value: TextValue {
                        text: "非公開".into(),
                    },
                },
                NOW,
            )
            .unwrap()
        else {
            panic!()
        };
        workflow
            .apply(
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
                NOW,
            )
            .unwrap();
        Arc::new(Self {
            state: Mutex::new((workflow, OrganizationPolicy::synthetic())),
        })
    }
    fn current(&self) -> Workflow {
        let state = self.state.lock().unwrap();
        state.0.clone().with_authority(
            state.1.clone(),
            time::OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339)
                .unwrap(),
        )
    }
}
fn ctx(acting: Uuid, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: acting,
    }
}
impl WorkRepository for Memory {
    fn organization(&self, actor: VerifiedActor) -> WorkFuture<'_, OrganizationView> {
        let policy = self.state.lock().unwrap().1.clone();
        Box::pin(async move {
            policy.view(
                actor,
                time::OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339)
                    .unwrap(),
            )
        })
    }
    fn execute_policy(
        &self,
        actor: VerifiedActor,
        command: PolicyCommand,
    ) -> WorkFuture<'_, MutationResult> {
        let result = self.state.lock().unwrap().1.apply(actor, &command, NOW);
        Box::pin(async move { result })
    }
    fn list_tasks_in(
        &self,
        actor: VerifiedActor,
        view: TaskView,
        scope: Option<Uuid>,
    ) -> WorkFuture<'_, Vec<TaskSummary>> {
        let result = self.current().list_tasks_in(actor, view, scope);
        Box::pin(async move { result })
    }
    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        let result = self.current().list_tasks(actor, view);
        Box::pin(async move { Ok(result) })
    }
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail> {
        let result = self.current().detail(actor, id);
        Box::pin(async move { result })
    }
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        let result = self.current().artifact(actor, id);
        Box::pin(async move { result })
    }
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        let result = self.current().snapshot(actor, id);
        Box::pin(async move { result })
    }
    fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, ReturnInstruction> {
        let result = self.current().return_instruction(actor, id);
        Box::pin(async move { result })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        let mut state = self.state.lock().unwrap();
        let policy = state.1.clone();
        let mut workflow = state.0.clone().with_authority(
            policy,
            time::OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339)
                .unwrap(),
        );
        let result = workflow.apply(actor, &command, NOW);
        if result.is_ok() {
            state.0 = workflow;
        }
        Box::pin(async move { result })
    }
    fn recover(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::WorkItemNotFound) })
    }
}
async fn call(
    repository: &Arc<Memory>,
    actor: VerifiedActor,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let app = work_api_http::router(repository.clone(), actor);
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = app
        .oneshot(
            request
                .body(body.map_or(Body::empty(), |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn session_lists_current_responsibilities_without_inventing_a_grant() {
    let repository = Memory::received();
    let (status, multi) = call(
        &repository,
        VerifiedActor::MultiRole01,
        "GET",
        "/v1/organization/session",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(multi["principalId"], "multi-role-01");
    assert_eq!(multi["responsibilities"].as_array().unwrap().len(), 2);
    assert_eq!(
        multi["actingAssignmentId"],
        json!(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID)
    );
    assert_eq!(multi["canManageOrganization"], false);
    let (_, delegate) = call(
        &repository,
        VerifiedActor::Delegate01,
        "GET",
        "/v1/organization/session",
        None,
    )
    .await;
    assert_eq!(delegate["responsibilities"], json!([]));
    assert!(delegate["actingAssignmentId"].is_null());
    let (_, approver) = call(
        &repository,
        VerifiedActor::Approver01,
        "GET",
        "/v1/organization/session",
        None,
    )
    .await;
    assert_eq!(approver["canManageOrganization"], true);
    assert_eq!(
        approver["responsibilities"][1]["actions"],
        json!(["queue.read", "work.assign", "organization.manage"])
    );
}

#[tokio::test]
async fn task_projection_scope_is_revalidated_and_never_switches_identity() {
    let repository = Memory::received();
    let uri =
        |acting: Uuid| format!("/v1/organization/tasks?view=queue&actingAssignmentId={acting}");
    let (status, page) = call(
        &repository,
        VerifiedActor::MultiRole01,
        "GET",
        &uri(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"][0]["id"], json!(OFFICE_TASK_ID));
    assert_eq!(page["items"][0]["canClaim"], true);
    assert!(page["items"][0]["assignment"].is_null());
    let (_, page) = call(
        &repository,
        VerifiedActor::MultiRole01,
        "GET",
        &uri(MULTI_ROLE_REVIEW_ASSIGNMENT_ID),
        None,
    )
    .await;
    assert_eq!(page["items"], json!([]));
    // Another principal's responsibility is refused rather than adopted.
    let (status, problem) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        &uri(OFFICE_ASSIGNMENT_ID),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(problem["code"], "FORBIDDEN");
    // Policy record pages carry the trusted evaluation instant for status hints.
    for path in [
        "/v1/organization/role-assignments",
        "/v1/organization/delegations",
    ] {
        let (status, page) = call(&repository, VerifiedActor::Sales01, "GET", path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(
            page["evaluatedAt"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z')),
            "{path}"
        );
    }
    for uri in [
        "/v1/organization/role-assignments?principalId=office-01".to_string(),
        "/v1/organization/delegations?all=true".to_string(),
        "/v1/organization/tasks?principalId=office-01".to_string(),
    ] {
        let (status, _) = call(&repository, VerifiedActor::Sales01, "GET", &uri, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
    }
}

#[tokio::test]
async fn policy_commands_require_management_or_own_assignment_and_closed_bodies() {
    let repository = Memory::received();
    let assignment = |acting: Uuid, principal: &str| json!({"operationId":Uuid::now_v7(),"expectedRevision":0,"actingAssignmentId":acting,"principalId":principal,"roleId":ROLE_PROCESSING_ID,"unitId":UNIT_OFFICE_ID,"reason":"応援"});
    let (status, _) = call(
        &repository,
        VerifiedActor::Office01,
        "POST",
        "/v1/organization/role-assignments",
        Some(assignment(OFFICE_ASSIGNMENT_ID, "delegate-01")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        "/v1/organization/role-assignments",
        Some(assignment(APPROVER_MANAGEMENT_ASSIGNMENT_ID, "admin")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut extra = assignment(APPROVER_MANAGEMENT_ASSIGNMENT_ID, "delegate-01");
    extra["grantAll"] = json!(true);
    let (status, _) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        "/v1/organization/role-assignments",
        Some(extra),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, created) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        "/v1/organization/role-assignments",
        Some(assignment(APPROVER_MANAGEMENT_ASSIGNMENT_ID, "delegate-01")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["kind"], "role_assignment_created");
    assert_eq!(created["assignment"]["principal"], "delegate-01");
    assert_eq!(created["policyRevision"], 1);
    let (_, own) = call(
        &repository,
        VerifiedActor::Delegate01,
        "GET",
        "/v1/organization/role-assignments",
        None,
    )
    .await;
    assert_eq!(own["items"].as_array().unwrap().len(), 1);
    let (_, all) = call(
        &repository,
        VerifiedActor::Approver01,
        "GET",
        "/v1/organization/role-assignments",
        None,
    )
    .await;
    assert_eq!(all["items"].as_array().unwrap().len(), 8);
    // Stale OCC and unknown records use the shared closed problem codes.
    let revoke = json!({"operationId":Uuid::now_v7(),"expectedRevision":0,"actingAssignmentId":APPROVER_MANAGEMENT_ASSIGNMENT_ID,"reason":"終了"});
    let (status, problem) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        &format!(
            "/v1/organization/role-assignments/{}/revoke",
            created["assignment"]["id"].as_str().unwrap()
        ),
        Some(revoke.clone()),
    )
    .await;
    assert_eq!(
        (status, problem["code"].clone()),
        (StatusCode::CONFLICT, json!("REVISION_CONFLICT"))
    );
    let mut current = revoke.clone();
    current["expectedRevision"] = json!(1);
    let (status, problem) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        &format!(
            "/v1/organization/role-assignments/{}/revoke",
            Uuid::now_v7()
        ),
        Some(current.clone()),
    )
    .await;
    assert_eq!(
        (status, problem["code"].clone()),
        (
            StatusCode::NOT_FOUND,
            json!("ORGANIZATION_RECORD_NOT_FOUND")
        )
    );
    // Own delegation: the holder narrows its own processing responsibility.
    let delegation = json!({"operationId":Uuid::now_v7(),"expectedRevision":1,"actingAssignmentId":OFFICE_ASSIGNMENT_ID,"sourceAssignmentId":OFFICE_ASSIGNMENT_ID,"recipientPrincipalId":"delegate-01","actions":["queue.read","work.read","work.claim"],"validUntil":"2026-10-07T18:00:00Z","reason":"代理"});
    let (status, created) = call(
        &repository,
        VerifiedActor::Office01,
        "POST",
        "/v1/organization/delegations",
        Some(delegation),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let (_, seen) = call(
        &repository,
        VerifiedActor::Delegate01,
        "GET",
        "/v1/organization/delegations",
        None,
    )
    .await;
    assert_eq!(seen["items"][0]["delegator"], "office-01");
    let (_, session) = call(
        &repository,
        VerifiedActor::Delegate01,
        "GET",
        "/v1/organization/session",
        None,
    )
    .await;
    assert_eq!(session["responsibilities"].as_array().unwrap().len(), 2);
    let revoke = json!({"operationId":Uuid::now_v7(),"expectedRevision":2,"actingAssignmentId":OFFICE_ASSIGNMENT_ID,"reason":"復帰"});
    let (status, revoked) = call(
        &repository,
        VerifiedActor::Office01,
        "POST",
        &format!(
            "/v1/organization/delegations/{}/revoke",
            created["delegation"]["id"].as_str().unwrap()
        ),
        Some(revoke),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(revoked["kind"], "delegation_revoked");
}

#[tokio::test]
async fn assignment_route_reassigns_with_management_and_rejects_forged_assignees() {
    let repository = Memory::received();
    let (_, page) = call(&repository, VerifiedActor::Approver01, "GET", &format!("/v1/organization/tasks?view=queue&actingAssignmentId={APPROVER_MANAGEMENT_ASSIGNMENT_ID}"), None).await;
    let task = &page["items"][0];
    assert_eq!(task["canAssign"], true);
    let body = |assignee: &str, responsibility: Uuid| json!({"operationId":Uuid::now_v7(),"expectedRevision":task["revision"],"actingAssignmentId":APPROVER_MANAGEMENT_ASSIGNMENT_ID,"expectedAttemptId":task["attemptId"],"assigneePrincipalId":assignee,"assigneeResponsibilityId":responsibility,"reason":"割当"});
    let uri = format!("/v1/organization/tasks/{OFFICE_TASK_ID}/assignment");
    // A responsibility that belongs to someone else is not an eligible assignee.
    let (status, _) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        &uri,
        Some(body("multi-role-01", OFFICE_ASSIGNMENT_ID)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&repository, VerifiedActor::Office01, "POST", &uri, Some(json!({"operationId":Uuid::now_v7(),"expectedRevision":task["revision"],"actingAssignmentId":OFFICE_ASSIGNMENT_ID,"expectedAttemptId":task["attemptId"],"assigneePrincipalId":"office-01","assigneeResponsibilityId":OFFICE_ASSIGNMENT_ID,"reason":"自分へ"}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, assigned) = call(
        &repository,
        VerifiedActor::Approver01,
        "POST",
        &uri,
        Some(body("multi-role-01", MULTI_ROLE_PROCESSING_ASSIGNMENT_ID)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    assert_eq!(assigned["kind"], "assigned");
    assert_eq!(assigned["assignment"]["principal"], "multi-role-01");
    let (status, detail) = call(
        &repository,
        VerifiedActor::MultiRole01,
        "GET",
        &format!("/v1/organization/tasks/{OFFICE_TASK_ID}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        detail["assignment"]["actingAssignmentId"],
        json!(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID)
    );
    let (status, _) = call(
        &repository,
        VerifiedActor::Office01,
        "GET",
        &format!("/v1/organization/tasks/{OFFICE_TASK_ID}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
