use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use lopdf::{Dictionary, Document};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn removing_visible_image_do_changes_pixels_but_not_identity() {
    let painted = pdf_with_image_paint(true, false);
    let omitted = pdf_with_image_paint(false, false);

    assert_eq!(page_resources(&painted), page_resources(&omitted));
    let painted_fingerprint = inspect_native_text_pdf(&painted, "painted");
    let omitted_fingerprint = inspect_native_text_pdf(&omitted, "omitted Do");
    let painted_pixels = render_rgba(&painted);
    let omitted_pixels = render_rgba(&omitted);
    assert_eq!(rgb_pixel(&painted_pixels), [0, 0, 0]);
    assert_eq!(rgb_pixel(&omitted_pixels), [255, 255, 255]);
    assert_ne!(painted_pixels, omitted_pixels);

    assert_ne!(
        painted_fingerprint, omitted_fingerprint,
        "removing a visible image paint operation must change PDF identity"
    );
}

#[test]
fn adding_unused_image_xobject_does_not_change_pixels_or_identity() {
    let baseline = pdf_with_image_paint(true, false);
    let with_unused_xobject = pdf_with_image_paint(true, true);

    assert_ne!(
        page_resources(&baseline),
        page_resources(&with_unused_xobject)
    );
    let baseline_fingerprint = inspect_native_text_pdf(&baseline, "baseline");
    let unused_fingerprint = inspect_native_text_pdf(&with_unused_xobject, "unused XObject");
    let baseline_pixels = render_rgba(&baseline);
    let unused_pixels = render_rgba(&with_unused_xobject);
    assert_eq!(rgb_pixel(&baseline_pixels), [0, 0, 0]);
    assert_eq!(rgb_pixel(&unused_pixels), [0, 0, 0]);
    assert_eq!(baseline_pixels, unused_pixels);

    assert_eq!(
        baseline_fingerprint, unused_fingerprint,
        "an uninvoked image XObject must not change PDF identity"
    );
}

fn inspect_native_text_pdf(input: &[u8], label: &str) -> [u8; 32] {
    let output = PdfAdapter
        .inspect(input, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{label} native-text PDF must parse: {error}"));
    let projection: Value = serde_json::from_slice(&output.semantic_projection)
        .unwrap_or_else(|error| panic!("{label} semantic projection must be JSON: {error}"));
    assert_eq!(
        projection["pages"][0]["text"].as_str(),
        Some("stable"),
        "{label} must retain the same native text"
    );
    fingerprint(&output.semantic_projection)
}

fn render_rgba(input: &[u8]) -> Vec<u8> {
    let pdfium = Pdfium::default();
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium must load the native-text paint fixture");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(200))
        .expect("PDFium must render the native-text paint fixture");
    assert_eq!(bitmap.width(), 200);
    assert_eq!(bitmap.height(), 200);
    bitmap.as_rgba_bytes()
}

fn rgb_pixel(pixels: &[u8]) -> [u8; 3] {
    let offset = (140 * 200 + 60) * 4;
    pixels[offset..offset + 3]
        .try_into()
        .expect("RGB sample inside the image paint area")
}

fn page_resources(input: &[u8]) -> Dictionary {
    let document = Document::load_mem(input).expect("synthetic PDF must be structurally valid");
    let page_id = *document.get_pages().values().next().expect("first page");
    document
        .get_dictionary(page_id)
        .expect("page dictionary")
        .get_deref(b"Resources", &document)
        .expect("page Resources")
        .as_dict()
        .expect("Resources dictionary")
        .clone()
}

fn pdf_with_image_paint(paint_image: bool, include_unused_xobject: bool) -> Vec<u8> {
    let mut page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\n".to_vec();
    if paint_image {
        page_content.extend_from_slice(b"q 80 0 0 80 20 20 cm /Im0 Do Q\n");
    }

    let xobjects = if include_unused_xobject {
        "/XObject << /Im0 6 0 R /Unused 7 0 R >>"
    } else {
        "/XObject << /Im0 6 0 R >>"
    };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> {xobjects} >> /Contents 4 0 R >>"
    );

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(3, page.into_bytes());
    objects.insert(4, stream_object("", &page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0x00],
        ),
    );
    if include_unused_xobject {
        objects.insert(
            7,
            stream_object(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[0xff],
            ),
        );
    }
    write_pdf(objects, 1)
}

fn stream_object(dictionary: &str, contents: &[u8]) -> Vec<u8> {
    let mut object =
        format!("<< {dictionary} /Length {} >>\nstream\n", contents.len()).into_bytes();
    object.extend_from_slice(contents);
    object.extend_from_slice(b"\nendstream");
    object
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>, root: u32) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-PoC\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (id, object) in &objects {
        offsets.insert(*id, bytes.len());
        bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let xref = bytes.len();
    let max = *objects.keys().max().expect("PDF objects");
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for id in 1..=max {
        match offsets.get(&id) {
            Some(offset) => {
                bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
            }
            None => bytes.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {root} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    bytes
}
