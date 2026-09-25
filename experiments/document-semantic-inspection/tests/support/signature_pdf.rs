use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
use openssl::pkey::PKey;
use openssl::stack::Stack;
use openssl::x509::X509;
use std::collections::BTreeMap;

// TEST ONLY: fixed SEC1 EC private-key bytes for deterministic PoC fixture generation.
// This is not a live credential and must never be reused outside this synthetic corpus.
const TEST_ONLY_EC_PRIVATE_KEY_DER: &[u8] = &[48,119,2,1,1,4,32,69,145,53,152,40,122,121,72,8,236,176,58,104,211,249,196,188,35,80,232,205,34,163,130,29,179,78,171,66,58,161,32,160,10,6,8,42,134,72,206,61,3,1,7,161,68,3,66,0,4,161,127,186,126,15,205,207,252,62,180,211,147,12,96,152,23,219,161,171,195,144,206,78,23,8,40,219,232,126,217,28,48,206,198,91,23,31,44,93,59,52,209,135,120,15,247,57,69,224,218,144,4,138,102,242,165,189,8,230,71,7,181,220,36];

#[derive(Debug, Clone, Copy)]
pub enum PdfByteRangeVariant {
    Valid,
    Tampered,
    MalformedByteRange,
}

pub struct PdfByteRangeFixture {
    pub pdf: Vec<u8>,
    pub trust_der: Vec<u8>,
}

pub fn build_pdf_byte_range_fixture(variant: PdfByteRangeVariant) -> PdfByteRangeFixture {
    let cert = X509::from_der(include_bytes!(
        "../../fixtures/pdf/signatures/byte-range-test-only-cert.der"
    ))
    .expect("TEST ONLY PDF signer certificate");
    let key = PKey::private_key_from_der(TEST_ONLY_EC_PRIVATE_KEY_DER)
        .expect("TEST ONLY PDF signer key");

    let mut pdf = unsigned_pdf_with_contents_placeholder(2048);
    let byte_range_marker = b"/ByteRange ";
    let byte_range_start = find_subslice(&pdf, byte_range_marker)
        .expect("ByteRange marker")
        + byte_range_marker.len();
    let contents_marker = find_subslice(&pdf[byte_range_start..], b"/Contents")
        .map(|offset| byte_range_start + offset)
        .expect("signature Contents marker");
    let contents_open = pdf[contents_marker..]
        .iter()
        .position(|byte| *byte == b'<')
        .map(|offset| contents_marker + offset)
        .expect("Contents opening delimiter");
    let contents_close = pdf[contents_open..]
        .iter()
        .position(|byte| *byte == b'>')
        .map(|offset| contents_open + offset)
        .expect("Contents closing delimiter");

    let range = format!(
        "[{:010} {:010} {:010} {:010}]",
        0,
        contents_open,
        contents_close + 1,
        pdf.len() - (contents_close + 1)
    );
    let range_bytes = range.as_bytes();
    let existing_close = pdf[byte_range_start..]
        .iter()
        .position(|byte| *byte == b']')
        .map(|offset| byte_range_start + offset + 1)
        .expect("ByteRange closing delimiter");
    assert_eq!(
        range_bytes.len(),
        existing_close - byte_range_start,
        "fixed-width ByteRange replacement"
    );
    pdf[byte_range_start..existing_close].copy_from_slice(range_bytes);

    let second_start = contents_close + 1;
    let mut covered = Vec::with_capacity(contents_open + pdf.len() - second_start);
    covered.extend_from_slice(&pdf[..contents_open]);
    covered.extend_from_slice(&pdf[second_start..]);

    let certs = Stack::new().expect("empty certificate stack");
    let flags = Pkcs7Flags::DETACHED | Pkcs7Flags::BINARY | Pkcs7Flags::NOATTR;
    let cms = Pkcs7::sign(&cert, &key, &certs, &covered, flags)
        .expect("create TEST ONLY detached CMS");
    let cms_der = cms.to_der().expect("serialize TEST ONLY CMS");
    let cms_hex = hex::encode(cms_der);
    let hex_capacity = contents_close - contents_open - 1;
    assert!(
        cms_hex.len() <= hex_capacity,
        "CMS does not fit reserved PDF /Contents"
    );
    let mut padded = vec![b'0'; hex_capacity];
    padded[..cms_hex.len()].copy_from_slice(cms_hex.as_bytes());
    pdf[contents_open + 1..contents_close].copy_from_slice(&padded);

    match variant {
        PdfByteRangeVariant::Valid => {}
        PdfByteRangeVariant::Tampered => {
            replace_same_length(&mut pdf, b"DSI Signed PDF v1", b"DSI Signed PDF v2");
        }
        PdfByteRangeVariant::MalformedByteRange => {
            let first_len = contents_open;
            let malformed = format!(
                "[{:010} {:010} {:010} {:010}]",
                0,
                first_len,
                first_len - 1,
                pdf.len() - second_start
            );
            assert_eq!(malformed.len(), existing_close - byte_range_start);
            pdf[byte_range_start..existing_close].copy_from_slice(malformed.as_bytes());
        }
    }

    PdfByteRangeFixture {
        pdf,
        trust_der: cert.to_der().expect("serialize TEST ONLY trust certificate"),
    }
}

fn unsigned_pdf_with_contents_placeholder(contents_bytes: usize) -> Vec<u8> {
    let byte_range_template = "[0000000000 0000000000 0000000000 0000000000]";
    let contents_placeholder = "00".repeat(contents_bytes);

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(
        1,
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [7 0 R] /SigFlags 3 >> >>"
            .to_vec(),
    );
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R /Annots [7 0 R] >>".to_vec(),
    );
    objects.insert(
        4,
        b"<< /Length 48 >>\nstream\nBT /F1 12 Tf 20 160 Td (DSI Signed PDF v1) Tj ET\nendstream"
            .to_vec(),
    );
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(6, b"<< /Producer (DSI Signature PoC) >>".to_vec());
    objects.insert(
        7,
        b"<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) /Rect [20 100 180 120] /V 8 0 R /P 3 0 R >>".to_vec(),
    );
    objects.insert(
        8,
        format!(
            "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached /ByteRange {byte_range_template} /Contents <{contents_placeholder}> /Reason (DSI TEST ONLY) >>"
        )
        .into_bytes(),
    );

    write_pdf(objects)
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-PoC\n".to_vec();
    let mut offsets = BTreeMap::<u32, usize>::new();

    for (id, object) in &objects {
        offsets.insert(*id, bytes.len());
        bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = bytes.len();
    let max_id = *objects.keys().max().expect("PDF objects");
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", max_id + 1).as_bytes(),
    );
    for id in 1..=max_id {
        if let Some(offset) = offsets.get(&id) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        } else {
            bytes.extend_from_slice(b"0000000000 65535 f \n");
        }
    }

    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    bytes
}

fn replace_same_length(bytes: &mut [u8], from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let offset = find_subslice(bytes, from).expect("signed text marker");
    bytes[offset..offset + from.len()].copy_from_slice(to);
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}
