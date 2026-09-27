use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use std::collections::BTreeMap;

const STREAM_A: &[u8] = b"1 0 0 1 10 0 cm 2 0 0 2 0 0 cm /Im0 Do";
const STREAM_B: &[u8] = b"2 0 0 2 20 0 cm /Im0 Do";

#[test]
fn noncommutative_cm_order_changes_raster_and_semantic_fingerprint() {
    let a = pdf_with_image_stream(STREAM_A);
    let b = pdf_with_image_stream(STREAM_B);

    let output_a = PdfAdapter
        .inspect(&a, &InspectionProfile::default())
        .expect("stream A is a qualified native-text PDF");
    let output_b = PdfAdapter
        .inspect(&b, &InspectionProfile::default())
        .expect("stream B is a qualified native-text PDF");

    let pdfium = Pdfium::default();
    let raster_a = render_rgba(&pdfium, &a);
    let raster_b = render_rgba(&pdfium, &b);
    assert_eq!(raster_a.dimensions, raster_b.dimensions);
    let changed_pixels = raster_a
        .rgba
        .chunks_exact(4)
        .zip(raster_b.rgba.chunks_exact(4))
        .filter(|(left, right)| left != right)
        .count();
    assert!(
        changed_pixels > 0,
        "PDFium must confirm the two noncommutative cm sequences render differently"
    );
    eprintln!("PDFium raster differs at {changed_pixels} pixels");

    assert_ne!(
        fingerprint(&output_a.semantic_projection),
        fingerprint(&output_b.semantic_projection),
        "PDFium-confirmed visible placement changed, but the PoC semantic fingerprint stayed equal"
    );
}

struct Raster {
    dimensions: (i32, i32),
    rgba: Vec<u8>,
}

fn render_rgba(pdfium: &Pdfium, input: &[u8]) -> Raster {
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium must load the generated PDF");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(400))
        .expect("PDFium must render the generated PDF");
    Raster {
        dimensions: (bitmap.width(), bitmap.height()),
        rgba: bitmap.as_rgba_bytes(),
    }
}

fn pdf_with_image_stream(image_stream: &[u8]) -> Vec<u8> {
    let mut page_content = b"BT /F1 12 Tf 10 80 Td (stable) Tj ET\n".to_vec();
    page_content.extend_from_slice(image_stream);

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", &page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &[0, 0, 0],
        ),
    );
    objects.insert(7, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(7))
}

fn stream_object(dictionary: &str, content: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dictionary} /Length {} >>\nstream\n", content.len()).into_bytes();
    object.extend_from_slice(content);
    object.extend_from_slice(b"\nendstream");
    object
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>, root: u32, info: Option<u32>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-PoC\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (id, object) in &objects {
        offsets.insert(*id, bytes.len());
        bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let xref = bytes.len();
    let max_id = objects.keys().next_back().copied().unwrap_or(0);
    bytes.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..=max_id {
        match offsets.get(&id) {
            Some(offset) => {
                bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
            }
            None => bytes.extend_from_slice(b"0000000000 00000 f \n"),
        }
    }

    let info = info
        .map(|id| format!(" /Info {id} 0 R"))
        .unwrap_or_default();
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {root} 0 R{info} >>\n",
            max_id + 1
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}
