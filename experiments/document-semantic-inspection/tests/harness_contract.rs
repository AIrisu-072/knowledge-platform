use document_semantic_inspection_poc::{
    AdapterOutput, ErrorCode, FixtureManifest, FormatId, InspectionAdapter, InspectionProfile,
    PocError, AlwaysSuccessAdapter, fixture_case, run_case,
};
use std::sync::atomic::{AtomicBool, Ordering};

fn base_sha() -> String {
    "7b49b9e063bd91a4f9252b413261f5557b9c570aa61516989499f64a62dbcdd6".into()
}

#[test]
fn duplicate_case_ids_are_rejected() {
    let json = format!(
        r#"{{
          "schema_version": 1,
          "cases": [
            {{"id":"dup","class":"BASE","path":"txt/base.txt","format":"txt","sha256":"{}","size":6,"expected":{{"kind":"success"}}}},
            {{"id":"dup","class":"BASE","path":"txt/base.txt","format":"txt","sha256":"{}","size":6,"expected":{{"kind":"success"}}}}
          ]
        }}"#,
        base_sha(),
        base_sha()
    );
    assert!(matches!(
        FixtureManifest::from_json(&json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}

#[test]
fn missing_base_reference_is_rejected() {
    let json = format!(
        r#"{{
          "schema_version": 1,
          "cases": [
            {{"id":"noise","class":"NOISE","path":"txt/base.txt","format":"txt","sha256":"{}","size":6,"expected":{{"kind":"same_as","case_id":"missing"}}}}
          ]
        }}"#,
        base_sha()
    );
    assert!(matches!(
        FixtureManifest::from_json(&json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}

#[test]
fn unknown_fixture_class_is_rejected() {
    let json = format!(
        r#"[{{"id":"x","class":"MAYBE","path":"txt/base.txt","format":"txt","sha256":"{}","size":6,"expected":{{"kind":"success"}}}}]"#,
        base_sha()
    );
    assert!(matches!(
        FixtureManifest::from_json(&json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}

#[test]
fn raw_hash_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", "00".repeat(32), 6);
    let err = run_case(&case, &AlwaysSuccessAdapter).unwrap_err();
    assert!(matches!(err, PocError::RawBindingMismatch { .. }));
}

#[test]
fn raw_size_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", base_sha(), 699);
    let err = run_case(&case, &AlwaysSuccessAdapter).unwrap_err();
    assert!(matches!(err, PocError::RawBindingMismatch { .. }));
}

struct PanicPdfAdapter {
    called: AtomicBool,
}

impl InspectionAdapter for PanicPdfAdapter {
    fn format(&self) -> FormatId {
        FormatId::Pdf
    }

    fn inspect(
        &self,
        _input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        self.called.store(true, Ordering::SeqCst);
        panic!("adapter must not be called before format validation");
    }
}

#[test]
fn format_mismatch_fails_before_adapter_invocation() {
    let mut case = fixture_case("txt/base.txt", base_sha(), 6);
    case.format = FormatId::Pdf;
    let adapter = PanicPdfAdapter {
        called: AtomicBool::new(false),
    };

    let err = run_case(&case, &adapter).unwrap_err();
    assert!(matches!(
        err,
        PocError::FormatMismatch {
            expected: FormatId::Pdf,
            observed: Some(FormatId::Txt)
        }
    ));
    assert!(!adapter.called.load(Ordering::SeqCst));
}

#[test]
fn bound_txt_fixture_succeeds_and_is_fingerprinted() {
    let case = fixture_case("txt/base.txt", base_sha(), 6);
    let result = run_case(&case, &AlwaysSuccessAdapter).expect("valid fixture should inspect");
    assert_eq!(
        hex::encode(result.semantic_fingerprint),
        "7b49b9e063bd91a4f9252b413261f5557b9c570aa61516989499f64a62dbcdd6"
    );
    assert_eq!(result.output.semantic_projection, b"caf\xc3\xa9\n");
}

#[test]
fn manifest_error_code_is_stable() {
    let err = FixtureManifest::from_json("{}").unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidManifest);
}
