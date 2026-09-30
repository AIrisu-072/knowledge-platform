use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{ApplicationError, VerifiedActorContext};

const CURSOR_VERSION: u8 = 1;
const MAX_CURSOR_BYTES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Published,
    Authoring,
    History,
    Folders,
    Versions,
    DocumentHistory,
    DocumentRevisions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentSort {
    CreatedAtDesc,
    TitleAsc,
    PublishedAtDesc,
    RevisionNumberDesc,
}

impl DocumentSort {
    pub const fn as_sql(self) -> &'static str {
        match self {
            Self::CreatedAtDesc => "created_at_desc",
            Self::TitleAsc => "title_asc",
            Self::PublishedAtDesc => "published_at_desc",
            Self::RevisionNumberDesc => "revision_number_desc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorBinding {
    pub kind: QueryKind,
    pub sort: DocumentSort,
    pub filter_fingerprint: String,
    pub principal_fingerprint: String,
    pub access_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorPosition {
    pub document_id: Uuid,
    pub sort_time_micros: Option<i64>,
    pub sort_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_revision_key: Option<RevisionSortKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionSortKey {
    pub major: i64,
    pub minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorEnvelope {
    version: u8,
    binding: CursorBinding,
    position: CursorPosition,
}

pub fn validate_page_size(value: u16) -> Result<u16, ApplicationError> {
    if (1..=200).contains(&value) {
        Ok(value)
    } else {
        Err(ApplicationError::Validation(
            "page size must be within 1..=200".into(),
        ))
    }
}

pub fn fingerprint_json(value: &Value) -> Result<String, ApplicationError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|_| ApplicationError::Validation("invalid query context".into()))?;
    Ok(hex_encode(&Sha256::digest(encoded)))
}

pub fn principal_fingerprint(ctx: &VerifiedActorContext) -> Result<String, ApplicationError> {
    ctx.ensure_current()?;
    let mut subjects = ctx
        .subjects()
        .iter()
        .map(|subject| {
            serde_json::json!([
                format!("{:?}", subject.kind()),
                subject.identity_provider(),
                subject.subject_id(),
            ])
        })
        .collect::<Vec<_>>();
    subjects.sort_by_key(Value::to_string);
    fingerprint_json(&serde_json::json!({
        "identity_provider": ctx.principal().identity_provider(),
        "principal_id": ctx.principal().principal_id(),
        "subjects": subjects,
    }))
}

pub fn encode_cursor(
    binding: &CursorBinding,
    position: &CursorPosition,
) -> Result<String, ApplicationError> {
    validate_position(binding, position)?;
    let bytes = serde_json::to_vec(&CursorEnvelope {
        version: CURSOR_VERSION,
        binding: binding.clone(),
        position: position.clone(),
    })
    .map_err(|_| ApplicationError::Validation("invalid cursor state".into()))?;
    if bytes.len() > MAX_CURSOR_BYTES / 2 {
        return Err(ApplicationError::Validation("cursor too large".into()));
    }
    Ok(hex_encode(&bytes))
}

pub fn decode_cursor(
    token: &str,
    expected: &CursorBinding,
) -> Result<CursorPosition, ApplicationError> {
    if token.is_empty() || token.len() > MAX_CURSOR_BYTES || !token.len().is_multiple_of(2) {
        return Err(ApplicationError::Validation("invalid cursor length".into()));
    }
    let bytes = hex_decode(token)?;
    let envelope: CursorEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| ApplicationError::Validation("invalid cursor structure".into()))?;
    if envelope.version != CURSOR_VERSION {
        return Err(ApplicationError::Validation(
            "unsupported cursor version".into(),
        ));
    }
    validate_position(&envelope.binding, &envelope.position)?;
    if &envelope.binding != expected {
        return Err(ApplicationError::CursorStale);
    }
    Ok(envelope.position)
}

fn validate_position(
    binding: &CursorBinding,
    position: &CursorPosition,
) -> Result<(), ApplicationError> {
    let valid = if binding.kind == QueryKind::DocumentRevisions {
        binding.sort == DocumentSort::RevisionNumberDesc
            && position.sort_time_micros.is_none()
            && position.sort_title.is_none()
            && position
                .sort_revision_key
                .is_some_and(|key| key.major >= 1 && key.minor >= 0)
    } else {
        position.sort_revision_key.is_none()
            && match binding.sort {
                DocumentSort::TitleAsc => {
                    position.sort_title.is_some() && position.sort_time_micros.is_none()
                }
                DocumentSort::CreatedAtDesc | DocumentSort::PublishedAtDesc => {
                    position.sort_time_micros.is_some() && position.sort_title.is_none()
                }
                DocumentSort::RevisionNumberDesc => false,
            }
    };
    if valid {
        Ok(())
    } else {
        Err(ApplicationError::Validation(
            "cursor sort key does not match sort".into(),
        ))
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 15) as usize]));
    }
    output
}

fn hex_decode(token: &str) -> Result<Vec<u8>, ApplicationError> {
    let mut bytes = Vec::with_capacity(token.len() / 2);
    for pair in token.as_bytes().as_chunks::<2>().0 {
        let high = char::from(pair[0]).to_digit(16);
        let low = char::from(pair[1]).to_digit(16);
        match (high, low) {
            (Some(high), Some(low)) => bytes.push(((high << 4) | low) as u8),
            _ => return Err(ApplicationError::Validation("invalid cursor hex".into())),
        }
    }
    Ok(bytes)
}
