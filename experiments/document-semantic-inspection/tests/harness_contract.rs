use document_semantic_inspection_poc::{
    AdapterOutput, ErrorCode, FixtureManifest, FormatId, InspectionAdapter, InspectionProfile,
    PocError, AlwaysSuccessAdapter, fixture_case, run_case,
};
use std::sync::atomic::{AtomicBool, Ordering};

fn base_sha() -> String {
    "4b654bd1437066b13498661f3ca14774daf1066d072036beffaf06f0c014250e".into()
}

#[test]
fn duplicate_case_ids_are_rejected() {
    let json = format!(
        r#"{{
          "schema_version": 1,
          "cases": [
            {{"id":"dup","class":"BASE","path":"txt/base.txt","format":"txt","sha256":"{}","size":9,"expected":{{"kind":"success"}}}},
            {{"id":"dup","class":"BASE","path":"txt/base.txt","format":"txt","sha256":"{}","size":9,"expected":{{"kind":"success"}}}}
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
            {{"id":"noise","class":"NOISE","path":"txt/base.txt","format":"txt","sha256":"{}","size":9,"expected":{{"kind":"same_as","case_id":"missing"}}}}
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
        r#"[{{"id":"x","class":"MAYBE","path":"txt/base.txt","format":"txt","sha256":"{}","size":9,"expected":{{"kind":"success"}}}}]"#,
        base_sha()
    );
    assert!(matches!(
        FixtureManifest::from_json(&json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}

#[test]
fn raw_hash_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", "00".repeat(32), 9);
    let err = run_case(&case, &AlwaysSuccessAdapter).unwrap_err();
    assert!(matches!(err, PocError::RawBindingMismatch { .. }));
}

#[test]
fn raw_size_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", base_sha(), 999);
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
    let mut case = fixture_case("txt/base.txt", base_sha(), 9);
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
    let case = fixture_case("txt/base.txt", base_sha(), 9);
    let result = run_case(&case, &AlwaysSuccessAdapter).expect("valid fixture should inspect");
    assert_eq!(
        hex::encode(result.semantic_fingerprint),
        "4b654bd1437066b13498661f3ca14774daf1066d072036beffaf06f0c014250e"
    );
    assert_eq!(result.output.semantic_projection, b"baseline\n");
}

#[test]
fn manifest_error_code_is_stable() {
    let err = FixtureManifest::from_json("{}").unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidManifest);
}
