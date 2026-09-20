use document_semantic_inspection_poc::{
    FixtureManifest, PocError, fixture_case, run_case, AlwaysSuccessAdapter,
};

#[test]
fn raw_binding_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", "00".repeat(32), 999);
    let err = run_case(&case, &AlwaysSuccessAdapter).unwrap_err();
    assert!(matches!(err, PocError::RawBindingMismatch { .. }));
}

#[test]
fn unknown_fixture_class_is_rejected() {
    let json = r#"[{"id":"x","class":"MAYBE","path":"txt/base.txt"}]"#;
    assert!(matches!(
        FixtureManifest::from_json(json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}
