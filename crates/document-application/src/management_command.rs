use std::collections::{BTreeMap, BTreeSet};

use document_domain::{DocumentId, FolderId, PolicyId, PolicyMode, PolicyTarget, ResourceRef};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ApplicationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ManagementOperationId(Uuid);

impl ManagementOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() != 7 {
            return Err(ApplicationError::Validation(
                "management operation id must be UUIDv7".into(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementErrorCode {
    InvalidInput,
    NotFound,
    Forbidden,
    RevisionConflict,
    OperationConflict,
    CursorStale,
    StaleVersion,
    ReservedDocument,
    FolderCycle,
    RootProtected,
    IdentityUnavailable,
    CommitOutcomeUnknown,
    IntegrityViolation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ManagementCommand {
    UpdateDocumentMetadata {
        operation_id: ManagementOperationId,
        document_id: DocumentId,
        expected_document_revision: i64,
        set: BTreeMap<String, Value>,
        unset: BTreeSet<String>,
        reason: String,
    },
    MoveDocument {
        operation_id: ManagementOperationId,
        document_id: DocumentId,
        from_folder_id: FolderId,
        to_folder_id: FolderId,
        expected_document_revision: i64,
        reason: String,
    },
    CreateFolder {
        operation_id: ManagementOperationId,
        folder_id: FolderId,
        parent_folder_id: FolderId,
        expected_parent_revision: i64,
        name: String,
        reason: String,
    },
    RenameFolder {
        operation_id: ManagementOperationId,
        folder_id: FolderId,
        expected_folder_revision: i64,
        name: String,
        reason: String,
    },
    MoveFolder {
        operation_id: ManagementOperationId,
        folder_id: FolderId,
        from_parent_id: FolderId,
        to_parent_id: FolderId,
        expected_folder_revision: i64,
        reason: String,
    },
    SetAccessPolicy {
        operation_id: ManagementOperationId,
        target: PolicyTarget,
        expected_policy_revision: i64,
        mode: PolicyMode,
        reason: String,
    },
}

impl ManagementCommand {
    pub const fn operation_id(&self) -> ManagementOperationId {
        match self {
            Self::UpdateDocumentMetadata { operation_id, .. }
            | Self::MoveDocument { operation_id, .. }
            | Self::CreateFolder { operation_id, .. }
            | Self::RenameFolder { operation_id, .. }
            | Self::MoveFolder { operation_id, .. }
            | Self::SetAccessPolicy { operation_id, .. } => *operation_id,
        }
    }

    pub const fn operation_kind(&self) -> &'static str {
        match self {
            Self::UpdateDocumentMetadata { .. } => "update_document_metadata",
            Self::MoveDocument { .. } => "move_document",
            Self::CreateFolder { .. } => "create_folder",
            Self::RenameFolder { .. } => "rename_folder",
            Self::MoveFolder { .. } => "move_folder",
            Self::SetAccessPolicy { .. } => "set_access_policy",
        }
    }

    pub const fn resource(&self) -> ResourceRef {
        match self {
            Self::UpdateDocumentMetadata { document_id, .. }
            | Self::MoveDocument { document_id, .. } => ResourceRef::Document(*document_id),
            Self::CreateFolder { folder_id, .. }
            | Self::RenameFolder { folder_id, .. }
            | Self::MoveFolder { folder_id, .. } => ResourceRef::Folder(*folder_id),
            Self::SetAccessPolicy { target, .. } => match target {
                PolicyTarget::Document(id) => ResourceRef::Document(*id),
                PolicyTarget::Folder(id) => ResourceRef::Folder(*id),
            },
        }
    }

    pub const fn expected_revision(&self) -> i64 {
        match self {
            Self::UpdateDocumentMetadata {
                expected_document_revision,
                ..
            }
            | Self::MoveDocument {
                expected_document_revision,
                ..
            } => *expected_document_revision,
            Self::CreateFolder {
                expected_parent_revision,
                ..
            } => *expected_parent_revision,
            Self::RenameFolder {
                expected_folder_revision,
                ..
            }
            | Self::MoveFolder {
                expected_folder_revision,
                ..
            } => *expected_folder_revision,
            Self::SetAccessPolicy {
                expected_policy_revision,
                ..
            } => *expected_policy_revision,
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::UpdateDocumentMetadata { reason, .. }
            | Self::MoveDocument { reason, .. }
            | Self::CreateFolder { reason, .. }
            | Self::RenameFolder { reason, .. }
            | Self::MoveFolder { reason, .. }
            | Self::SetAccessPolicy { reason, .. } => reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagementMoveDetails {
    pub from_folder_id: FolderId,
    pub to_folder_id: FolderId,
    pub subtree_affected: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagementMutationResult {
    pub operation_id: ManagementOperationId,
    pub resource: ResourceRef,
    pub resulting_revision: i64,
    pub access_revision: Option<i64>,
    pub policy_id: Option<PolicyId>,
    pub changed: bool,
    pub occurred_at: OffsetDateTime,
    pub document_metadata: Option<Value>,
    pub movement: Option<ManagementMoveDetails>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ManagementResult {
    MetadataUpdate(ManagementMutationResult),
    DocumentMove(ManagementMutationResult),
    FolderMutation(ManagementMutationResult),
    PolicyMutation(ManagementMutationResult),
}
