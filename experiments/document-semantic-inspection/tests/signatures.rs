use document_semantic_inspection_poc::{
    SignatureInspector, SignatureTrustContext, SignatureValidity,
};
use std::path::Path;

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).expect(path)
}

fn cms(name: &str, content_name: &str, with_crl: bool) -> document_semantic_inspection_poc::SignatureEvidence {
    let mut trust = SignatureTrustContext::new(vec![fixture("fixtures/pdf/signatures/root.der")]);
    if with_crl {
        trust = trust.with_crl_der(fixture("fixtures/pdf/signatures/revoked.crl.der"));
    }
    SignatureInspector::verify_detached_cms(
        &fixture(&format!("fixtures/pdf/signatures/{content_name}")),
        &fixture(&format!("fixtures/pdf/signatures/{name}.cms.der")),
        &trust,
    )
    .expect("invalid signatures must still produce evidence")
}

#[test]
fn cms_signature_vectors_have_stable_validity_classes() {
    assert_eq!(cms("valid", "content.bin", false).validity, SignatureValidity::Valid);
    assert_eq!(cms("valid", "tampered-content.bin", false).validity, SignatureValidity::Invalid);
    assert_eq!(cms("invalid-digest", "content.bin", false).validity, SignatureValidity::Invalid);
    assert_eq!(cms("expired", "content.bin", false).validity, SignatureValidity::Invalid);
    assert_eq!(cms("revoked", "content.bin", true).validity, SignatureValidity::Invalid);
    assert_eq!(cms("unknown", "content.bin", false).validity, SignatureValidity::Unverifiable);
    assert_eq!(cms("broken", "content.bin", false).validity, SignatureValidity::Unverifiable);
    assert_eq!(cms("unsupported-algorithm", "content.bin", false).validity, SignatureValidity::Unverifiable);
    assert_eq!(cms("malformed", "content.bin", false).validity, SignatureValidity::Invalid);
}

#[test]
fn valid_cms_evidence_preserves_signer_and_coverage_metadata() {
    let evidence = cms("valid", "content.bin", false);
    assert_eq!(evidence.kind, "cms");
    assert!(evidence.signer_claim.as_deref().is_some_and(|v| v.contains("DSI TEST VALID SIGNER")));
    assert!(evidence.certificate_subject.is_some());
    assert!(evidence.certificate_issuer.is_some());
    assert!(evidence.certificate_fingerprint.as_deref().is_some_and(|v| v.len() == 64));
    assert_eq!(evidence.covered_content.as_deref(), Some("detached-content"));
    assert!(!evidence.validation_diagnostics.is_empty());
}
