use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

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
}
