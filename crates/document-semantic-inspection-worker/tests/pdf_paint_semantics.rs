use std::env;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use document_semantic_inspection_core::SemanticFingerprint;
use document_semantic_inspection_worker::{AdapterProfile, PdfAdapter, SemanticAdapter};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

const PAGE_PIXELS: i32 = 200;
const NATIVE_TEXT: &str = "SAME NATIVE TEXT";
const BASE_PAINT: &str = "q 40 0 0 40 20 20 cm /Im0 Do Q";
const UNUSED_PIXEL: [u8; 3] = [0, 0, 0];

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

fn native_text_pdf(paint_ops: &str, unused_pixel: [u8; 3]) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 12 180 Td ({NATIVE_TEXT}) Tj ET\n{paint_ops}\n");
    let resources = b"/Font << /F1 5 0 R >> /XObject << /Im0 6 0 R /Im1 7 0 R /Unused 8 0 R >>";

    let objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_PIXELS} {PAGE_PIXELS}] /Resources << {} >> /Contents 4 0 R >>",
                String::from_utf8_lossy(resources)
            )
            .into_bytes(),
        ),
        (4, stream(b"", content.as_bytes())),
        (
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ),
        (6, image_xobject([255, 0, 0])),
        (7, image_xobject([0, 0, 255])),
        (8, image_xobject(unused_pixel)),
    ];
    serialize_pdf(objects)
}

fn image_xobject(pixel: [u8; 3]) -> Vec<u8> {
    stream(
        b"/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &pixel,
    )
}

fn stream(dictionary_entries: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "<< {} /Length {} >>\nstream\n",
        String::from_utf8_lossy(dictionary_entries),
        data.len()
    )
    .into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

fn serialize_pdf(objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize];

    for (id, body) in objects {
        assert_eq!(id as usize, offsets.len(), "PDF object ids are dense");
        offsets.push(bytes.len());
        writeln!(bytes, "{id} 0 obj").expect("write object header");
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = bytes.len();
    write!(bytes, "xref\n0 {}\n0000000000 65535 f \n", offsets.len()).expect("write xref header");
    for offset in offsets.iter().skip(1) {
        writeln!(bytes, "{offset:010} 00000 n ").expect("write xref entry");
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        offsets.len()
    )
    .expect("write trailer");
    bytes
}

fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("CI must set the pinned PDFIUM_DYNAMIC_LIB_PATH");
        let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
        let bindings = Pdfium::bind_to_library(library).expect("load pinned PDFium library");
        Pdfium::new(bindings)
    })
}

fn render_rgba(bytes: &[u8]) -> Vec<u8> {
    let directory = tempfile::tempdir().expect("temporary raster fixture directory");
    let input_path = directory.path().join("input.pdf");
    let output_path = directory.path().join("raster.rgba");
    fs::write(&input_path, bytes).expect("write synthetic PDF for raster proof");

    // pdfium-render keeps one process-global binding. Render out-of-process so
    // this test binary can also initialize the adapter's independent binding.
    let child = Command::new(env::current_exe().expect("test binary path"))
        .args(["--exact", "pdfium_raster_child_entrypoint", "--ignored"])
        .env("DSI_PDF_RASTER_INPUT", &input_path)
        .env("DSI_PDF_RASTER_OUTPUT", &output_path)
        .output()
        .expect("start isolated PDFium raster proof");
    assert!(
        child.status.success(),
        "isolated PDFium raster proof failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    fs::read(output_path).expect("read isolated PDFium raster")
}

fn render_rgba_in_process(bytes: &[u8]) -> Vec<u8> {
    let document = pdfium()
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium must load the synthetic native-text PDF");
    let page = document.pages().get(0).expect("first page");
    let text = page.text().expect("native text page").all();
    assert_eq!(text.trim(), NATIVE_TEXT, "all variants retain native text");

    page.render_with_config(&PdfRenderConfig::new().set_fixed_size(PAGE_PIXELS, PAGE_PIXELS))
        .expect("PDFium must rasterize the synthetic page")
        .as_rgba_bytes()
}

#[test]
#[ignore = "invoked in a child process to isolate pdfium-render's global binding"]
fn pdfium_raster_child_entrypoint() {
    let input_path = env::var_os("DSI_PDF_RASTER_INPUT").expect("child input path");
    let output_path = env::var_os("DSI_PDF_RASTER_OUTPUT").expect("child output path");
    let bytes = fs::read(input_path).expect("read synthetic PDF");
    fs::write(output_path, render_rgba_in_process(&bytes)).expect("write raster output");
}

fn fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("valid native-text PDF must be inspected")
        .semantic_fingerprint()
}

fn assert_visible_change_changes_fingerprint(label: &str, changed_paint: &str) {
    let baseline = native_text_pdf(BASE_PAINT, UNUSED_PIXEL);
    let changed = native_text_pdf(changed_paint, UNUSED_PIXEL);
    assert_ne!(
        render_rgba(&baseline),
        render_rgba(&changed),
        "fixture proof: {label} must change PDFium reader-visible pixels"
    );
    assert_ne!(
        fingerprint(&baseline),
        fingerprint(&changed),
        "semantic contract: {label} changes reader-visible image content"
    );
}

fn differing_byte_count(left: &[u8], right: &[u8]) -> usize {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .filter(|(left, right)| left != right)
        .count()
}

#[test]
fn removing_a_do_invocation_changes_reader_visible_image_semantics() {
    assert_visible_change_changes_fingerprint("removing /Im0 Do", "");
}

#[test]
fn changing_cm_placement_changes_reader_visible_image_semantics() {
    assert_visible_change_changes_fingerprint(
        "moving the image through cm translation",
        "q 40 0 0 40 90 20 cm /Im0 Do Q",
    );
}

#[test]
fn changing_cm_geometry_changes_reader_visible_image_semantics() {
    assert_visible_change_changes_fingerprint(
        "resizing the image through cm scale",
        "q 60 0 0 40 20 20 cm /Im0 Do Q",
    );
}

#[test]
fn changing_do_invocation_count_changes_reader_visible_image_semantics() {
    assert_visible_change_changes_fingerprint(
        "adding a second /Im0 Do invocation",
        "q 40 0 0 40 20 20 cm /Im0 Do Q\nq 40 0 0 40 90 20 cm /Im0 Do Q",
    );
}

#[test]
fn changing_do_z_order_changes_reader_visible_image_semantics() {
    let baseline_paint = "q 40 0 0 40 50 50 cm /Im0 Do Q\nq 40 0 0 40 50 50 cm /Im1 Do Q";
    let changed_paint = "q 40 0 0 40 50 50 cm /Im1 Do Q\nq 40 0 0 40 50 50 cm /Im0 Do Q";
    let baseline = native_text_pdf(baseline_paint, UNUSED_PIXEL);
    let changed = native_text_pdf(changed_paint, UNUSED_PIXEL);
    assert_ne!(
        render_rgba(&baseline),
        render_rgba(&changed),
        "fixture proof: swapping opaque XObject order changes PDFium pixels"
    );
    assert_ne!(
        fingerprint(&baseline),
        fingerprint(&changed),
        "semantic contract: PDF paint order determines reader-visible pixels"
    );
}

#[test]
fn changing_an_unused_xobject_does_not_change_reader_visible_semantics() {
    let baseline = native_text_pdf(BASE_PAINT, UNUSED_PIXEL);
    let changed = native_text_pdf(BASE_PAINT, [0, 0, 255]);
    assert_eq!(
        differing_byte_count(&baseline, &changed),
        1,
        "only one sample byte in the uninvoked /Unused image changes"
    );
    assert_eq!(
        render_rgba(&baseline),
        render_rgba(&changed),
        "an uninvoked XObject must not alter PDFium reader-visible pixels"
    );
    assert_eq!(
        fingerprint(&baseline),
        fingerprint(&changed),
        "resource bytes with no reachable Do invocation are not reader-visible semantics"
    );
}
