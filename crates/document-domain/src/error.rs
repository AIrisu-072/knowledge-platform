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
}
