//! Identity resolution keeps similarity separate from strong evidence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityState {
    Resolved,
    Provisional,
    Unresolved,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityEvidenceKind {
    StableProviderId,
    SourceNativeId,
    CanonicalUri,
    IntegrityDigest,
    ExplicitDeclaration,
    SchemaAndContentIdentity,
    NameSimilarity,
    SchemaSimilarity,
    LlmInference,
}

impl IdentityEvidenceKind {
    const fn is_strong(self) -> bool {
        matches!(
            self,
            Self::StableProviderId
                | Self::SourceNativeId
                | Self::CanonicalUri
                | Self::IntegrityDigest
                | Self::ExplicitDeclaration
                | Self::SchemaAndContentIdentity
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEvidence {
    pub kind: IdentityEvidenceKind,
    pub supports_same_resource: bool,
    pub evidence_ref: Option<String>,
}

impl IdentityEvidence {
    pub const fn new(kind: IdentityEvidenceKind, supports_same_resource: bool) -> Self {
        Self {
            kind,
            supports_same_resource,
            evidence_ref: None,
        }
    }
}

pub fn resolve_identity(evidence: &[IdentityEvidence]) -> IdentityState {
    let supports_strong = evidence
        .iter()
        .any(|item| item.kind.is_strong() && item.supports_same_resource);
    let rejects_strong = evidence
        .iter()
        .any(|item| item.kind.is_strong() && !item.supports_same_resource);
    if supports_strong && rejects_strong {
        IdentityState::Conflict
    } else if supports_strong {
        IdentityState::Resolved
    } else if rejects_strong {
        IdentityState::Unresolved
    } else if evidence.iter().any(|item| item.supports_same_resource) {
        IdentityState::Provisional
    } else {
        IdentityState::Unresolved
    }
}
