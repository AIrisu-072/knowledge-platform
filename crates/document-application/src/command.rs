use document_domain::{FileId, FolderId, MediaType, Metadata, PrincipalRef};

use crate::ContentReader;

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
    document_id: document_domain::DocumentId,
    document_version_id: document_domain::DocumentVersionId,
    file_id: FileId,
}

impl CreateDocumentResult {
    pub(crate) const fn new(
        document_id: document_domain::DocumentId,
        document_version_id: document_domain::DocumentVersionId,
        file_id: FileId,
    ) -> Self {
        Self {
            document_id,
            document_version_id,
            file_id,
        }
    }

    pub const fn document_id(&self) -> document_domain::DocumentId {
        self.document_id
    }

    pub const fn document_version_id(&self) -> document_domain::DocumentVersionId {
        self.document_version_id
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }
}
