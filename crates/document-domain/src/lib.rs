#![forbid(unsafe_code)]

//! Infrastructure-free authoritative document domain.

#[cfg(test)]
mod tests {
    use super::*;
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
}
