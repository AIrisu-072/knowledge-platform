use time::OffsetDateTime;

use crate::{DocumentVersionId, DomainError, FileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn from_slice(value: &[u8]) -> Result<Self, DomainError> {
        let bytes: [u8; 32] = value
            .try_into()
            .map_err(|_| DomainError::InvalidContentHash)?;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileSize(i64);

impl FileSize {
    pub fn new(value: i64) -> Result<Self, DomainError> {
        if value >= 0 {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidFileSize)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey(String);

impl StorageKey {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() || value.starts_with('/') || value.starts_with('\\') {
            return Err(DomainError::InvalidStorageKey);
        }

        if value
            .split(['/', '\\'])
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Err(DomainError::InvalidStorageKey);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaType(String);

impl MediaType {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(DomainError::BlankMediaType);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFileDescriptor {
    storage_key: StorageKey,
    content_hash: ContentHash,
    size_bytes: FileSize,
    media_type: MediaType,
}

impl StoredFileDescriptor {
    pub fn new(
        storage_key: StorageKey,
        content_hash: ContentHash,
        size_bytes: FileSize,
        media_type: MediaType,
    ) -> Self {
        Self {
            storage_key,
            content_hash,
            size_bytes,
            media_type,
        }
    }

    pub fn storage_key(&self) -> &StorageKey {
        &self.storage_key
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    pub const fn size_bytes(&self) -> FileSize {
        self.size_bytes
    }

    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    pub(crate) fn into_parts(self) -> (StorageKey, ContentHash, FileSize, MediaType) {
        (
            self.storage_key,
            self.content_hash,
            self.size_bytes,
            self.media_type,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Primary,
    Attachment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObject {
    file_id: FileId,
    content_hash: ContentHash,
    media_type: MediaType,
    size_bytes: FileSize,
    storage_key: StorageKey,
    created_at: OffsetDateTime,
}

impl FileObject {
    pub(crate) fn new(
        file_id: FileId,
        stored_file: StoredFileDescriptor,
        created_at: OffsetDateTime,
    ) -> Self {
        let (storage_key, content_hash, size_bytes, media_type) = stored_file.into_parts();
        Self {
            file_id,
            content_hash,
            media_type,
            size_bytes,
            storage_key,
            created_at,
        }
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    pub const fn size_bytes(&self) -> FileSize {
        self.size_bytes
    }

    pub fn storage_key(&self) -> &StorageKey {
        &self.storage_key
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFile {
    document_version_id: DocumentVersionId,
    file_id: FileId,
    role: FileRole,
    ordinal: u32,
    original_filename: String,
}

impl VersionFile {
    pub(crate) fn primary(
        document_version_id: DocumentVersionId,
        file_id: FileId,
        original_filename: String,
    ) -> Self {
        Self {
            document_version_id,
            file_id,
            role: FileRole::Primary,
            ordinal: 0,
            original_filename,
        }
    }

    pub const fn document_version_id(&self) -> DocumentVersionId {
        self.document_version_id
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub const fn role(&self) -> FileRole {
        self.role
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub fn original_filename(&self) -> &str {
        &self.original_filename
    }
}
