use unicode_normalization::UnicodeNormalization;

use crate::DomainError;

pub fn normalize_folder_name(value: &str) -> Result<String, DomainError> {
    if value.chars().any(char::is_control) {
        return Err(DomainError::InvalidFolderName);
    }
    let name: String = value.trim().nfc().collect();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > 255
        || name.contains(['/', '\\'])
    {
        return Err(DomainError::InvalidFolderName);
    }
    Ok(name)
}
