//! WorkContext, Attention and WorkViewProfile transport (U2) over an in-memory
//! repository holding every fixture instance and using the real domain.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tower::ServiceExt;
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository};
use work_domain::*;

const NOW: &str = "2026-10-07T09:00:00Z";
fn now() -> OffsetDateTime {
    OffsetDateTime::parse(NOW, &Rfc3339).unwrap()
}
struct Memory {
    workflows: Mutex<Vec<Workflow>>,
    seen: Mutex<BTreeSet<(VerifiedActor, Uuid)>>,
}
impl Memory {
    fn seeded() -> Arc<Self> {
        let workflows = CONTEXT_FIXTURES
            .iter()
            .map(|fixture| Workflow::from_fixture(fixture, None, now()).unwrap())
            .collect();
        Arc::new(Self {
            workflows: Mutex::new(workflows),
            seen: Mutex::new(BTreeSet::new()),
        })
    }
    fn all(&self, actor: VerifiedActor) -> Vec<Workflow> {
        let seen: BTreeSet<Uuid> = self
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(owner, _)| *owner == actor)
            .map(|(_, id)| *id)
            .collect();
        self.workflows
            .lock()
            .unwrap()
            .iter()
            .map(|w| {
                let mut w = w
                    .clone()
                    .with_authority(OrganizationPolicy::synthetic(), now());
                w.attach_acknowledgements(actor, seen.clone());
                w
            })
            .collect()
    }
    fn owner(&self, actor: VerifiedActor, target: WorkTarget) -> Workflow {
        let mut all = self.all(actor);
        let index = all.iter().position(|w| w.owns(target)).unwrap_or(0);
        all.swap_remove(index)
    }
}
impl WorkRepository for Memory {
    fn organization(&self, actor: VerifiedActor) -> WorkFuture<'_, OrganizationView> {
        Box::pin(async move { OrganizationPolicy::synthetic().view(actor, now()) })
    }
    fn list_tasks_in(
        &self,
        actor: VerifiedActor,
        view: TaskView,
        scope: Option<Uuid>,
    ) -> WorkFuture<'_, Vec<TaskSummary>> {
        let result = self.all(actor).iter().try_fold(vec![], |mut items, w| {
            items.extend(w.list_tasks_in(actor, view, scope)?);
            Ok(items)
        });
        Box::pin(async move { result })
    }
    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        self.list_tasks_in(actor, view, None)
    }
    fn list_work_contexts(
        &self,
        actor: VerifiedActor,
        scope: Option<Uuid>,
    ) -> WorkFuture<'_, Vec<WorkContextView>> {
        let result = if scope.is_some_and(|id| {
            OrganizationPolicy::synthetic()
                .responsibility(actor, id, now())
                .is_none()
        }) {
            Err(WorkError::Forbidden)
        } else {
            Ok(self
                .all(actor)
                .iter()
                .filter_map(|w| w.context_view(actor, scope))
                .collect())
        };
        Box::pin(async move { result })
    }
    fn work_context(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkContextView> {
        let w = self.owner(actor, WorkTarget::Context(id));
        let result = w
            .owns(WorkTarget::Context(id))
            .then(|| w.context_view(actor, None))
            .flatten()
            .ok_or(WorkError::WorkContextNotFound);
        Box::pin(async move { result })
    }
    fn work_context_history(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, WorkContextHistory> {
        let w = self.owner(actor, WorkTarget::Context(id));
        let result = if w.owns(WorkTarget::Context(id)) {
            w.context_history(actor)
        } else {
            Err(WorkError::WorkContextNotFound)
        };
        Box::pin(async move { result })
    }
    fn task_attention(&self, actor: VerifiedActor, task_id: Uuid) -> WorkFuture<'_, TaskAttention> {
        let result = self
            .owner(actor, WorkTarget::Task(task_id))
            .attention(actor, task_id);
        Box::pin(async move { result })
    }
    fn acknowledge_attention(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
        work_assignment_id: Uuid,
    ) -> WorkFuture<'_, TaskAttention> {
        let result = self
            .owner(actor, WorkTarget::Task(task_id))
            .acknowledge(actor, task_id, work_assignment_id)
            .and_then(|acknowledged| {
                self.seen
                    .lock()
                    .unwrap()
                    .insert((actor, acknowledged.work_assignment_id));
                self.owner(actor, WorkTarget::Task(task_id))
                    .attention(actor, task_id)
            });
        Box::pin(async move { result })
    }
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail> {
        let result = self.owner(actor, WorkTarget::Task(id)).detail(actor, id);
        Box::pin(async move { result })
    }
    fn artifact(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async { Err(WorkError::WorkArtifactNotFound) })
    }
    fn snapshot(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async { Err(WorkError::WorkArtifactNotFound) })
    }
    fn return_instruction(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async { Err(WorkError::WorkArtifactNotFound) })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        let mut workflows = self.workflows.lock().unwrap();
        let index = workflows
            .iter()
            .position(|w| w.owns(WorkTarget::Task(command.task_id())))
            .unwrap_or(0);
        let mut w = workflows[index]
            .clone()
            .with_authority(OrganizationPolicy::synthetic(), now());
        let result = w.apply(actor, &command, NOW);
        if result.is_ok() {
            workflows[index] = w;
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
async fn profiles_and_session_defaults_are_presentation_only() {
    let repository = Memory::seeded();
    let (status, profiles) = call(
        &repository,
        VerifiedActor::Delegate01,
        "GET",
        "/v1/organization/work-view-profiles",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys: Vec<_> = profiles["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["key"].clone())
        .collect();
    assert_eq!(
        keys,
        [
            json!("sales-context"),
            json!("office-queue"),
            json!("review-queue")
        ]
    );
    assert_eq!(profiles["items"][2]["initialModule"], "evidence");
    let (_, session) = call(
        &repository,
        VerifiedActor::MultiRole01,
        "GET",
        "/v1/organization/session",
        None,
    )
    .await;
    let defaults: Vec<_> = session["responsibilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["workViewProfileId"].clone())
        .collect();
    assert_eq!(
        defaults,
        [
            json!(PROFILE_OFFICE_QUEUE_ID),
            json!(PROFILE_REVIEW_QUEUE_ID)
        ]
    );
}

#[tokio::test]
async fn contexts_are_disclosed_only_to_authorized_readers_and_filters_never_widen() {
    let repository = Memory::seeded();
    let (status, sales) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        "/v1/organization/work-contexts",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<_> = sales["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["title"].clone())
        .collect();
    assert_eq!(
        titles,
        [
            json!("合成案件A・設備更新相談"),
            json!("合成案件B・運転資金相談"),
            json!("合成依頼C・住所変更届")
        ]
    );
    assert_eq!(
        sales["items"][2]["progress"][0]["dueAt"]
            .as_str()
            .map(|value| value.ends_with('Z')),
        Some(true)
    );
    let (_, office) = call(
        &repository,
        VerifiedActor::Office01,
        "GET",
        "/v1/organization/work-contexts",
        None,
    )
    .await;
    assert_eq!(office["items"], json!([]));
    for uri in [
        format!("/v1/organization/work-contexts/{CONTEXT_B_ID}"),
        format!("/v1/organization/work-contexts/{CONTEXT_B_ID}/history"),
        format!("/v1/organization/work-contexts/{}", Uuid::now_v7()),
        "/v1/organization/work-contexts/not-a-uuid".into(),
    ] {
        let (status, problem) = call(&repository, VerifiedActor::Office01, "GET", &uri, None).await;
        assert_eq!(
            (status, problem["code"].clone()),
            (StatusCode::NOT_FOUND, json!("WORK_CONTEXT_NOT_FOUND")),
            "{uri}"
        );
    }
    let (status, history) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        &format!("/v1/organization/work-contexts/{CONTEXT_C_ID}/history"),
        None,
    )
    .await;
    assert_eq!(
        (status, history["contextId"].clone()),
        (StatusCode::OK, json!(CONTEXT_C_ID))
    );
    // Another principal's responsibility is refused rather than adopted as a scope.
    let (status, _) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        &format!("/v1/organization/work-contexts?actingAssignmentId={OFFICE_ASSIGNMENT_ID}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        "/v1/organization/work-contexts?principalId=office-01",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Task filters narrow the same authorized projection.
    let (_, filtered) = call(
        &repository,
        VerifiedActor::Sales01,
        "GET",
        &format!("/v1/organization/tasks?view=context&contextId={CONTEXT_C_ID}"),
        None,
    )
    .await;
    let items = filtered["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!(CONTEXT_C_SALES_TASK_ID));
    assert_eq!(items[0]["contextTitle"], "合成依頼C・住所変更届");
    assert_eq!(items[0]["workTypeLabel"], "営業内容整理");
    assert_eq!(items[0]["attention"][0]["kind"], "overdue");
    let (_, none) = call(
        &repository,
        VerifiedActor::Office01,
        "GET",
        &format!("/v1/organization/tasks?view=queue&contextId={CONTEXT_C_ID}"),
        None,
    )
    .await;
    assert_eq!(none["items"], json!([]));
}

#[tokio::test]
async fn attention_acknowledgment_is_the_assignees_own_and_idempotent() {
    let repository = Memory::seeded();
    let (status, result) = call(&repository, VerifiedActor::Approver01, "POST", &format!("/v1/organization/tasks/{CONTEXT_B_SALES_TASK_ID}/assignment"), Some(json!({"operationId":Uuid::now_v7(),"expectedRevision":0,"actingAssignmentId":APPROVER_MANAGEMENT_ASSIGNMENT_ID,"expectedAttemptId":context_fixture(CONTEXT_B_WORKFLOW_ID).unwrap().source_attempt_id,"assigneePrincipalId":"sales-01","assigneeResponsibilityId":SALES_ASSIGNMENT_ID,"reason":"担当の割当"}))).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let period = result["assignment"]["id"].clone();
    let uri = format!("/v1/organization/tasks/{CONTEXT_B_SALES_TASK_ID}/attention");
    let (_, attention) = call(&repository, VerifiedActor::Sales01, "GET", &uri, None).await;
    assert_eq!(
        attention["items"][0],
        json!({"kind":"newly_assigned","sourceId":period,"dueAt":null})
    );
    let seen = format!("/v1/organization/tasks/{CONTEXT_B_SALES_TASK_ID}/attention-seen");
    for (actor, body, expected) in [
        (
            VerifiedActor::Sales01,
            json!({"workAssignmentId":period,"principalId":"office-01"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            VerifiedActor::Sales01,
            json!({"workAssignmentId":Uuid::now_v7()}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            VerifiedActor::Approver01,
            json!({"workAssignmentId":period}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            call(&repository, actor, "POST", &seen, Some(body)).await.0,
            expected
        );
    }
    for _ in 0..2 {
        let (status, after) = call(
            &repository,
            VerifiedActor::Sales01,
            "POST",
            &seen,
            Some(json!({"workAssignmentId":period})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let kinds: Vec<_> = after["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value["kind"].clone())
            .collect();
        assert_eq!(kinds, [json!("due_soon")]);
    }
    let (status, _) = call(&repository, VerifiedActor::Delegate01, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
