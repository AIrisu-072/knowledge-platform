use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use document_application::{
    ApplicationError, ContentReader, FileStorage, VerifiedActorContext,
    VersionFileAccessRepository, VersionFileAccessService, VersionFileRequest, VersionPurpose,
    VersionRequest,
};
use document_domain::{DocumentId, DocumentVersionId};
use http_body::{Frame, SizeHint};
use serde::Deserialize;
use tokio::io::ReadBuf;
use uuid::Uuid;

use crate::error::{ApiError, ApiProblem, ErrorCode};
use crate::identity::IdentityAdapter;
use crate::limits::{DOWNLOAD_IDLE_TIMEOUT, ORDINARY_OPERATION_TIMEOUT};
use crate::management::{problem, validation};
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

pub trait FileDownloadRepository: VersionFileAccessRepository + Send + Sync + 'static {}

impl<T> FileDownloadRepository for T where T: VersionFileAccessRepository + Send + Sync + 'static {}

struct FileDownloadState<R, F> {
    repository: Arc<R>,
    storage: Arc<F>,
    idle_timeout: Duration,
}

impl<R, F> Clone for FileDownloadState<R, F> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            storage: self.storage.clone(),
            idle_timeout: self.idle_timeout,
        }
    }
}

pub fn file_download_router<R, F>(
    repository: Arc<R>,
    storage: Arc<F>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError>
where
    R: FileDownloadRepository,
    F: FileStorage + 'static,
{
    file_download_router_with_idle_timeout(
        repository,
        storage,
        identity_adapter,
        DOWNLOAD_IDLE_TIMEOUT,
    )
}

pub fn file_download_router_with_idle_timeout<R, F>(
    repository: Arc<R>,
    storage: Arc<F>,
    identity_adapter: Arc<dyn IdentityAdapter>,
    idle_timeout: Duration,
) -> Result<Router, StartupError>
where
    R: FileDownloadRepository,
    F: FileStorage + 'static,
{
    let route = with_operation_timeout(
        Router::new()
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/files/{content_item_id}/{representation_id}",
            get(download::<R, F>),
        )
        .with_state(FileDownloadState {
            repository,
            storage,
            idle_timeout,
        }),
        ORDINARY_OPERATION_TIMEOUT,
    );
    protect_routes(route, Some(identity_adapter))
}

#[derive(Debug, Deserialize)]
struct DownloadQuery {
    purpose: Option<String>,
}

async fn download<R, F>(
    State(state): State<FileDownloadState<R, F>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id, content_item_id, representation_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    query: Result<Query<DownloadQuery>, QueryRejection>,
) -> Result<Response, ApiError>
where
    R: FileDownloadRepository,
    F: FileStorage + 'static,
{
    if headers.contains_key(header::RANGE) {
        return Ok((StatusCode::RANGE_NOT_SATISFIABLE, Body::empty()).into_response());
    }
    let path = format!(
        "/v1/documents/{document_id}/versions/{version_id}/files/{content_item_id}/{representation_id}"
    );
    let params = query
        .map(|Query(value)| value)
        .map_err(|_| problem(validation("invalid download query"), &path, &trace))?;
    let request = VersionFileRequest {
        version: VersionRequest {
            document_id: document_id_value(&document_id)
                .map_err(|error| problem(error, &path, &trace))?,
            document_version_id: version_id_value(&version_id)
                .map_err(|error| problem(error, &path, &trace))?,
            purpose: purpose(params.purpose.as_deref())
                .map_err(|error| problem(error, &path, &trace))?,
        },
        content_item_id: uuid_value(&content_item_id, "contentItemId")
            .map_err(|error| problem(error, &path, &trace))?,
        representation_id: uuid_value(&representation_id, "representationId")
            .map_err(|error| problem(error, &path, &trace))?,
    };
    let opened = VersionFileAccessService::new(state.repository, state.storage)
        .open_version_file(&ctx, request)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let size_bytes = u64::try_from(opened.size_bytes)
        .map_err(|_| problem(ApplicationError::IntegrityViolation, &path, &trace))?;
    let media_type = HeaderValue::from_str(opened.media_type.as_str())
        .map_err(|_| problem(ApplicationError::IntegrityViolation, &path, &trace))?;
    let disposition = HeaderValue::from_str(&content_disposition(&opened.safe_display_name))
        .map_err(|_| problem(ApplicationError::IntegrityViolation, &path, &trace))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, size_bytes)
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::new(AsyncReadBody::new(
            opened.content,
            size_bytes,
            state.idle_timeout,
        )))
        .map_err(|_| ApiProblem::new(ErrorCode::Internal, &path, &trace.trace_id).into())
}

fn document_id_value(value: &str) -> Result<DocumentId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentId::from_uuid)
        .map_err(|_| validation("invalid documentId"))
}

fn version_id_value(value: &str) -> Result<DocumentVersionId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentVersionId::from_uuid)
        .map_err(|_| validation("invalid versionId"))
}

fn uuid_value(value: &str, name: &str) -> Result<Uuid, ApplicationError> {
    Uuid::parse_str(value).map_err(|_| validation(&format!("invalid {name}")))
}

fn purpose(value: Option<&str>) -> Result<VersionPurpose, ApplicationError> {
    match value.ok_or_else(|| validation("purpose is required"))? {
        "published" => Ok(VersionPurpose::Published),
        "authoring" => Ok(VersionPurpose::Authoring),
        "history" => Ok(VersionPurpose::History),
        _ => Err(validation("invalid purpose")),
    }
}

fn content_disposition(name: &str) -> String {
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .take(255)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let name = if name.trim().is_empty() || name == "." || name == ".." {
        "document.bin"
    } else {
        &name
    };
    format!("attachment; filename=\"{name}\"")
}

struct AsyncReadBody {
    reader: ContentReader,
    remaining: u64,
    idle_timeout: Duration,
    idle: Pin<Box<tokio::time::Sleep>>,
}

impl AsyncReadBody {
    fn new(reader: ContentReader, size_bytes: u64, idle_timeout: Duration) -> Self {
        Self {
            reader,
            remaining: size_bytes,
            idle_timeout,
            idle: Box::pin(tokio::time::sleep(idle_timeout)),
        }
    }
}

impl http_body::Body for AsyncReadBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        let chunk_size = usize::try_from(self.remaining.min(STREAM_CHUNK_BYTES as u64))
            .expect("bounded chunk size fits usize");
        let mut chunk = vec![0_u8; chunk_size];
        let mut read_buffer = ReadBuf::new(&mut chunk);
        match tokio::io::AsyncRead::poll_read(self.reader.as_mut(), cx, &mut read_buffer) {
            Poll::Pending => match self.idle.as_mut().poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(()) => Poll::Ready(Some(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "download stream idle timeout",
                )))),
            },
            Poll::Ready(Err(error)) => Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok(())) => {
                let read = read_buffer.filled().len();
                if read == 0 {
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "stored object ended before declared size",
                    ))));
                }
                chunk.truncate(read);
                self.remaining -= read as u64;
                let deadline = tokio::time::Instant::now() + self.idle_timeout;
                self.idle.as_mut().reset(deadline);
                Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk)))))
            }
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
mod tests {
    use super::content_disposition;

    #[test]
    fn disposition_is_ascii_and_header_safe() {
        let value = content_disposition("../../報告\"\r\nX-Test: value.pdf");
        assert!(value.starts_with("attachment; filename=\""));
        assert!(value.is_ascii());
        assert!(!value.contains(['\r', '\n', '\\']));
        assert!(!value.contains("../"));
    }
}
