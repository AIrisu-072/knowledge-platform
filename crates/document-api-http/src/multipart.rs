use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::Multipart;
use axum::http::HeaderMap;
use document_application::ContentReader;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};
use uuid::Uuid;

use crate::limits::UploadLimits;

pub(crate) struct InitialUpload {
    pub request_json: Vec<u8>,
    pub original_filename: String,
    pub media_type: String,
    pub content: ContentReader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MultipartFailure {
    Validation,
    UnsupportedMediaType,
    Internal,
}

pub(crate) async fn parse_initial_upload(
    mut multipart: Multipart,
    limits: UploadLimits,
) -> Result<InitialUpload, MultipartFailure> {
    let mut request_json = None;
    let mut file = None;
    let mut part_count = 0_usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| MultipartFailure::Validation)?
    {
        part_count = part_count
            .checked_add(1)
            .ok_or(MultipartFailure::Validation)?;
        if part_count > limits.parts || header_bytes(field.headers()) > limits.header_bytes {
            return Err(MultipartFailure::Validation);
        }
        let name = field.name().ok_or(MultipartFailure::Validation)?.to_owned();
        match name.as_str() {
            "request" => {
                if request_json.is_some() || !is_json(field.content_type()) {
                    return Err(MultipartFailure::Validation);
                }
                request_json = Some(read_bounded(&mut field, limits.json_bytes).await?);
            }
            "file" => {
                if file.is_some() {
                    return Err(MultipartFailure::Validation);
                }
                let filename = field
                    .file_name()
                    .ok_or(MultipartFailure::Validation)?
                    .to_owned();
                if filename.trim().is_empty() || filename.len() > limits.filename_bytes {
                    return Err(MultipartFailure::Validation);
                }
                let media_type = field
                    .content_type()
                    .filter(|value| valid_media_type(value))
                    .ok_or(MultipartFailure::UnsupportedMediaType)?
                    .to_owned();
                let content = spool_bounded(&mut field, limits.file_bytes).await?;
                file = Some((filename, media_type, content));
            }
            _ => return Err(MultipartFailure::Validation),
        }
    }
    let request_json = request_json.ok_or(MultipartFailure::Validation)?;
    let (original_filename, media_type, content) = file.ok_or(MultipartFailure::Validation)?;
    Ok(InitialUpload {
        request_json,
        original_filename,
        media_type,
        content,
    })
}

fn header_bytes(headers: &HeaderMap) -> usize {
    headers
        .iter()
        .try_fold(0_usize, |total, (name, value)| {
            total
                .checked_add(name.as_str().len())?
                .checked_add(value.as_bytes().len())?
                .checked_add(4)
        })
        .unwrap_or(usize::MAX)
}

pub(crate) fn request_header_bytes(headers: &HeaderMap) -> usize {
    header_bytes(headers)
}

async fn read_bounded(
    field: &mut axum::extract::multipart::Field<'_>,
    limit: usize,
) -> Result<Vec<u8>, MultipartFailure> {
    let mut output = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| MultipartFailure::Validation)?
    {
        let new_len = output
            .len()
            .checked_add(chunk.len())
            .ok_or(MultipartFailure::Validation)?;
        if new_len > limit {
            return Err(MultipartFailure::Validation);
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

async fn spool_bounded(
    field: &mut axum::extract::multipart::Field<'_>,
    limit: usize,
) -> Result<ContentReader, MultipartFailure> {
    let (guard, mut writer) = create_spool()
        .await
        .map_err(|_| MultipartFailure::Internal)?;
    let mut written = 0_usize;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| MultipartFailure::Validation)?
    {
        written = written
            .checked_add(chunk.len())
            .ok_or(MultipartFailure::Validation)?;
        if written > limit {
            return Err(MultipartFailure::Validation);
        }
        writer
            .write_all(&chunk)
            .await
            .map_err(|_| MultipartFailure::Internal)?;
    }
    writer
        .flush()
        .await
        .map_err(|_| MultipartFailure::Internal)?;
    drop(writer);
    let path = guard.into_path();
    let file = match File::open(&path).await {
        Ok(file) => file,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            return Err(MultipartFailure::Internal);
        }
    };
    Ok(Box::pin(TemporaryUploadReader { file, path }))
}

async fn create_spool() -> io::Result<(SpoolGuard, File)> {
    for _ in 0..8 {
        let path =
            std::env::temp_dir().join(format!("knowledge-platform-upload-{}.part", Uuid::now_v7()));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
        {
            Ok(file) => return Ok((SpoolGuard(Some(path)), file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate upload spool",
    ))
}

struct SpoolGuard(Option<PathBuf>);

impl SpoolGuard {
    fn into_path(mut self) -> PathBuf {
        self.0.take().expect("spool path exists")
    }
}

impl Drop for SpoolGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct TemporaryUploadReader {
    file: File,
    path: PathBuf,
}

impl AsyncRead for TemporaryUploadReader {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().file).poll_read(context, buffer)
    }
}

impl Drop for TemporaryUploadReader {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn is_json(value: Option<&str>) -> bool {
    value
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn valid_media_type(value: &str) -> bool {
    let mut segments = value.split(';');
    let Some(essence) = segments.next().map(str::trim) else {
        return false;
    };
    let Some((top, subtype)) = essence.split_once('/') else {
        return false;
    };
    if subtype.contains('/') || !valid_token(top) || !valid_token(subtype) {
        return false;
    }
    segments.all(|parameter| {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            return false;
        };
        valid_token(name.trim()) && !value.trim().is_empty()
    })
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}
