mod support;
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

fn xml(name: &str) -> document_semantic_inspection_poc::SignatureEvidence {
    let trust = SignatureTrustContext::new(vec![fixture("fixtures/pdf/signatures/root.der")]);
    let source = String::from_utf8(
        fixture(&format!("fixtures/docx/signatures/{name}.xml"))
    ).expect("XML fixture is UTF-8");
    SignatureInspector::verify_xmldsig(&source, &trust)
        .expect("invalid XML signatures must still produce evidence")
}

#[test]
fn xmldsig_signature_vectors_have_stable_validity_classes() {
    assert_eq!(xml("xml-valid").validity, SignatureValidity::Valid);
    assert_eq!(xml("xml-tampered").validity, SignatureValidity::Invalid);
    assert_eq!(xml("xml-invalid-digest").validity, SignatureValidity::Invalid);
    assert_eq!(xml("xml-expired").validity, SignatureValidity::Invalid);
    assert_eq!(xml("xml-unknown").validity, SignatureValidity::Unverifiable);
    assert_eq!(xml("xml-unsupported-algorithm").validity, SignatureValidity::Unverifiable);
    assert_eq!(xml("xml-malformed").validity, SignatureValidity::Invalid);
}

#[test]
fn valid_xmldsig_evidence_is_separate_from_semantic_identity() {
    let evidence = xml("xml-valid");
    assert_eq!(evidence.kind, "xmldsig");
    assert_eq!(evidence.covered_content.as_deref(), Some("same-document-references"));
    assert!(evidence.certificate_fingerprint.as_deref().is_some_and(|v| v.len() == 64));
}


fn pdf_byte_range(
    variant: support::signature_pdf::PdfByteRangeVariant,
) -> document_semantic_inspection_poc::SignatureEvidence {
    let fixture = support::signature_pdf::build_pdf_byte_range_fixture(variant);
    let trust = SignatureTrustContext::new(vec![fixture.trust_der]);
    SignatureInspector::verify_pdf_byte_range(&fixture.pdf, &trust)
        .expect("invalid PDF signatures must still produce evidence")
}

#[test]
fn pdf_byte_range_signature_vectors_validate_exact_covered_bytes() {
    use support::signature_pdf::PdfByteRangeVariant;

    assert_eq!(
        pdf_byte_range(PdfByteRangeVariant::Valid).validity,
        SignatureValidity::Valid
    );
    assert_eq!(
        pdf_byte_range(PdfByteRangeVariant::Tampered).validity,
        SignatureValidity::Invalid
    );
    assert_eq!(
        pdf_byte_range(PdfByteRangeVariant::MalformedByteRange).validity,
        SignatureValidity::Invalid
    );
}

#[test]
fn valid_pdf_signature_evidence_records_byte_range_coverage() {
    let evidence = pdf_byte_range(support::signature_pdf::PdfByteRangeVariant::Valid);
    assert_eq!(evidence.kind, "pdf-cms");
    assert!(
        evidence
            .signer_claim
            .as_deref()
            .is_some_and(|value| value.contains("DSI TEST PDF SIGNER"))
    );
    assert!(
        evidence
            .covered_content
            .as_deref()
            .is_some_and(|value| value.starts_with("pdf-byte-range:"))
    );
}

#[test]
fn ooxml_signature_wrapper_covers_all_required_office_formats() {
    let trust = SignatureTrustContext::new(vec![fixture("fixtures/pdf/signatures/root.der")]);
    let signature_xml = fixture("fixtures/docx/signatures/xml-valid.xml");

    for path in [
        "fixtures/docx/base.docx",
        "fixtures/xlsx/base.xlsx",
        "fixtures/pptx/base.pptx",
    ] {
        let unsigned = fixture(path);
        let unsigned_evidence =
            SignatureInspector::verify_ooxml_package(&unsigned, &trust).expect("unsigned package");
        assert!(unsigned_evidence.is_empty(), "{path}");

        let signed = support::signature_ooxml::add_ooxml_signature(&unsigned, &signature_xml);
        let evidence =
            SignatureInspector::verify_ooxml_package(&signed, &trust).expect("signed OOXML package");
        assert_eq!(evidence.len(), 1, "{path}");
        assert_eq!(evidence[0].kind, "ooxml-xmldsig", "{path}");
        assert_eq!(evidence[0].validity, SignatureValidity::Valid, "{path}");
        assert_eq!(
            evidence[0].covered_content.as_deref(),
            Some("ooxml-signature-part:_xmlsignatures/sig1.xml"),
            "{path}"
        );
    }
}
