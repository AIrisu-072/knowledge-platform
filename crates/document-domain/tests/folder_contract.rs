use document_domain::normalize_folder_name;

#[test]
fn folder_name_is_trimmed_nfc_and_case_sensitive() {
    assert_eq!(normalize_folder_name("  a\u{308}  ").unwrap(), "ä");
    assert_eq!(normalize_folder_name("Reports").unwrap(), "Reports");
    assert_eq!(normalize_folder_name("reports").unwrap(), "reports");
}

#[test]
fn folder_name_rejects_ambiguous_or_oversize_values() {
    assert!(normalize_folder_name(&"あ".repeat(255)).is_ok());
    assert!(normalize_folder_name(&"あ".repeat(256)).is_err());
    for invalid in ["", "  ", ".", "..", "a/b", "a\\b", "a\nb", "\u{0}x"] {
        assert!(normalize_folder_name(invalid).is_err(), "{invalid:?}");
    }
}
