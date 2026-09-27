mod support;

use document_semantic_inspection_poc::{
    SignatureInspector, SignatureTrustContext, SignatureValidity,
};
use std::path::Path;

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).expect(path)
}

#[test]
fn unsigned_manifest_inside_valid_xml_signature_does_not_authenticate_package_part() {
    let trust = SignatureTrustContext::new(vec![fixture("fixtures/pdf/signatures/root.der")]);
    let original =
        String::from_utf8(fixture("fixtures/docx/signatures/xml-valid.xml")).expect("UTF-8 XML");
    let unsigned_manifest = r#"<ds:Object Id="package-object"><ds:Manifest><ds:Reference URI="/word/document.xml?ContentType=application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"><ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><ds:DigestValue>AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=</ds:DigestValue></ds:Reference></ds:Manifest></ds:Object></ds:Signature>"#;
    let injected = original.replacen("</ds:Signature>", unsigned_manifest, 1);
    assert_ne!(
        injected, original,
        "the signature fixture has one closing element"
    );
    assert_eq!(
        SignatureInspector::verify_xmldsig(&injected, &trust)
            .expect("XML signature itself is parseable")
            .validity,
        SignatureValidity::Valid,
        "the manifest is outside SignedInfo coverage"
    );

    let package = fixture("fixtures/docx/base.docx");
    let wrapped = support::signature_ooxml::add_ooxml_signature(&package, injected.as_bytes());
    let evidence =
        SignatureInspector::verify_ooxml_package(&wrapped, &trust).expect("OPC origin traversal");
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        evidence[0].validity,
        SignatureValidity::Unverifiable,
        "an unsigned Manifest cannot prove word/document.xml coverage"
    );
}
