use crate::DomainError;

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
            .split(|c| c == '/' || c == '\\')
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
