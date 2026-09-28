use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("version number must be positive")]
    InvalidVersionNo,
    #[error("content hash must be exactly 32 bytes")]
    InvalidContentHash,
    #[error("file size cannot be negative")]
    InvalidFileSize,
    #[error("title cannot be blank")]
    BlankTitle,
    #[error("storage key must be a non-empty relative key without traversal")]
    InvalidStorageKey,
    #[error("identity provider cannot be blank")]
    BlankIdentityProvider,
    #[error("principal id cannot be blank")]
    BlankPrincipalId,
    #[error("media type cannot be blank")]
    BlankMediaType,
    #[error("original filename cannot be blank")]
    BlankOriginalFilename,
    #[error("document version belongs to another document")]
    VersionDocumentMismatch,
    #[error("document already has a current version")]
    CurrentVersionAlreadySet,
    #[error("document version is not working")]
    VersionNotWorking,
    #[error("document revision overflow")]
    RevisionOverflow,
    #[error("logical path must be a normalized relative path without ambiguous segments")]
    InvalidLogicalPath,
    #[error("semantic content manifest must contain at least one item")]
    InvalidContentManifest,
    #[error("content item key is duplicated")]
    DuplicateContentItemKey,
    #[error("semantic format identifier is invalid")]
    InvalidSemanticFormat,
    #[error("inspection profile identifier is invalid")]
    InvalidInspectionProfile,
    #[error("a current published version is required")]
    NoCurrentPublishedVersion,
    #[error("a working version already exists")]
    ExistingWorkingVersion,
    #[error("working version base does not match the current published version")]
    StaleVersionBase,
    #[error("document version is not published")]
    VersionNotPublished,
    #[error("withdrawal restoration candidate does not match the immediate published base")]
    InvalidRestorationCandidate,
    #[error("persisted document state is invalid")]
    InvalidPersistedState,
    #[error("policy subject issuer or id is invalid")]
    InvalidPolicySubject,
    #[error("policy grant must contain unique actions")]
    InvalidPolicyGrant,
    #[error("explicit policy must contain at least one grant")]
    EmptyExplicitPolicy,
    #[error("policy contains duplicate subject grants")]
    DuplicatePolicySubject,
}
