#![forbid(unsafe_code)]

//! Infrastructure-free authoritative document domain.

#[cfg(test)]
mod tests {
    use super::*;

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
}
