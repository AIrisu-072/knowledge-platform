use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn nested_form_clip_transform_changes_visible_image_semantics() {
    let outer_scaled = nested_clipped_image_pdf(true);
    let inner_scaled = nested_clipped_image_pdf(false);
    assert_eq!(
        normalize_form_matrices(&outer_scaled),
        normalize_form_matrices(&inner_scaled),
        "the two PDF files must differ only in which Form owns the scale"
    );

    let outer_fingerprint = inspect(&outer_scaled);
    let inner_fingerprint = inspect(&inner_scaled);
    let outer_pixel = render_pixel(&outer_scaled, 140, 60);
    let inner_pixel = render_pixel(&inner_scaled, 140, 60);
    assert_eq!(
        outer_pixel,
        [0, 0, 0],
        "image remains inside the outer clip"
    );
    assert_eq!(inner_pixel, [255, 255, 255], "outer clip hides this pixel");

    assert_ne!(
        outer_fingerprint, inner_fingerprint,
        "the Form BBox clip transform changes visible pixels despite the same final image CTM"
    );
}

fn inspect(bytes: &[u8]) -> [u8; 32] {
    let output = PdfAdapter
        .inspect(bytes, &InspectionProfile::default())
        .expect("valid native-text PDF must parse");
    let projection: Value =
        serde_json::from_slice(&output.semantic_projection).expect("semantic JSON");
    assert_eq!(projection["pages"][0]["text"].as_str(), Some("stable"));
    fingerprint(&output.semantic_projection)
}

fn render_pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 3] {
    let pdfium = Pdfium::default();
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium must load the nested Form fixture");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(200))
        .expect("PDFium must render the nested Form fixture");
    assert_eq!(bitmap.width(), 200);
    assert_eq!(bitmap.height(), 200);
    let pixels = bitmap.as_rgba_bytes();
    pixels[(y * 200 + x) * 4..(y * 200 + x) * 4 + 3]
        .try_into()
        .expect("RGB pixel")
}

fn nested_clipped_image_pdf(outer_scaled: bool) -> Vec<u8> {
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Fm0 Do Q";
    let outer_matrix = if outer_scaled {
        "[2 0 0 2 0 0]"
    } else {
        "[1 0 0 1 0 0]"
    };
    let inner_matrix = if outer_scaled {
        "[1 0 0 1 0 0]"
    } else {
        "[2 0 0 2 0 0]"
    };
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 6 0 R >> >> /Contents 4 0 R >>".to_vec());
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Matrix {outer_matrix} /Resources << /XObject << /Fm1 7 0 R >> >>"),
            b"q /Fm1 Do Q",
        ),
    );
    objects.insert(
        7,
        stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 2 2] /Matrix {inner_matrix} /Resources << /XObject << /Im0 8 0 R >> >>"),
            b"q /Im0 Do Q",
        ),
    );
    objects.insert(
        8,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        ),
    );
    objects.insert(9, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects)
}

fn normalize_form_matrices(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = bytes.to_vec();
    for matrix in [
        b"/Matrix [2 0 0 2 0 0]".as_slice(),
        b"/Matrix [1 0 0 1 0 0]".as_slice(),
    ] {
        let replacement = b"/Matrix [x x x x x x]";
        let position = normalized
            .windows(matrix.len())
            .position(|window| window == matrix)
            .expect("each matrix occurs once");
        normalized[position..position + matrix.len()].copy_from_slice(replacement);
    }
    normalized
}

fn stream_object(extra_dictionary: &str, content: &[u8]) -> Vec<u8> {
    let mut value = format!(
        "<< {extra_dictionary} /Length {} >>\nstream\n",
        content.len()
    )
    .into_bytes();
    value.extend_from_slice(content);
    value.extend_from_slice(b"\nendstream");
    value
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-clip\n".to_vec();
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
            "trailer\n<< /Size {} /Root 1 0 R /Info 9 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    bytes
}
