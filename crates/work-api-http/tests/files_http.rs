//! Private work file transport (U3): bounded binary bodies, header-carried
//! operation identity, opaque attachment responses and unavailable-store errors.
//! The repository double records calls; domain and store rules are tested there.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository};
use work_domain::*;

const NAME: &str = "合成 資料\"1\".txt";
#[derive(Default)]
struct Recorder {
    writes: Mutex<Vec<(Uuid, CommandContext, i64, usize)>>,
    commands: Mutex<Vec<Command>>,
    reads: Mutex<Vec<(Option<Uuid>, Uuid)>>,
    content: Mutex<Option<Result<Vec<u8>, WorkError>>>,
}
fn summary() -> TaskSummary {
    Workflow::synthetic(None)
        .detail(VerifiedActor::Sales01, SALES_TASK_ID)
        .unwrap()
        .task
}
fn file() -> WorkFile {
    WorkFile {
        file_name: NAME.into(),
        media_type: "text/html".into(),
        generation: Some(FileGeneration {
            id: Uuid::now_v7(),
            size_bytes: 3,
            sha256: "0".repeat(64),
            stored_at: "2026-10-07T09:00:00Z".into(),
            provider_id: WORK_ARTIFACT_PROVIDER_ID.into(),
        }),
    }
}
fn artifact(id: Uuid) -> WorkingArtifact {
    WorkingArtifact {
        id,
        task_id: SALES_TASK_ID,
        attempt_id: SALES_ATTEMPT_ID,
        revision: 1,
        schema_id: FILE_SCHEMA_ID.into(),
        value: None,
        visibility: "work_item_private".into(),
        file: Some(file()),
        derived_from: None,
    }
}
impl Recorder {
    fn content(&self) -> Result<(WorkFile, Vec<u8>), WorkError> {
        self.content
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(b"<script>synthetic</script>".to_vec()))
            .map(|bytes| (file(), bytes))
    }
}
impl WorkRepository for Recorder {
    fn list_tasks(&self, _: VerifiedActor, _: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn task(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async { Err(WorkError::WorkItemNotFound) })
    }
    fn artifact(&self, _: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async move { Ok(artifact(id)) })
    }
    fn snapshot(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async { Err(WorkError::WorkArtifactNotFound) })
    }
    fn return_instruction(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async { Err(WorkError::WorkArtifactNotFound) })
    }
    fn execute(&self, _: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        self.commands.lock().unwrap().push(command);
        Box::pin(async {
            Ok(MutationResult::ArtifactDiscarded {
                task: summary(),
                artifact_id: Uuid::nil(),
            })
        })
    }
    fn recover(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::WorkItemNotFound) })
    }
    fn write_artifact_content(
        &self,
        _: VerifiedActor,
        artifact_id: Uuid,
        context: CommandContext,
        expected_artifact_revision: i64,
        bytes: Vec<u8>,
    ) -> WorkFuture<'_, MutationResult> {
        self.writes.lock().unwrap().push((
            artifact_id,
            context,
            expected_artifact_revision,
            bytes.len(),
        ));
        Box::pin(async move {
            Ok(MutationResult::ArtifactContentWritten {
                task: summary(),
                artifact: artifact(artifact_id),
            })
        })
    }
    fn artifact_content(
        &self,
        _: VerifiedActor,
        artifact_id: Uuid,
    ) -> WorkFuture<'_, (WorkFile, Vec<u8>)> {
        self.reads.lock().unwrap().push((None, artifact_id));
        let result = self.content();
        Box::pin(async move { result })
    }
    fn snapshot_content(
        &self,
        _: VerifiedActor,
        snapshot_id: Uuid,
        artifact_id: Uuid,
    ) -> WorkFuture<'_, (WorkFile, Vec<u8>)> {
        self.reads
            .lock()
            .unwrap()
            .push((Some(snapshot_id), artifact_id));
        let result = self.content();
        Box::pin(async move { result })
    }
}
async fn send(
    repository: &Arc<Recorder>,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let app = work_api_http::router(repository.clone(), VerifiedActor::Sales01);
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, body)
}
fn upload(id: Uuid, bytes: Vec<u8>, operation: Uuid) -> Request<Body> {
    Request::put(format!("/v1/organization/working-artifacts/{id}/content"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header("x-operation-id", operation.to_string())
        .header("x-expected-revision", "4")
        .header("x-acting-assignment-id", SALES_ASSIGNMENT_ID.to_string())
        .header("x-expected-artifact-revision", "1")
        .body(Body::from(bytes))
        .unwrap()
}
fn code(body: &[u8]) -> Value {
    serde_json::from_slice::<Value>(body).unwrap()["code"].clone()
}

#[tokio::test]
async fn content_upload_accepts_exactly_the_file_profile_with_header_identity() {
    let repository = Arc::new(Recorder::default());
    let id = Uuid::now_v7();
    let operation = Uuid::now_v7();
    let max = MAX_FILE_BYTES as usize;
    let (status, _, body) = send(&repository, upload(id, vec![1; max], operation)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let writes = repository.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    let (artifact_id, context, expected, size) = &writes[0];
    assert_eq!((*artifact_id, *expected, *size), (id, 1, max));
    assert_eq!(
        context,
        &CommandContext {
            operation_id: operation,
            expected_revision: 4,
            acting_assignment_id: SALES_ASSIGNMENT_ID
        }
    );
    // One byte over, an empty body, a missing header or another media type never
    // reach the repository.
    let mut missing = upload(id, vec![1], operation);
    missing.headers_mut().remove("x-expected-artifact-revision");
    let mut html = upload(id, vec![1], operation);
    html.headers_mut()
        .insert(header::CONTENT_TYPE, "text/html".parse().unwrap());
    for request in [
        upload(id, vec![1; max + 1], operation),
        upload(id, vec![], operation),
        missing,
        html,
    ] {
        let (status, _, body) = send(&repository, request).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(code(&body), json!("VALIDATION_FAILED"));
    }
    assert_eq!(repository.writes.lock().unwrap().len(), 1);
    // Other Work routes keep the JSON profile.
    let (status, _, _) = send(
        &repository,
        Request::post(format!(
            "/v1/organization/tasks/{SALES_TASK_ID}/working-artifacts"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn content_is_an_opaque_attachment_and_an_unverified_store_is_unavailable() {
    let repository = Arc::new(Recorder::default());
    let id = Uuid::now_v7();
    let (status, headers, body) = send(
        &repository,
        Request::get(format!("/v1/organization/working-artifacts/{id}/content"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"<script>synthetic</script>");
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"__ ___1_.txt\"; filename*=UTF-8''%E5%90%88%E6%88%90%20%E8%B3%87%E6%96%99%221%22.txt"
    );
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    // A full-size file passes the transport boundary; the snapshot route binds both IDs.
    *repository.content.lock().unwrap() = Some(Ok(vec![9; MAX_FILE_BYTES as usize]));
    let snapshot = Uuid::now_v7();
    let (status, _, body) = send(
        &repository,
        Request::get(format!(
            "/v1/organization/handoff-snapshots/{snapshot}/artifacts/{id}/content"
        ))
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.len(), MAX_FILE_BYTES as usize);
    assert_eq!(
        repository.reads.lock().unwrap().clone(),
        [(None, id), (Some(snapshot), id)]
    );
    for (error, status, expected) in [
        (
            WorkError::WorkArtifactUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "WORK_ARTIFACT_UNAVAILABLE",
        ),
        (
            WorkError::WorkArtifactNotFound,
            StatusCode::NOT_FOUND,
            "WORK_ARTIFACT_NOT_FOUND",
        ),
    ] {
        *repository.content.lock().unwrap() = Some(Err(error));
        let (actual, headers, body) = send(
            &repository,
            Request::get(format!("/v1/organization/working-artifacts/{id}/content"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(actual, status);
        assert_eq!(code(&body), json!(expected));
        assert!(headers.get(header::CONTENT_DISPOSITION).is_none());
    }
}

#[tokio::test]
async fn file_records_discard_and_import_map_to_their_commands_only() {
    let repository = Arc::new(Recorder::default());
    let json_post = |uri: String, body: Value| {
        Request::post(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let context = json!({"operationId": Uuid::now_v7(), "expectedRevision": 2, "actingAssignmentId": SALES_ASSIGNMENT_ID});
    let mut create = context.clone();
    create["file"] = json!({"fileName": "合成.pdf", "mediaType": "application/pdf"});
    let (status, _, _) = send(
        &repository,
        json_post(
            format!("/v1/organization/tasks/{SALES_TASK_ID}/working-artifacts"),
            create.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // A body naming both a text value and a file, or an unknown key, is refused.
    let mut both = create.clone();
    both["value"] = json!({"text": "本文"});
    let mut extra = create;
    extra["path"] = json!("/home/synthetic/合成.pdf");
    for body in [both, extra] {
        let (status, _, _) = send(
            &repository,
            json_post(
                format!("/v1/organization/tasks/{SALES_TASK_ID}/working-artifacts"),
                body,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let artifact_id = Uuid::now_v7();
    let mut discard = context.clone();
    discard["expectedArtifactRevision"] = json!(1);
    send(
        &repository,
        json_post(
            format!("/v1/organization/working-artifacts/{artifact_id}/discard"),
            discard,
        ),
    )
    .await;
    let snapshot_id = Uuid::now_v7();
    let mut import = context;
    import["expectedAttemptId"] = json!(SALES_ATTEMPT_ID);
    import["snapshotId"] = json!(snapshot_id);
    send(
        &repository,
        json_post(
            format!("/v1/organization/tasks/{SALES_TASK_ID}/working-artifacts/import"),
            import,
        ),
    )
    .await;
    let commands = repository.commands.lock().unwrap().clone();
    assert!(matches!(
        &commands[..],
        [
            Command::CreateFileArtifact { file_name, media_type, .. },
            Command::DiscardArtifact { artifact_id: discarded, task_id: SALES_TASK_ID, expected_artifact_revision: 1, .. },
            Command::ImportSubmission { snapshot_id: imported, expected_attempt_id: SALES_ATTEMPT_ID, .. },
        ] if file_name == "合成.pdf" && media_type == "application/pdf" && *discarded == artifact_id && *imported == snapshot_id
    ));
}
