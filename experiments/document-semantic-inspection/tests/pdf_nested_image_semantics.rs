use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn changed_pixel_inside_visible_form_xobject_changes_identity() {
    let baseline = pdf_with_form_image(0x00);
    let changed = pdf_with_form_image(0xff);
    assert_one_byte_diff(&baseline, &changed);

    let baseline_fingerprint = inspect_native_text_pdf(&baseline, "baseline");
    let changed_fingerprint = inspect_native_text_pdf(&changed, "changed");
    assert_eq!(render_rgb_pixel(&baseline), [0, 0, 0]);
    assert_eq!(render_rgb_pixel(&changed), [255, 255, 255]);
    assert_ne!(
        baseline_fingerprint, changed_fingerprint,
        "a visible image nested inside a Form XObject is semantic PDF content"
    );
}

#[test]
fn changed_pixel_from_page_tree_inherited_resources_changes_identity() {
    let baseline = pdf_with_inherited_image(0x00);
    let changed = pdf_with_inherited_image(0xff);
    assert_one_byte_diff(&baseline, &changed);

    let baseline_fingerprint = inspect_native_text_pdf(&baseline, "baseline");
    let changed_fingerprint = inspect_native_text_pdf(&changed, "changed");
    assert_eq!(render_rgb_pixel(&baseline), [0, 0, 0]);
    assert_eq!(render_rgb_pixel(&changed), [255, 255, 255]);
    assert_ne!(
        baseline_fingerprint, changed_fingerprint,
        "a visible image resolved through inherited page-tree Resources is semantic PDF content"
    );
}

#[test]
fn image_decode_array_changes_identity_when_raw_sample_is_unchanged() {
    let baseline = pdf_with_decoded_direct_image("[0 1]");
    let changed = pdf_with_decoded_direct_image("[1 0]");
    assert_only_decode_array_differs(&baseline, &changed);

    let baseline_fingerprint = inspect_native_text_pdf(&baseline, "baseline");
    let changed_fingerprint = inspect_native_text_pdf(&changed, "changed");
    assert_eq!(render_rgb_pixel(&baseline), [0, 0, 0]);
    assert_eq!(render_rgb_pixel(&changed), [255, 255, 255]);
    assert_ne!(
        baseline_fingerprint, changed_fingerprint,
        "Decode reversal changes the visible pixel even when the raw 0x00 sample is identical"
    );
}

#[test]
fn named_indexed_palette_changes_visible_image_identity() {
    let black = pdf_with_indexed_palette([0, 0, 0]);
    let blue = pdf_with_indexed_palette([0, 0, 255]);
    assert_eq!(black.len(), blue.len());
    assert_eq!(
        black
            .iter()
            .zip(&blue)
            .filter(|(left, right)| left != right)
            .count(),
        2,
        "only the two hexadecimal digits of the palette's blue channel change"
    );
    let black_fingerprint = inspect_native_text_pdf(&black, "black");
    let blue_fingerprint = inspect_native_text_pdf(&blue, "blue");
    assert_eq!(render_rgb_pixel(&black), [0, 0, 0]);
    assert_eq!(render_rgb_pixel(&blue), [0, 0, 255]);
    assert_ne!(
        black_fingerprint, blue_fingerprint,
        "a named ColorSpace palette is visible image semantics"
    );
}

#[test]
fn visible_inline_image_changes_identity_or_fails_closed() {
    let _ = inspect_native_text_pdf(&pdf_with_form_image(0x00), "PDFium setup");
    let black = pdf_with_inline_image([0, 0, 0]);
    let blue = pdf_with_inline_image([0, 0, 255]);
    assert_one_byte_diff(&black, &blue);

    let black_result = PdfAdapter.inspect(&black, &InspectionProfile::default());
    let blue_result = PdfAdapter.inspect(&blue, &InspectionProfile::default());
    assert_eq!(render_rgb_pixel(&black), [0, 0, 0]);
    assert_eq!(render_rgb_pixel(&blue), [0, 0, 255]);
    match (black_result, blue_result) {
        (Ok(black), Ok(blue)) => assert_ne!(
            fingerprint(&black.semantic_projection),
            fingerprint(&blue.semantic_projection),
            "visible inline image pixels must change identity"
        ),
        (Err(black), Err(blue)) => {
            assert_eq!(black.code(), blue.code());
            assert_eq!(
                black.code(),
                document_semantic_inspection_poc::ErrorCode::UnsupportedSemanticConstruct
            );
        }
        (black, blue) => panic!("inline image handling must agree: {black:?} / {blue:?}"),
    }
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

fn render_rgb_pixel(input: &[u8]) -> [u8; 3] {
    let pdfium = Pdfium::default();
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium must load the visible image fixture");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(200))
        .expect("PDFium must render the visible image fixture");
    let pixels = bitmap.as_rgba_bytes();
    let offset = (140 * bitmap.width() as usize + 60) * 4;
    pixels[offset..offset + 3]
        .try_into()
        .expect("RGB sample at the center of the rendered image")
}

fn assert_one_byte_diff(baseline: &[u8], changed: &[u8]) {
    assert_eq!(baseline.len(), changed.len());
    let differences: Vec<_> = baseline
        .iter()
        .zip(changed)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect();
    assert_eq!(differences.len(), 1, "only the image sample may change");
    let index = differences[0];
    assert_eq!(baseline[index], 0x00);
    assert_eq!(changed[index], 0xff);
}

fn assert_only_decode_array_differs(baseline: &[u8], changed: &[u8]) {
    assert_eq!(
        replace_decode_array(baseline, b"/Decode [0 1]"),
        replace_decode_array(changed, b"/Decode [1 0]")
    );
}

fn replace_decode_array(input: &[u8], array: &[u8]) -> Vec<u8> {
    let mut normalized = input.to_vec();
    let matches: Vec<_> = input
        .windows(array.len())
        .enumerate()
        .filter_map(|(index, window)| (window == array).then_some(index))
        .collect();
    assert_eq!(matches.len(), 1, "PDF must have exactly one Decode array");
    normalized[matches[0]..matches[0] + array.len()].copy_from_slice(b"/Decode [x x]");
    normalized
}

fn pdf_with_form_image(sample: u8) -> Vec<u8> {
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Fm0 Do Q";
    let form_content = b"q 1 0 0 1 0 0 cm /Im0 Do Q";
    let image_content = [sample];

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /Im0 7 0 R >> >>",
            form_content,
        ),
    );
    objects.insert(
        7,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &image_content,
        ),
    );
    objects.insert(8, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(8))
}

fn pdf_with_inherited_image(sample: u8) -> Vec<u8> {
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Im0 Do Q";
    let image_content = [sample];

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(
        2,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> >>".to_vec(),
    );
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &image_content,
        ),
    );
    objects.insert(7, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(7))
}

fn pdf_with_decoded_direct_image(decode_array: &str) -> Vec<u8> {
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Im0 Do Q";
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            &format!(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Decode {decode_array}"
            ),
            &[0x00],
        ),
    );
    objects.insert(7, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(7))
}

fn pdf_with_indexed_palette(palette: [u8; 3]) -> Vec<u8> {
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm /Im0 Do Q";
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> /ColorSpace << /CS1 [/Indexed /DeviceRGB 0 <{:02x}{:02x}{:02x}>] >> >> /Contents 4 0 R >>",
            palette[0], palette[1], palette[2]
        )
        .into_bytes(),
    );
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        6,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /CS1 /BitsPerComponent 8",
            &[0x00],
        ),
    );
    objects.insert(7, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(7))
}

fn pdf_with_inline_image(pixel: [u8; 3]) -> Vec<u8> {
    let mut page_content =
        b"BT /F1 12 Tf 20 160 Td (stable) Tj ET\nq 80 0 0 80 20 20 cm BI /W 1 /H 1 /CS /RGB /BPC 8 ID "
            .to_vec();
    page_content.extend_from_slice(&pixel);
    page_content.extend_from_slice(b" EI Q");
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", &page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(6, b"<< /Producer (DSI fixture) >>".to_vec());
    write_pdf(objects, 1, Some(6))
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
    let max = *objects.keys().max().expect("PDF must contain objects");
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for id in 1..=max {
        match offsets.get(&id) {
            Some(offset) => bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()),
            None => bytes.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }

    let mut trailer = format!("trailer\n<< /Size {} /Root {root} 0 R", max + 1);
    if let Some(info) = info {
        trailer.push_str(&format!(" /Info {info} 0 R"));
    }
    trailer.push_str(&format!(" >>\nstartxref\n{xref}\n%%EOF\n"));
    bytes.extend_from_slice(trailer.as_bytes());
    bytes
}
