use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{DomainError, Title};

const IDENTITY_DOMAIN: &[u8] = b"document-version-identity-v0\0";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalPath(String);

impl LogicalPath {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let normalized: String = value.nfc().collect();
        if normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.contains('\\')
            || normalized.chars().any(char::is_control)
            || normalized.len() > u32::MAX as usize
            || normalized
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(DomainError::InvalidLogicalPath);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticContentItem {
    logical_path: LogicalPath,
    ordinal: u32,
    format_id: String,
    inspection_profile_id: String,
    semantic_fingerprint: [u8; 32],
}

impl SemanticContentItem {
    pub fn new(
        logical_path: LogicalPath,
        ordinal: u32,
        format_id: impl Into<String>,
        inspection_profile_id: impl Into<String>,
        semantic_fingerprint: [u8; 32],
    ) -> Result<Self, DomainError> {
        let format_id = format_id.into();
        let inspection_profile_id = inspection_profile_id.into();
        if !valid_identifier(&format_id) {
            return Err(DomainError::InvalidSemanticFormat);
        }
        if !valid_identifier(&inspection_profile_id) {
            return Err(DomainError::InvalidInspectionProfile);
        }
        Ok(Self {
            logical_path,
            ordinal,
            format_id,
            inspection_profile_id,
            semantic_fingerprint,
        })
    }

    pub fn logical_path(&self) -> &LogicalPath {
        &self.logical_path
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub fn format_id(&self) -> &str {
        &self.format_id
    }

    pub fn inspection_profile_id(&self) -> &str {
        &self.inspection_profile_id
    }

    pub const fn semantic_fingerprint(&self) -> [u8; 32] {
        self.semantic_fingerprint
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= u32::MAX as usize
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionManifest {
    title: Title,
    items: Vec<SemanticContentItem>,
}

impl VersionManifest {
    pub fn new(title: Title, mut items: Vec<SemanticContentItem>) -> Result<Self, DomainError> {
        if items.is_empty() || items.len() > u32::MAX as usize {
            return Err(DomainError::InvalidContentManifest);
        }
        let normalized = title.as_str().replace("\r\n", "\n").replace('\r', "\n");
        let normalized: String = normalized.nfc().collect();
        let title = Title::new(normalized.trim())?;
        if title.as_str().len() > u32::MAX as usize {
            return Err(DomainError::InvalidContentManifest);
        }
        items.sort_by(|left, right| {
            (left.ordinal, left.logical_path.as_str())
                .cmp(&(right.ordinal, right.logical_path.as_str()))
        });
        if items.windows(2).any(|pair| {
            pair[0].ordinal == pair[1].ordinal && pair[0].logical_path == pair[1].logical_path
        }) {
            return Err(DomainError::DuplicateContentItemKey);
        }
        Ok(Self { title, items })
    }

    pub fn title(&self) -> &Title {
        &self.title
    }

    pub fn items(&self) -> &[SemanticContentItem] {
        &self.items
    }

    pub fn identity_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(IDENTITY_DOMAIN);
        digest_field(&mut digest, self.title.as_str().as_bytes());
        digest.update(
            u32::try_from(self.items.len())
                .expect("manifest count validated")
                .to_be_bytes(),
        );
        for item in &self.items {
            digest_field(&mut digest, item.logical_path.as_str().as_bytes());
            digest.update(item.ordinal.to_be_bytes());
            digest_field(&mut digest, item.format_id.as_bytes());
            digest_field(&mut digest, item.inspection_profile_id.as_bytes());
            digest.update(item.semantic_fingerprint);
        }
        digest.finalize().into()
    }
}

fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(
        u32::try_from(bytes.len())
            .expect("manifest field length validated")
            .to_be_bytes(),
    );
    digest.update(bytes);
}
