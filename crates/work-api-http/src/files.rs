//! Private work files (U3). Binary bodies are bounded by the file profile, and
//! operation identity travels in headers; content is always an untrusted
//! attachment, never rendered.
use super::*;
use axum::{
    body::Bytes,
    extract::rejection::BytesRejection,
    http::{HeaderMap, header::HeaderName},
};

/// The file profile: 8 MiB of content plus nothing else.
pub(super) const MAX_CONTENT_BYTES: usize = MAX_FILE_BYTES as usize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileLabel {
    file_name: String,
    media_type: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileArtifactBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    file: FileLabel,
}
impl FileArtifactBody {
    pub(super) fn command(self, task_id: Uuid) -> Command {
        Command::CreateFileArtifact {
            task_id,
            context: CommandContext {
                operation_id: self.operation_id,
                expected_revision: self.expected_revision,
                acting_assignment_id: self.acting_assignment_id,
            },
            file_name: self.file.file_name,
            media_type: self.file.media_type,
        }
    }
}
fn header<T: std::str::FromStr>(headers: &HeaderMap, name: &'static str) -> Result<T, Problem> {
    headers
        .get(HeaderName::from_static(name))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .ok_or(Problem(WorkError::ValidationFailed))
}
pub(super) async fn write_content(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let artifact_id = path_id(path)?;
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/octet-stream")
    {
        return Err(Problem(WorkError::ValidationFailed));
    }
    let context = CommandContext {
        operation_id: header(&headers, "x-operation-id")?,
        expected_revision: header(&headers, "x-expected-revision")?,
        acting_assignment_id: header(&headers, "x-acting-assignment-id")?,
    };
    let expected_artifact_revision = header(&headers, "x-expected-artifact-revision")?;
    let bytes = body.map_err(|_| Problem(WorkError::ValidationFailed))?;
    if bytes.is_empty() || bytes.len() > MAX_CONTENT_BYTES {
        return Err(Problem(WorkError::ValidationFailed));
    }
    Ok(Json(
        state
            .repository
            .write_artifact_content(
                state.actor,
                artifact_id,
                context,
                expected_artifact_revision,
                bytes.to_vec(),
            )
            .await?,
    ))
}
pub(super) async fn read_content(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Response, Problem> {
    let (file, bytes) = state
        .repository
        .artifact_content(state.actor, path_id(path)?)
        .await?;
    Ok(attachment(&file, bytes))
}
pub(super) async fn read_snapshot_content(
    State(state): State<ApiState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Response, Problem> {
    let Path((snapshot_id, artifact_id)) =
        path.map_err(|_| Problem(WorkError::ValidationFailed))?;
    let (file, bytes) = state
        .repository
        .snapshot_content(state.actor, snapshot_id, artifact_id)
        .await?;
    Ok(attachment(&file, bytes))
}
/// Always an opaque download: the declared media type is metadata only.
fn attachment(file: &WorkFile, bytes: Vec<u8>) -> Response {
    let fallback: String = file
        .file_name
        .chars()
        .map(|value| {
            if value.is_ascii_graphic() && value != '"' && value != '\\' || value == ' ' {
                value
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = file
        .file_name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    let disposition = format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}");
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    match HeaderValue::from_str(&disposition) {
        Ok(value) => {
            headers.insert(header::CONTENT_DISPOSITION, value);
            response
        }
        Err(_) => Problem(WorkError::IntegrityViolation).into_response(),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DiscardBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_artifact_revision: i64,
}
pub(super) async fn discard(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<DiscardBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let artifact_id = path_id(path)?;
    let body = json_body(body)?;
    let artifact = state.repository.artifact(state.actor, artifact_id).await?;
    let command = Command::DiscardArtifact {
        task_id: artifact.task_id,
        artifact_id,
        context: CommandContext {
            operation_id: body.operation_id,
            expected_revision: body.expected_revision,
            acting_assignment_id: body.acting_assignment_id,
        },
        expected_artifact_revision: body.expected_artifact_revision,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ImportBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    snapshot_id: Uuid,
}
pub(super) async fn import_submission(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<ImportBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let command = Command::ImportSubmission {
        task_id: path_id(path)?,
        context: CommandContext {
            operation_id: body.operation_id,
            expected_revision: body.expected_revision,
            acting_assignment_id: body.acting_assignment_id,
        },
        expected_attempt_id: body.expected_attempt_id,
        snapshot_id: body.snapshot_id,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
