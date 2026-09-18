use serde_json::Value;
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, FromRow)]
pub(crate) struct PublishOperationRow {
    pub(crate) publish_operation_id: Uuid,
    pub(crate) document_id: Uuid,
    pub(crate) target_document_version_id: Uuid,
    pub(crate) expected_document_revision: i64,
    pub(crate) actor_identity_provider: String,
    pub(crate) actor_principal_id: String,
    pub(crate) published_at: OffsetDateTime,
    pub(crate) resulting_document_revision: i64,
    pub(crate) created_at: OffsetDateTime,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(crate) struct PublishCandidateRow {
    pub(crate) document_id: Uuid,
    pub(crate) folder_id: Uuid,
    pub(crate) current_version_id: Option<Uuid>,
    pub(crate) document_revision: i64,
    pub(crate) document_metadata: Value,
    pub(crate) document_created_at: OffsetDateTime,
    pub(crate) document_version_id: Uuid,
    pub(crate) version_no: i64,
    pub(crate) lifecycle_state: String,
    pub(crate) title: String,
    pub(crate) revision_reason: Option<String>,
    pub(crate) approved_at: Option<OffsetDateTime>,
    pub(crate) scheduled_publish_at: Option<OffsetDateTime>,
    pub(crate) published_at: Option<OffsetDateTime>,
    pub(crate) withdrawn_at: Option<OffsetDateTime>,
    pub(crate) effective_from: Option<OffsetDateTime>,
    pub(crate) effective_to: Option<OffsetDateTime>,
    pub(crate) created_by_identity_provider: String,
    pub(crate) created_by_principal_id: String,
    pub(crate) version_metadata: Value,
    pub(crate) version_created_at: OffsetDateTime,
    pub(crate) file_id: Uuid,
    pub(crate) content_hash: Vec<u8>,
    pub(crate) media_type: String,
    pub(crate) size_bytes: i64,
    pub(crate) storage_locator: String,
    pub(crate) file_created_at: OffsetDateTime,
    pub(crate) role: String,
    pub(crate) ordinal: i32,
    pub(crate) original_filename: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(crate) struct LockedPublishStateRow {
    pub(crate) document_id: Uuid,
    pub(crate) current_version_id: Option<Uuid>,
    pub(crate) document_revision: i64,
    pub(crate) document_version_id: Uuid,
    pub(crate) version_document_id: Uuid,
    pub(crate) lifecycle_state: String,
    pub(crate) published_at: Option<OffsetDateTime>,
}
