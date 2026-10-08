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
    native_text_pdf_with_prefix("", paint_ops, unused_pixel)
}

fn native_text_pdf_with_prefix(prefix: &str, paint_ops: &str, unused_pixel: [u8; 3]) -> Vec<u8> {
    let content = format!("{prefix}\nBT /F1 12 Tf 12 180 Td ({NATIVE_TEXT}) Tj ET\n{paint_ops}\n");
    native_text_pdf_content(&content, unused_pixel)
}

fn native_text_pdf_content(content: &str, unused_pixel: [u8; 3]) -> Vec<u8> {
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
        "isolated PDFium raster proof failed ({}): stdout={} stderr={}",
        child.status,
        String::from_utf8_lossy(&child.stdout[..child.stdout.len().min(4096)]),
        String::from_utf8_lossy(&child.stderr[..child.stderr.len().min(4096)])
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

#[test]
fn explicit_default_graphics_state_preserves_existing_pdf_identity() {
    let baseline = native_text_pdf(BASE_PAINT, UNUSED_PIXEL);
    let explicit = native_text_pdf(
        "0 g 0 G 1 w 0 J 0 j 10 M [] 0 d\nq 40 0 0 40 20 20 cm /Im0 Do Q",
        UNUSED_PIXEL,
    );
    assert_eq!(render_rgba(&baseline), render_rgba(&explicit));
    assert_eq!(fingerprint(&baseline), fingerprint(&explicit));
}

#[test]
fn vector_line_change_is_verified_by_pixels_and_semantic_identity() {
    let baseline = native_text_pdf("20 20 m 80 20 l S", UNUSED_PIXEL);
    let changed = native_text_pdf("20 20 m 120 20 l S", UNUSED_PIXEL);
    assert_ne!(render_rgba(&baseline), render_rgba(&changed));
    assert_ne!(fingerprint(&baseline), fingerprint(&changed));
}

#[test]
fn equivalent_line_numbers_and_redundant_state_have_same_identity() {
    let baseline = native_text_pdf("20 20 m 80 20 l S", UNUSED_PIXEL);
    let changed = native_text_pdf("q 0 G 1.0 w 20.0 20 m 80 20.00 l S Q", UNUSED_PIXEL);
    assert_eq!(render_rgba(&baseline), render_rgba(&changed));
    assert_eq!(fingerprint(&baseline), fingerprint(&changed));
}

#[test]
fn vector_stroke_colour_is_not_dropped_from_identity() {
    let baseline = native_text_pdf("0 0 0 RG 20 20 m 80 20 l S", UNUSED_PIXEL);
    let changed = native_text_pdf("1 0 0 RG 20 20 m 80 20 l S", UNUSED_PIXEL);
    assert_ne!(render_rgba(&baseline), render_rgba(&changed));
    assert_ne!(fingerprint(&baseline), fingerprint(&changed));
}

#[test]
fn enclosing_page_clip_does_not_change_unclipped_content_identity() {
    let baseline = native_text_pdf("20 20 m 80 20 l S", UNUSED_PIXEL);
    let clipped = native_text_pdf("0 0 200 200 re W* n 20 20 m 80 20 l S", UNUSED_PIXEL);
    assert_eq!(render_rgba(&baseline), render_rgba(&clipped));
    assert_eq!(fingerprint(&baseline), fingerprint(&clipped));
}

#[test]
fn enclosing_clip_before_text_preserves_pixels_and_identity() {
    let baseline = native_text_pdf("20 20 m 80 20 l S", UNUSED_PIXEL);
    let clipped =
        native_text_pdf_with_prefix("0 0 200 200 re W* n", "20 20 m 80 20 l S", UNUSED_PIXEL);
    assert_eq!(render_rgba(&baseline), render_rgba(&clipped));
    assert_eq!(fingerprint(&baseline), fingerprint(&clipped));
}

#[test]
fn clip_hiding_native_text_is_rejected_after_rendered_difference_proof() {
    let baseline = native_text_pdf("20 20 m 80 20 l S", UNUSED_PIXEL);
    let clipped =
        native_text_pdf_with_prefix("0 0 200 100 re W* n", "20 20 m 80 20 l S", UNUSED_PIXEL);
    assert_ne!(render_rgba(&baseline), render_rgba(&clipped));
    let error = PdfAdapter
        .inspect(&clipped, &AdapterProfile::default())
        .expect_err("clipped-away text must not count as visible native text");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
    assert_eq!(error.message(), "pdf_clip_does_not_enclose_paint");
}

#[test]
fn nondefault_stroked_text_is_not_silently_unchanged() {
    let baseline = native_text_pdf_with_prefix("1 Tr", "", UNUSED_PIXEL);
    let widened = native_text_pdf_with_prefix("1 Tr 8 w", "", UNUSED_PIXEL);
    assert_ne!(render_rgba(&baseline), render_rgba(&widened));
    let error = PdfAdapter
        .inspect(&widened, &AdapterProfile::default())
        .expect_err("stroke-state text requires a qualified semantic mapping");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn default_colour_space_override_is_explicitly_unsupported() {
    let bytes = native_text_pdf("0 G 20 20 m 80 20 l S", UNUSED_PIXEL);
    let mut document = lopdf::Document::load_mem(&bytes).expect("synthetic PDF");
    let page_id = document.get_pages()[&1];
    let resources = document
        .get_dictionary_mut(page_id)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap();
    let mut spaces = lopdf::Dictionary::new();
    spaces.set("DefaultGray", lopdf::Object::Name(b"DeviceGray".to_vec()));
    resources.set("ColorSpace", spaces);
    let mut changed = Vec::new();
    document.save_to(&mut changed).unwrap();
    let error = PdfAdapter
        .inspect(&changed, &AdapterProfile::default())
        .expect_err("resource-dependent default colors require explicit qualification");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn vector_occluding_only_one_text_run_is_never_accepted_as_unchanged() {
    let first = "BT /F1 12 Tf 12 180 Td (SAME ) Tj ET";
    let middle = "BT /F1 12 Tf 52 180 Td (NATIVE ) Tj ET";
    let last = "BT /F1 12 Tf 100 180 Td (TEXT) Tj ET";
    let cover = "q 1 g 51 177 48 15 re f Q";
    let visible =
        native_text_pdf_content(&format!("{first}\n{cover}\n{middle}\n{last}"), UNUSED_PIXEL);
    let obscured =
        native_text_pdf_content(&format!("{first}\n{middle}\n{cover}\n{last}"), UNUSED_PIXEL);
    assert_ne!(render_rgba(&visible), render_rgba(&obscured));
    for input in [&visible, &obscured] {
        let error = PdfAdapter
            .inspect(input, &AdapterProfile::default())
            .expect_err("overlapping text/vector ordering needs a qualified association");
        assert_eq!(
            error.code(),
            document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
        );
        assert_eq!(error.message(), "pdf_vector_text_overlap_unqualified");
    }
}

const SEPARATED_CUBIC: &str = "0.75 w 1 j 21 170 m 9.954 170 1 178.954 1 190 c S";

#[test]
fn subdivided_curve_hulls_can_prove_separation_from_native_text() {
    let plain = native_text_pdf("", UNUSED_PIXEL);
    let curved = native_text_pdf(SEPARATED_CUBIC, UNUSED_PIXEL);
    assert_ne!(render_rgba(&plain), render_rgba(&curved));
    assert_ne!(fingerprint(&plain), fingerprint(&curved));
}

#[test]
fn bounded_curve_geometry_change_remains_a_semantic_difference() {
    let first = native_text_pdf(SEPARATED_CUBIC, UNUSED_PIXEL);
    let second = native_text_pdf(
        "0.75 w 1 j 21 168 m 9.954 168 1 176.954 1 188 c S",
        UNUSED_PIXEL,
    );
    assert_ne!(render_rgba(&first), render_rgba(&second));
    assert_ne!(fingerprint(&first), fingerprint(&second));
}

#[test]
fn curved_white_fill_that_occludes_text_still_fails_closed() {
    let plain = native_text_pdf("", UNUSED_PIXEL);
    let covered = native_text_pdf(
        "1 g 21 170 m 9.954 170 1 178.954 1 190 c 21 190 l h f",
        UNUSED_PIXEL,
    );
    assert_ne!(render_rgba(&plain), render_rgba(&covered));
    let error = PdfAdapter
        .inspect(&covered, &AdapterProfile::default())
        .expect_err("filled curve hides text");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

const CROP_VECTOR_PAINT: &str = "160 20 m 180 20 l S";

fn crop_vector_pdf(crop: [i64; 4]) -> Vec<u8> {
    crop_pdf(&native_text_pdf(CROP_VECTOR_PAINT, UNUSED_PIXEL), crop)
}

fn crop_pdf(bytes: &[u8], crop: [i64; 4]) -> Vec<u8> {
    let mut document = lopdf::Document::load_mem(bytes).unwrap();
    let page_id = document.get_pages()[&1];
    document.get_dictionary_mut(page_id).unwrap().set(
        "CropBox",
        crop.into_iter()
            .map(lopdf::Object::Integer)
            .collect::<Vec<_>>(),
    );
    let mut output = Vec::new();
    document.save_to(&mut output).unwrap();
    output
}

#[test]
fn page_crop_hiding_new_vector_content_is_not_silently_unchanged() {
    let baseline = native_text_pdf(CROP_VECTOR_PAINT, UNUSED_PIXEL);
    let cropped = crop_vector_pdf([0, 0, 140, 200]);
    assert_ne!(render_rgba(&baseline), render_rgba(&cropped));
    let without_vector = crop_pdf(&native_text_pdf("", UNUSED_PIXEL), [0, 0, 140, 200]);
    assert_eq!(
        render_rgba(&without_vector),
        render_rgba(&cropped),
        "same viewport proves the vector is completely hidden"
    );
    let error = PdfAdapter
        .inspect(&cropped, &AdapterProfile::default())
        .expect_err("implicit page crop must be proven non-cutting");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn explicit_enclosing_page_crop_preserves_vector_identity() {
    let baseline = native_text_pdf(CROP_VECTOR_PAINT, UNUSED_PIXEL);
    let cropped = crop_vector_pdf([0, 0, 200, 200]);
    assert_eq!(render_rgba(&baseline), render_rgba(&cropped));
    assert_eq!(fingerprint(&baseline), fingerprint(&cropped));
}

#[test]
fn inherited_page_crop_is_also_required_to_preserve_vector_content() {
    let bytes = crop_vector_pdf([0, 0, 140, 200]);
    let mut document = lopdf::Document::load_mem(&bytes).unwrap();
    let page_id = document.get_pages()[&1];
    let page = document.get_dictionary_mut(page_id).unwrap();
    let parent = page.get(b"Parent").unwrap().as_reference().unwrap();
    let crop = page.remove(b"CropBox").unwrap();
    document
        .get_dictionary_mut(parent)
        .unwrap()
        .set("CropBox", crop);
    let mut changed = Vec::new();
    document.save_to(&mut changed).unwrap();
    let error = PdfAdapter
        .inspect(&changed, &AdapterProfile::default())
        .expect_err("inherited crop must be checked");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn malformed_page_crop_is_not_accepted_through_native_fallback() {
    let mut document = lopdf::Document::load_mem(&crop_vector_pdf([0, 0, 200, 200])).unwrap();
    let page_id = document.get_pages()[&1];
    document
        .get_dictionary_mut(page_id)
        .unwrap()
        .set("CropBox", "InvalidCrop");
    let mut changed = Vec::new();
    document.save_to(&mut changed).unwrap();
    let error = PdfAdapter
        .inspect(&changed, &AdapterProfile::default())
        .expect_err("malformed crop must fail closed");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::ParserDisagreement
    );
}

#[test]
fn media_box_intersection_of_a_larger_crop_preserves_identity() {
    let baseline = native_text_pdf(CROP_VECTOR_PAINT, UNUSED_PIXEL);
    let cropped = crop_vector_pdf([-20, -20, 400, 400]);
    assert_eq!(render_rgba(&baseline), render_rgba(&cropped));
    assert_eq!(fingerprint(&baseline), fingerprint(&cropped));
}

#[test]
fn nonzero_crop_cannot_silently_drop_text_outside_native_extraction_frame() {
    let raw = native_text_pdf("160 70 m 180 70 l S", UNUSED_PIXEL);
    let cropped = crop_pdf(&raw, [0, 50, 200, 200]);
    let error = PdfAdapter
        .inspect(&cropped, &AdapterProfile::default())
        .expect_err("unqualified text extraction frame must be rejected");
    assert_eq!(
        error.code(),
        document_semantic_inspection_worker::WorkerFailureCode::UnsupportedSemanticConstruct
    );
}
