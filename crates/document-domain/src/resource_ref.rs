use serde::{Deserialize, Serialize};

use crate::{DocumentId, FolderId, PolicyId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceRef {
    Document(DocumentId),
    Folder(FolderId),
    AccessPolicy(PolicyId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyTarget {
    Document(DocumentId),
    Folder(FolderId),
}

impl From<PolicyTarget> for ResourceRef {
    fn from(target: PolicyTarget) -> Self {
        match target {
            PolicyTarget::Document(id) => Self::Document(id),
            PolicyTarget::Folder(id) => Self::Folder(id),
        }
    }
}
