use time::OffsetDateTime;

use crate::{
    DocumentId, DocumentVersionId, DomainError, FileId, FileObject, FolderId, Metadata,
    PrincipalRef, StoredFileDescriptor, VersionFile,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Working,
    Published,
    Withdrawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishTransition {
    resulting_document_revision: i64,
}

impl PublishTransition {
    pub const fn resulting_document_revision(self) -> i64 {
        self.resulting_document_revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionNo(i64);

impl VersionNo {
    pub fn new(value: i64) -> Result<Self, DomainError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidVersionNo)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Title(String);

impl Title {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(DomainError::BlankTitle);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    document_id: DocumentId,
    folder_id: FolderId,
    current_version_id: Option<DocumentVersionId>,
    revision: i64,
    metadata: Metadata,
    created_at: OffsetDateTime,
}

impl Document {
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn folder_id(&self) -> FolderId {
        self.folder_id
    }

    pub const fn current_version_id(&self) -> Option<DocumentVersionId> {
        self.current_version_id
    }

    pub const fn revision(&self) -> i64 {
        self.revision
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub fn publish_initial_version(
        &mut self,
        target: &mut DocumentVersion,
        published_at: OffsetDateTime,
    ) -> Result<PublishTransition, DomainError> {
        if target.document_id != self.document_id {
            return Err(DomainError::VersionDocumentMismatch);
        }
        if self.current_version_id.is_some() {
            return Err(DomainError::CurrentVersionAlreadySet);
        }
        if target.lifecycle_state != LifecycleState::Working {
            return Err(DomainError::VersionNotWorking);
        }

        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(DomainError::RevisionOverflow)?;

        target.lifecycle_state = LifecycleState::Published;
        target.published_at = Some(published_at);
        self.current_version_id = Some(target.document_version_id);
        self.revision = next_revision;

        Ok(PublishTransition {
            resulting_document_revision: next_revision,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentVersion {
    document_version_id: DocumentVersionId,
    document_id: DocumentId,
    version_no: VersionNo,
    lifecycle_state: LifecycleState,
    title: Title,
    revision_reason: Option<String>,
    approved_at: Option<OffsetDateTime>,
    scheduled_publish_at: Option<OffsetDateTime>,
    published_at: Option<OffsetDateTime>,
    withdrawn_at: Option<OffsetDateTime>,
    effective_from: Option<OffsetDateTime>,
    effective_to: Option<OffsetDateTime>,
    created_by: PrincipalRef,
    metadata: Metadata,
    created_at: OffsetDateTime,
}

impl DocumentVersion {
    pub const fn document_version_id(&self) -> DocumentVersionId {
        self.document_version_id
    }

    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    pub const fn version_no(&self) -> VersionNo {
        self.version_no
    }

    pub const fn lifecycle_state(&self) -> LifecycleState {
        self.lifecycle_state
    }

    pub fn title(&self) -> &Title {
        &self.title
    }

    pub fn revision_reason(&self) -> Option<&str> {
        self.revision_reason.as_deref()
    }

    pub fn approved_at(&self) -> Option<OffsetDateTime> {
        self.approved_at
    }

    pub fn scheduled_publish_at(&self) -> Option<OffsetDateTime> {
        self.scheduled_publish_at
    }

    pub fn published_at(&self) -> Option<OffsetDateTime> {
        self.published_at
    }

    pub fn withdrawn_at(&self) -> Option<OffsetDateTime> {
        self.withdrawn_at
    }

    pub fn effective_from(&self) -> Option<OffsetDateTime> {
        self.effective_from
    }

    pub fn effective_to(&self) -> Option<OffsetDateTime> {
        self.effective_to
    }

    pub fn created_by(&self) -> &PrincipalRef {
        &self.created_by
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateInitialDocument {
    pub document_id: DocumentId,
    pub version_id: DocumentVersionId,
    pub file_id: FileId,
    pub folder_id: FolderId,
    pub title: Title,
    pub document_metadata: Metadata,
    pub version_metadata: Metadata,
    pub principal: PrincipalRef,
    pub stored_file: StoredFileDescriptor,
    pub original_filename: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialDocument {
    document: Document,
    version: DocumentVersion,
    file: FileObject,
    version_file: VersionFile,
}

impl InitialDocument {
    pub fn create(input: CreateInitialDocument) -> Result<Self, DomainError> {
        if input.original_filename.trim().is_empty() {
            return Err(DomainError::BlankOriginalFilename);
        }

        let version_no = VersionNo::new(1)?;
        let file = FileObject::new(input.file_id, input.stored_file, input.created_at);
        let version_file =
            VersionFile::primary(input.version_id, input.file_id, input.original_filename);

        let document = Document {
            document_id: input.document_id,
            folder_id: input.folder_id,
            current_version_id: None,
            revision: 0,
            metadata: input.document_metadata,
            created_at: input.created_at,
        };

        let version = DocumentVersion {
            document_version_id: input.version_id,
            document_id: input.document_id,
            version_no,
            lifecycle_state: LifecycleState::Working,
            title: input.title,
            revision_reason: None,
            approved_at: None,
            scheduled_publish_at: None,
            published_at: None,
            withdrawn_at: None,
            effective_from: None,
            effective_to: None,
            created_by: input.principal,
            metadata: input.version_metadata,
            created_at: input.created_at,
        };

        Ok(Self {
            document,
            version,
            file,
            version_file,
        })
    }

    pub fn restore_published(
        input: CreateInitialDocument,
        published_at: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        let mut aggregate = Self::create(input)?;
        aggregate
            .document
            .publish_initial_version(&mut aggregate.version, published_at)?;
        Ok(aggregate)
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn version(&self) -> &DocumentVersion {
        &self.version
    }

    pub fn file(&self) -> &FileObject {
        &self.file
    }

    pub fn version_file(&self) -> &VersionFile {
        &self.version_file
    }

    pub fn into_parts(self) -> (Document, DocumentVersion, FileObject, VersionFile) {
        (self.document, self.version, self.file, self.version_file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn fixture(
        document_raw: u128,
        version_document_raw: u128,
        version_raw: u128,
        revision: i64,
        current_version_raw: Option<u128>,
        lifecycle_state: LifecycleState,
    ) -> (Document, DocumentVersion) {
        let document_id = DocumentId::from_uuid(Uuid::from_u128(document_raw));
        let version_document_id = DocumentId::from_uuid(Uuid::from_u128(version_document_raw));
        let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(version_raw));
        let created_at = OffsetDateTime::UNIX_EPOCH;

        let document = Document {
            document_id,
            folder_id: FolderId::from_uuid(Uuid::from_u128(90)),
            current_version_id: current_version_raw
                .map(|value| DocumentVersionId::from_uuid(Uuid::from_u128(value))),
            revision,
            metadata: Metadata::default(),
            created_at,
        };

        let version = DocumentVersion {
            document_version_id: version_id,
            document_id: version_document_id,
            version_no: VersionNo::new(1).unwrap(),
            lifecycle_state,
            title: Title::new("Policy v1").unwrap(),
            revision_reason: None,
            approved_at: None,
            scheduled_publish_at: None,
            published_at: match lifecycle_state {
                LifecycleState::Published => Some(created_at),
                LifecycleState::Working | LifecycleState::Withdrawn => None,
            },
            withdrawn_at: match lifecycle_state {
                LifecycleState::Withdrawn => Some(created_at),
                LifecycleState::Working | LifecycleState::Published => None,
            },
            effective_from: None,
            effective_to: None,
            created_by: PrincipalRef::new("test-idp", "actor-1").unwrap(),
            metadata: Metadata::default(),
            created_at,
        };

        (document, version)
    }

    #[test]
    fn initial_working_version_publishes_without_approval() {
        let published_at = OffsetDateTime::from_unix_timestamp(1_700_000_001).unwrap();
        let (mut document, mut version) = fixture(1, 1, 2, 0, None, LifecycleState::Working);

        let transition = document
            .publish_initial_version(&mut version, published_at)
            .expect("initial working version should publish");

        assert_eq!(version.lifecycle_state(), LifecycleState::Published);
        assert_eq!(version.published_at(), Some(published_at));
        assert_eq!(
            document.current_version_id(),
            Some(version.document_version_id())
        );
        assert_eq!(document.revision(), 1);
        assert_eq!(transition.resulting_document_revision(), 1);
        assert_eq!(version.approved_at(), None);
    }

    #[test]
    fn publish_rejects_cross_document_target_without_mutation() {
        let (mut document, mut version) = fixture(1, 2, 3, 0, None, LifecycleState::Working);
        let before_document = document.clone();
        let before_version = version.clone();

        let error = document
            .publish_initial_version(&mut version, OffsetDateTime::UNIX_EPOCH)
            .unwrap_err();

        assert_eq!(error, DomainError::VersionDocumentMismatch);
        assert_eq!(document, before_document);
        assert_eq!(version, before_version);
    }

    #[test]
    fn publish_rejects_existing_current_version_without_mutation() {
        let (mut document, mut version) = fixture(1, 1, 2, 0, Some(99), LifecycleState::Working);
        let before_document = document.clone();
        let before_version = version.clone();

        let error = document
            .publish_initial_version(&mut version, OffsetDateTime::UNIX_EPOCH)
            .unwrap_err();

        assert_eq!(error, DomainError::CurrentVersionAlreadySet);
        assert_eq!(document, before_document);
        assert_eq!(version, before_version);
    }

    #[test]
    fn publish_rejects_non_working_target_without_mutation() {
        let (mut document, mut version) = fixture(1, 1, 2, 0, None, LifecycleState::Published);
        let before_document = document.clone();
        let before_version = version.clone();

        let error = document
            .publish_initial_version(&mut version, OffsetDateTime::UNIX_EPOCH)
            .unwrap_err();

        assert_eq!(error, DomainError::VersionNotWorking);
        assert_eq!(document, before_document);
        assert_eq!(version, before_version);
    }

    #[test]
    fn publish_rejects_revision_overflow_without_mutation() {
        let (mut document, mut version) = fixture(1, 1, 2, i64::MAX, None, LifecycleState::Working);
        let before_document = document.clone();
        let before_version = version.clone();

        let error = document
            .publish_initial_version(&mut version, OffsetDateTime::UNIX_EPOCH)
            .unwrap_err();

        assert_eq!(error, DomainError::RevisionOverflow);
        assert_eq!(document, before_document);
        assert_eq!(version, before_version);
    }
}
