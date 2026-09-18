use document_domain::{
    DocumentId, DocumentVersionId, FileId, FolderId, MediaType, Metadata, PrincipalRef,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ContentReader, error::ApplicationError};

pub struct CreateDocumentCommand {
    pub folder_id: FolderId,
    pub title: String,
    pub document_metadata: Metadata,
    pub version_metadata: Metadata,
    pub principal: PrincipalRef,
    pub original_filename: String,
    pub media_type: MediaType,
    pub content: ContentReader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateDocumentResult {
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    file_id: FileId,
}

impl CreateDocumentResult {
    pub(crate) const fn new(
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
        file_id: FileId,
    ) -> Self {
        Self {
            document_id,
            document_version_id,
            file_id,
        }
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn document_version_id(&self) -> DocumentVersionId {
        self.document_version_id
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublishOperationId(Uuid);

impl PublishOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() == 7 {
            Ok(Self(value))
        } else {
            Err(ApplicationError::Validation(
                "publish operation id must be UUIDv7".to_owned(),
            ))
        }
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishDocumentCommand {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
}

impl PublishDocumentCommand {
    pub fn new(
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        target_document_version_id: DocumentVersionId,
        expected_document_revision: i64,
        principal: PrincipalRef,
    ) -> Result<Self, ApplicationError> {
        if expected_document_revision < 0 {
            return Err(ApplicationError::Validation(
                "expected document revision cannot be negative".to_owned(),
            ));
        }

        Ok(Self {
            publish_operation_id,
            document_id,
            target_document_version_id,
            expected_document_revision,
            principal,
        })
    }

    pub const fn publish_operation_id(&self) -> PublishOperationId {
        self.publish_operation_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn target_document_version_id(&self) -> DocumentVersionId {
        self.target_document_version_id
    }

    pub const fn expected_document_revision(&self) -> i64 {
        self.expected_document_revision
    }

    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishDocumentResult {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    resulting_document_revision: i64,
    published_at: OffsetDateTime,
}

impl PublishDocumentResult {
    pub fn from_persisted(
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
        resulting_document_revision: i64,
        published_at: OffsetDateTime,
    ) -> Self {
        Self {
            publish_operation_id,
            document_id,
            document_version_id,
            resulting_document_revision,
            published_at,
        }
    }

    pub const fn publish_operation_id(&self) -> PublishOperationId {
        self.publish_operation_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn document_version_id(&self) -> DocumentVersionId {
        self.document_version_id
    }

    pub const fn resulting_document_revision(&self) -> i64 {
        self.resulting_document_revision
    }

    pub const fn published_at(&self) -> OffsetDateTime {
        self.published_at
    }
}
