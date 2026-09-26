#[path = "support/signature_ooxml.rs"]
mod signature_ooxml;

use std::io::Cursor;
use std::path::Path;

use document_semantic_inspection_core::{
    InspectionProfileVersion, SignatureValidity, WorkerProtocolVersion, WorkerRequest,
    WorkerResponse,
};
use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, SignatureInspector, SignatureTrustContext,
    run_worker_shell_with_signature_trust,
};
use sha2::{Digest, Sha256};

const VALID_PDF: &[u8] = include_bytes!("fixtures/signatures/valid.pdf");
const TAMPERED_PDF: &[u8] = include_bytes!("fixtures/signatures/tampered.pdf");
const MALFORMED_PDF: &[u8] = include_bytes!("fixtures/signatures/malformed.pdf");
const PDF_TRUST: &[u8] = include_bytes!("fixtures/signatures/test-only-trust.der");
const UNSIGNED_PDF: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");

fn poc_fixture(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../experiments/document-semantic-inspection")
            .join(path),
    )
    .expect("qualified synthetic signature fixture")
}

fn cms(
    name: &str,
    content_name: &str,
    revoked: bool,
) -> document_semantic_inspection_core::DigitalSignatureEvidence {
    let mut trust =
        SignatureTrustContext::new(vec![poc_fixture("fixtures/pdf/signatures/root.der")]);
    if revoked {
        trust = trust.with_crl_der(poc_fixture("fixtures/pdf/signatures/revoked.crl.der"));
    }
    SignatureInspector::verify_detached_cms(
        &poc_fixture(&format!("fixtures/pdf/signatures/{content_name}")),
        &poc_fixture(&format!("fixtures/pdf/signatures/{name}.cms.der")),
        &trust,
    )
    .expect("invalid signatures still produce evidence")
}

#[test]
fn cms_nine_qualified_classes_remain_distinct() {
    for (signature, content, revoked, expected) in [
        ("valid", "content.bin", false, SignatureValidity::Valid),
        (
            "valid",
            "tampered-content.bin",
            false,
            SignatureValidity::Invalid,
        ),
        (
            "invalid-digest",
            "content.bin",
            false,
            SignatureValidity::Invalid,
        ),
        ("expired", "content.bin", false, SignatureValidity::Invalid),
        ("revoked", "content.bin", true, SignatureValidity::Invalid),
        (
            "unknown",
            "content.bin",
            false,
            SignatureValidity::Unverifiable,
        ),
        (
            "broken",
            "content.bin",
            false,
            SignatureValidity::Unverifiable,
        ),
        (
            "unsupported-algorithm",
            "content.bin",
            false,
            SignatureValidity::Unverifiable,
        ),
        (
            "malformed",
            "content.bin",
            false,
            SignatureValidity::Invalid,
        ),
    ] {
        let evidence = cms(signature, content, revoked);
        assert_eq!(
            evidence.cryptographic_validity, expected,
            "CMS vector {signature} with {content}"
        );
        assert_eq!(evidence.signature_type, "cms");
        assert_eq!(evidence.covered_content, ["detached-content"]);
    }
    let valid = cms("valid", "content.bin", false);
    assert!(
        valid
            .signer_claim
            .as_deref()
            .is_some_and(|claim| claim.contains("DSI TEST VALID SIGNER"))
    );
    assert!(
        valid
            .certificate_fingerprint
            .as_deref()
            .is_some_and(|fingerprint| fingerprint.len() == 64)
    );
}

#[test]
fn xmldsig_valid_invalid_and_unverifiable_are_evidence() {
    let trust = SignatureTrustContext::new(vec![poc_fixture("fixtures/pdf/signatures/root.der")]);
    for (name, expected) in [
        ("xml-valid", SignatureValidity::Valid),
        ("xml-tampered", SignatureValidity::Invalid),
        ("xml-invalid-digest", SignatureValidity::Invalid),
        ("xml-expired", SignatureValidity::Invalid),
        ("xml-unknown", SignatureValidity::Unverifiable),
        ("xml-unsupported-algorithm", SignatureValidity::Unverifiable),
        ("xml-malformed", SignatureValidity::Invalid),
    ] {
        let xml = String::from_utf8(poc_fixture(&format!("fixtures/docx/signatures/{name}.xml")))
            .expect("synthetic XML is UTF-8");
        let evidence = SignatureInspector::verify_xmldsig(&xml, &trust).expect("XMLDSig evidence");
        assert_eq!(evidence.cryptographic_validity, expected, "{name}");
        assert_eq!(evidence.signature_type, "xmldsig");
    }
}

#[test]
fn pdf_byte_range_verifies_exact_covered_bytes() {
    let trust = SignatureTrustContext::new(vec![PDF_TRUST.to_vec()]);
    for (pdf, expected) in [
        (VALID_PDF, SignatureValidity::Valid),
        (TAMPERED_PDF, SignatureValidity::Invalid),
        (MALFORMED_PDF, SignatureValidity::Invalid),
    ] {
        let evidence =
            SignatureInspector::verify_pdf_byte_range(pdf, &trust).expect("PDF signature evidence");
        assert_eq!(evidence.cryptographic_validity, expected);
        assert_eq!(evidence.signature_type, "pdf-cms");
        assert!(
            evidence
                .covered_content
                .iter()
                .any(|part| part.starts_with("pdf-byte-range:"))
        );
    }
}

#[test]
fn pdf_comment_tokens_do_not_create_a_signature() {
    let mut unsigned = UNSIGNED_PDF.to_vec();
    unsigned.extend_from_slice(b"\n% /ByteRange [0 0 0 0] /Contents <00>\n");
    let trust = SignatureTrustContext::new(vec![PDF_TRUST.to_vec()]);
    let evidence =
        SignatureInspector::inspect_pdf_signatures(&unsigned, &trust).expect("unsigned PDF");
    assert!(
        evidence.is_empty(),
        "comment text is not a PDF signature dictionary"
    );
}

#[test]
fn self_contained_xml_signature_is_not_a_valid_package_signature() {
    let trust = SignatureTrustContext::new(vec![poc_fixture("fixtures/pdf/signatures/root.der")]);
    let xml = poc_fixture("fixtures/docx/signatures/xml-valid.xml");
    for path in [
        "fixtures/docx/base.docx",
        "fixtures/xlsx/base.xlsx",
        "fixtures/pptx/base.pptx",
    ] {
        let package = poc_fixture(path);
        assert!(
            SignatureInspector::verify_ooxml_package(&package, &trust)
                .expect("unsigned package")
                .is_empty(),
            "{path}"
        );
        let wrapped = signature_ooxml::add_ooxml_signature(&package, &xml);
        let evidence =
            SignatureInspector::verify_ooxml_package(&wrapped, &trust).expect("signature origin");
        assert_eq!(evidence.len(), 1, "{path}");
        assert_eq!(evidence[0].signature_type, "ooxml-xmldsig", "{path}");
        assert_eq!(
            evidence[0].cryptographic_validity,
            SignatureValidity::Unverifiable,
            "XML that signs only itself cannot authenticate any package part: {path}"
        );
    }
}

#[test]
fn unsigned_manifest_inside_valid_xml_signature_cannot_claim_package_coverage() {
    let trust = SignatureTrustContext::new(vec![poc_fixture("fixtures/pdf/signatures/root.der")]);
    let original = String::from_utf8(poc_fixture("fixtures/docx/signatures/xml-valid.xml"))
        .expect("UTF-8 XML");
    let unsigned_manifest = r#"<ds:Object Id="package-object"><ds:Manifest><ds:Reference URI="/word/document.xml?ContentType=application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"><ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><ds:DigestValue>AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=</ds:DigestValue></ds:Reference></ds:Manifest></ds:Object></ds:Signature>"#;
    let injected = original.replacen("</ds:Signature>", unsigned_manifest, 1);
    assert_ne!(injected, original);

    let package = poc_fixture("fixtures/docx/base.docx");
    let wrapped = signature_ooxml::add_ooxml_signature(&package, injected.as_bytes());
    let evidence =
        SignatureInspector::verify_ooxml_package(&wrapped, &trust).expect("signature evidence");
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        evidence[0].cryptographic_validity,
        SignatureValidity::Unverifiable,
        "an unsigned Manifest cannot prove word/document.xml coverage"
    );
    assert!(
        !evidence[0]
            .covered_content
            .iter()
            .any(|part| part == "word/document.xml"),
        "unsigned Manifest parts cannot be reported as authenticated coverage"
    );
}

fn worker_response(pdf: &[u8], trust: &SignatureTrustContext) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: "application/pdf".to_owned(),
        expected_raw_content_hash: Sha256::digest(pdf).into(),
        expected_size_bytes: pdf.len() as u64,
        trace_context: None,
    };
    let mut input = Cursor::new(pdf);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_worker_shell_with_signature_trust(
        &serde_json::to_vec(&request).expect("request serialization"),
        &mut input,
        &mut stdout,
        &mut stderr,
        64 * 1024,
        8 * 1024 * 1024,
        trust,
    );
    assert_eq!(
        exit,
        0,
        "worker failed: {}",
        String::from_utf8_lossy(&stderr)
    );
    serde_json::from_slice(&stdout).expect("structured worker response")
}

#[test]
fn signature_validity_changes_evidence_without_changing_semantic_identity() {
    let trust = SignatureTrustContext::new(vec![PDF_TRUST.to_vec()]);
    let mut invalid_signature = VALID_PDF.to_vec();
    let marker = b"/Contents <";
    let offset = invalid_signature
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("signature Contents")
        + marker.len();
    invalid_signature[offset + 16] = if invalid_signature[offset + 16] == b'0' {
        b'1'
    } else {
        b'0'
    };

    let left_semantic = PdfAdapter
        .inspect(VALID_PDF, &AdapterProfile::default())
        .expect("signed PDF semantic projection")
        .semantic_fingerprint();
    let right_semantic = PdfAdapter
        .inspect(&invalid_signature, &AdapterProfile::default())
        .expect("invalid signature semantic projection")
        .semantic_fingerprint();
    assert_eq!(left_semantic, right_semantic);

    let valid = worker_response(VALID_PDF, &trust);
    let invalid = worker_response(&invalid_signature, &trust);
    assert_eq!(valid.semantic_fingerprint, invalid.semantic_fingerprint);
    assert_eq!(valid.digital_signature_evidence.len(), 1);
    assert_eq!(invalid.digital_signature_evidence.len(), 1);
    assert_eq!(
        valid.digital_signature_evidence[0].cryptographic_validity,
        SignatureValidity::Valid
    );
    assert_eq!(
        invalid.digital_signature_evidence[0].cryptographic_validity,
        SignatureValidity::Invalid
    );
}
