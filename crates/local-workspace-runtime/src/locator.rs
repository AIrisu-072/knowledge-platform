//! Relative locator validation shared by every platform.
//!
//! The same rules apply on Unix and Windows so a locator accepted on one
//! device never changes meaning on another: no absolute/drive/UNC/device
//! forms, no dot segments, no embedded separators/NUL/control characters, no
//! Windows reserved device names or alternate data streams, and no second
//! decoding of percent or URI escapes.

use crate::error::{RuntimeError, RuntimeErrorCode, RuntimeErrorReason, RuntimeResult};

pub const MAX_DEPTH: usize = 32;
pub const MAX_NAME_BYTES: usize = 255;
pub const MAX_WORKSPACE_NAME_CHARS: usize = 80;

const WINDOWS_FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED_BASES: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];

fn invalid() -> RuntimeError {
    RuntimeError::with(
        RuntimeErrorCode::InvalidLocator,
        RuntimeErrorReason::InvalidName,
    )
}

fn is_reserved_device(name: &str) -> bool {
    // Windows ignores everything from the first dot and trailing spaces when
    // matching device names ("con.txt", "COM1 .log").
    let base = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = base.to_ascii_uppercase();
    if RESERVED_BASES.contains(&upper.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let mut chars = rest.chars();
            if let (Some(c), None) = (chars.next(), chars.next())
                && (c.is_ascii_digit() || matches!(c, '\u{b9}' | '\u{b2}' | '\u{b3}'))
            {
                return true;
            }
        }
    }
    false
}

/// Validate one relative name component.
pub fn validate_name(name: &str) -> RuntimeResult<()> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name == "." || name == ".." {
        return Err(invalid());
    }
    if name
        .chars()
        .any(|c| c.is_control() || WINDOWS_FORBIDDEN.contains(&c))
    {
        return Err(invalid());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(invalid());
    }
    if is_reserved_device(name) {
        return Err(invalid());
    }
    Ok(())
}

/// Validate a full relative locator (empty means the binding root).
pub fn validate_locator(locator: &[String]) -> RuntimeResult<()> {
    if locator.len() > MAX_DEPTH {
        return Err(RuntimeError::with(
            RuntimeErrorCode::Limit,
            RuntimeErrorReason::TooMany,
        ));
    }
    locator.iter().try_for_each(|name| validate_name(name))
}

/// Logical Workspace names are presentation only and never become paths.
pub fn normalize_workspace_name(name: &str) -> RuntimeResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_WORKSPACE_NAME_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return Err(RuntimeError::with(
            RuntimeErrorCode::InvalidLocator,
            RuntimeErrorReason::InvalidName,
        ));
    }
    Ok(trimmed.to_owned())
}

/// Operation IDs are caller-generated retry keys (for example UUIDv7).
pub fn validate_operation_id(operation_id: &str) -> RuntimeResult<()> {
    let ok = !operation_id.is_empty()
        && operation_id.len() <= 64
        && operation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(RuntimeError::with(
            RuntimeErrorCode::InvalidLocator,
            RuntimeErrorReason::OperationMismatch,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejects(name: &str) -> bool {
        validate_name(name).is_err()
    }

    #[test]
    fn rejects_traversal_absolute_and_separator_forms() {
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "/etc",
            "\\\\server\\share",
            "C:",
            "C:\\Windows",
            "file.txt:stream",
            "nul\0byte",
            "tab\tname",
            "line\nfeed",
            "del\u{7f}",
            "trailing.",
            "trailing ",
            "q?",
            "star*",
            "pipe|",
            "\"quoted\"",
            "<lt",
            "gt>",
        ] {
            assert!(rejects(name), "{name:?} must be rejected");
        }
    }

    #[test]
    fn rejects_windows_reserved_device_names_case_and_extension_insensitively() {
        for name in [
            "CON",
            "con",
            "Con.txt",
            "PRN",
            "aux.log",
            "NUL",
            "nul.tar.gz",
            "COM1",
            "com9.txt",
            "LPT1",
            "lpt9",
            "COM\u{b9}",
            "LPT\u{b2}.x",
            "CONIN$",
            "conout$",
            "COM1 .txt",
        ] {
            assert!(rejects(name), "{name:?} must be rejected");
        }
        for name in [
            "CONSOLE",
            "COM10",
            "LPT0x",
            "nullable",
            "auxiliary.txt",
            "com",
        ] {
            assert!(!rejects(name), "{name:?} is an ordinary name");
        }
    }

    #[test]
    fn does_not_decode_escapes_or_normalize_a_second_time() {
        // Percent and dot-like Unicode are ordinary characters, never decoded.
        for name in ["%2e%2e", "%2F", "..%2f", "\u{ff0e}\u{ff0e}", "\u{2024}"] {
            assert!(!rejects(name), "{name:?} is literal, not decoded");
        }
    }

    #[test]
    fn enforces_name_byte_and_depth_limits() {
        assert!(!rejects(&"a".repeat(MAX_NAME_BYTES)));
        assert!(rejects(&"a".repeat(MAX_NAME_BYTES + 1)));
        // 85 three-byte characters = 255 bytes; 86 exceed the byte limit.
        assert!(!rejects(&"\u{3042}".repeat(85)));
        assert!(rejects(&"\u{3042}".repeat(86)));
        let deep: Vec<String> = (0..MAX_DEPTH).map(|i| format!("d{i}")).collect();
        assert!(validate_locator(&deep).is_ok());
        let deeper: Vec<String> = (0..=MAX_DEPTH).map(|i| format!("d{i}")).collect();
        assert_eq!(
            validate_locator(&deeper).unwrap_err().code,
            RuntimeErrorCode::Limit
        );
    }

    #[test]
    fn workspace_names_are_trimmed_bounded_and_never_paths() {
        assert_eq!(
            normalize_workspace_name("  見積 ").unwrap(),
            "見積".to_owned()
        );
        // Names may contain path-like text; it is never used as a path.
        assert_eq!(normalize_workspace_name("../a/b").unwrap(), "../a/b");
        assert!(normalize_workspace_name("   ").is_err());
        assert!(normalize_workspace_name("a\nb").is_err());
        assert!(normalize_workspace_name(&"x".repeat(81)).is_err());
    }

    #[test]
    fn operation_ids_are_bounded_tokens() {
        assert!(validate_operation_id("0199b2c4-5d6e-7f80-9a1b-2c3d4e5f6071").is_ok());
        for bad in ["", "a b", "../x", &"a".repeat(65)] {
            assert!(validate_operation_id(bad).is_err(), "{bad:?}");
        }
    }
}
