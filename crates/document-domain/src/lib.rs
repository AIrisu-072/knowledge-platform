#![forbid(unsafe_code)]

//! Infrastructure-free authoritative document domain.

mod document;
mod error;
mod file;
mod ids;
mod metadata;
mod principal;

pub use document::{
    CreateInitialDocument, Document, DocumentVersion, InitialDocument, LifecycleState,
    PublishTransition, Title, VersionNo,
};
pub use error::DomainError;
pub use file::{
    ContentHash, FileObject, FileRole, FileSize, MediaType, StorageKey, StoredFileDescriptor,
    VersionFile,
};
pub use ids::{AuditEventId, DocumentId, DocumentVersionId, EventId, FileId, FolderId};
pub use metadata::Metadata;
pub use principal::PrincipalRef;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn typed_ids_round_trip_without_exposing_infrastructure() {
        let raw = Uuid::from_u128(0x1234);
        assert_eq!(DocumentId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(DocumentVersionId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(FileId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(FolderId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(EventId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(AuditEventId::from_uuid(raw).as_uuid(), raw);
    }

    #[test]
    fn version_number_must_be_positive() {
        assert!(VersionNo::new(0).is_err());
        assert_eq!(VersionNo::new(1).unwrap().get(), 1);
    }

    #[test]
    fn content_hash_must_be_exactly_sha256_width() {
        assert!(ContentHash::from_slice(&[0_u8; 31]).is_err());
        assert!(ContentHash::from_slice(&[0_u8; 32]).is_ok());
        assert!(ContentHash::from_slice(&[0_u8; 33]).is_err());
    }

    #[test]
    fn file_size_cannot_be_negative() {
        assert!(FileSize::new(-1).is_err());
        assert_eq!(FileSize::new(0).unwrap().get(), 0);
    }

    #[test]
    fn title_cannot_be_blank() {
        assert!(Title::new("   ").is_err());
        assert_eq!(Title::new(" Policy v1 ").unwrap().as_str(), "Policy v1");
    }

    #[test]
    fn storage_key_is_relative_and_cannot_escape_storage_root() {
        assert!(StorageKey::new("").is_err());
        assert!(StorageKey::new("/objects/file").is_err());
        assert!(StorageKey::new("../outside").is_err());
        assert!(StorageKey::new("objects/../outside").is_err());
        assert!(StorageKey::new(r"objects\..\outside").is_err());
        assert_eq!(
            StorageKey::new("objects/ab/file-id").unwrap().as_str(),
            "objects/ab/file-id"
        );
    }

    #[test]
    fn principal_ref_rejects_blank_identity_fields() {
        assert!(PrincipalRef::new("", "principal-1").is_err());
        assert!(PrincipalRef::new("windows", "   ").is_err());
        let principal = PrincipalRef::new(" windows ", " principal-1 ").unwrap();
        assert_eq!(principal.identity_provider(), "windows");
        assert_eq!(principal.principal_id(), "principal-1");
    }

    #[test]
    fn metadata_round_trips_domain_json_map() {
        let mut map = Map::new();
        map.insert("department".into(), Value::String("risk".into()));
        let metadata = Metadata::from_map(map.clone());
        assert_eq!(metadata.as_map(), &map);
    }

    #[test]
    fn stored_file_descriptor_requires_media_type() {
        assert!(MediaType::new("   ").is_err());
        let descriptor = StoredFileDescriptor::new(
            StorageKey::new("objects/ab/file-id").unwrap(),
            ContentHash::from_slice(&[7_u8; 32]).unwrap(),
            FileSize::new(3).unwrap(),
            MediaType::new(" application/pdf ").unwrap(),
        );
        assert_eq!(descriptor.media_type().as_str(), "application/pdf");
    }

    #[test]
    fn initial_document_is_working_not_current_and_links_primary_file() {
        let created_at = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let document_id = DocumentId::from_uuid(Uuid::from_u128(1));
        let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(2));
        let file_id = FileId::from_uuid(Uuid::from_u128(3));
        let folder_id = FolderId::from_uuid(Uuid::from_u128(4));

        let aggregate = InitialDocument::create(CreateInitialDocument {
            document_id,
            version_id,
            file_id,
            folder_id,
            title: Title::new("Policy v1").unwrap(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("windows", "principal-1").unwrap(),
            stored_file: StoredFileDescriptor::new(
                StorageKey::new("objects/ab/file-id").unwrap(),
                ContentHash::from_slice(&[9_u8; 32]).unwrap(),
                FileSize::new(3).unwrap(),
                MediaType::new("application/pdf").unwrap(),
            ),
            original_filename: "policy.pdf".into(),
            created_at,
        })
        .unwrap();

        assert_eq!(aggregate.document().document_id(), document_id);
        assert_eq!(aggregate.document().folder_id(), folder_id);
        assert_eq!(aggregate.document().current_version_id(), None);
        assert_eq!(aggregate.document().revision(), 0);
        assert_eq!(aggregate.version().document_version_id(), version_id);
        assert_eq!(aggregate.version().document_id(), document_id);
        assert_eq!(aggregate.version().version_no().get(), 1);
        assert_eq!(
            aggregate.version().lifecycle_state(),
            LifecycleState::Working
        );
        assert_eq!(aggregate.version().approved_at(), None);
        assert_eq!(aggregate.version().scheduled_publish_at(), None);
        assert_eq!(aggregate.version().published_at(), None);
        assert_eq!(aggregate.version().withdrawn_at(), None);
        assert_eq!(aggregate.version().effective_from(), None);
        assert_eq!(aggregate.version().effective_to(), None);
        assert_eq!(aggregate.file().file_id(), file_id);
        assert_eq!(aggregate.version_file().document_version_id(), version_id);
        assert_eq!(aggregate.version_file().file_id(), file_id);
        assert_eq!(aggregate.version_file().role(), FileRole::Primary);
        assert_eq!(aggregate.version_file().ordinal(), 0);
        assert_eq!(aggregate.version_file().original_filename(), "policy.pdf");
    }

    #[test]
    fn initial_document_rejects_blank_original_filename() {
        let result = InitialDocument::create(CreateInitialDocument {
            document_id: DocumentId::from_uuid(Uuid::from_u128(11)),
            version_id: DocumentVersionId::from_uuid(Uuid::from_u128(12)),
            file_id: FileId::from_uuid(Uuid::from_u128(13)),
            folder_id: FolderId::from_uuid(Uuid::from_u128(14)),
            title: Title::new("Policy v1").unwrap(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("windows", "principal-1").unwrap(),
            stored_file: StoredFileDescriptor::new(
                StorageKey::new("objects/ab/file-id").unwrap(),
                ContentHash::from_slice(&[5_u8; 32]).unwrap(),
                FileSize::new(1).unwrap(),
                MediaType::new("application/pdf").unwrap(),
            ),
            original_filename: "   ".into(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        });

        assert_eq!(result.unwrap_err(), DomainError::BlankOriginalFilename);
    }
}
