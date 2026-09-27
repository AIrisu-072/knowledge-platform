use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PdfAdapter,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use std::collections::BTreeMap;

#[test]
fn optional_content_on_an_image_cannot_be_accepted_without_visibility_semantics() {
    let visible = image_pdf(false);
    let hidden = image_pdf(true);
    PdfAdapter
        .inspect(&visible, &InspectionProfile::default())
        .expect("ordinary visible-image PDF remains supported");

    assert_eq!(render_pixel(&visible), [0, 0, 0]);
    assert_eq!(render_pixel(&hidden), [255, 255, 255]);

    let outcome = PdfAdapter.inspect(&hidden, &InspectionProfile::default());
    assert_eq!(
        outcome.err().map(|error| error.code()),
        Some(ErrorCode::UnsupportedSemanticConstruct),
        "Image XObject /OC needs visibility evaluation or fail-closed handling"
    );
}

fn render_pixel(bytes: &[u8]) -> [u8; 3] {
    let pdfium = Pdfium::default();
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("valid PDF");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(200))
        .expect("PDFium raster");
    let pixels = bitmap.as_rgba_bytes();
    pixels[(140 * 200 + 60) * 4..(140 * 200 + 60) * 4 + 3]
        .try_into()
        .expect("RGB pixel")
}

fn image_pdf(hidden: bool) -> Vec<u8> {
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    let catalog = if hidden {
        b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [8 0 R] /D << /OFF [8 0 R] /Order [8 0 R] >> >> >>".to_vec()
    } else {
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()
    };
    objects.insert(1, catalog);
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 4 0 R >>".to_vec());
    objects.insert(
        4,
        stream(
            "",
            b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Im0 Do Q",
        ),
    );
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    let image_dict = if hidden {
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /OC 8 0 R"
    } else {
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8"
    };
    objects.insert(6, stream(image_dict, &[0]));
    objects.insert(7, b"<< /Producer (DSI fixture) >>".to_vec());
    if hidden {
        objects.insert(8, b"<< /Type /OCG /Name (Hidden) >>".to_vec());
    }
    let mut bytes = b"%PDF-1.7\n%DSI-OC\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (id, object) in &objects {
        offsets.insert(*id, bytes.len());
        bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    let max = *objects.keys().max().expect("objects");
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for id in 1..=max {
        bytes.extend_from_slice(format!("{:010} 00000 n \n", offsets[&id]).as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    bytes
}

fn stream(dictionary: &str, content: &[u8]) -> Vec<u8> {
    let mut value = format!("<< {dictionary} /Length {} >>\nstream\n", content.len()).into_bytes();
    value.extend_from_slice(content);
    value.extend_from_slice(b"\nendstream");
    value
}
