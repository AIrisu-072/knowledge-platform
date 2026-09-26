use std::env;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use document_semantic_inspection_core::SemanticFingerprint;
use document_semantic_inspection_worker::{AdapterProfile, PdfAdapter, SemanticAdapter};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use sha2::{Digest, Sha256};

const PAGE_PIXELS: i32 = 200;
const NATIVE_TEXT: &str = "SAME NATIVE TEXT";
const TEXT_PAINT: &str = "BT /F1 12 Tf 12 180 Td (SAME NATIVE TEXT) Tj ET";
const IMAGE_PAINT: &str = "q 200 0 0 200 0 0 cm /Im0 Do Q";
const IMAGE_SAMPLE: [u8; 3] = [255, 0, 0];

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

struct PdfFixture {
    bytes: Vec<u8>,
    content: Vec<u8>,
    text_paint: &'static str,
    image_paint: &'static str,
    image_xobject: Vec<u8>,
}

fn native_text_pdf(text_before_image: bool) -> PdfFixture {
    let content = if text_before_image {
        format!("{TEXT_PAINT}\n{IMAGE_PAINT}")
    } else {
        format!("{IMAGE_PAINT}\n{TEXT_PAINT}")
    };
    let image_xobject = image_xobject(IMAGE_SAMPLE);
    let objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_PIXELS} {PAGE_PIXELS}] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 4 0 R >>"
            )
            .into_bytes(),
        ),
        (4, stream(b"", content.as_bytes())),
        (
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ),
        (6, image_xobject.clone()),
    ];

    PdfFixture {
        bytes: serialize_pdf(objects),
        content: content.into_bytes(),
        text_paint: TEXT_PAINT,
        image_paint: IMAGE_PAINT,
        image_xobject,
    }
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

fn expected_pdfium_sha256() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7"
    } else {
        panic!("no qualified PDFium binary exists for this test platform")
    }
}

fn pdfium_library_sha256() -> String {
    let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
        .expect("focused PDF test requires the pinned PDFIUM_DYNAMIC_LIB_PATH");
    let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
    Sha256::digest(fs::read(library).expect("read pinned PDFium native library"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assert_pinned_pdfium() {
    assert_eq!(
        pdfium_library_sha256(),
        expected_pdfium_sha256(),
        "isolated PDFium raster process must load the qualified native binary"
    );
}

fn fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("valid native-text PDF must be inspected")
        .semantic_fingerprint()
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

fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("CI must set the pinned PDFIUM_DYNAMIC_LIB_PATH");
        let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
        let bindings = Pdfium::bind_to_library(library).expect("load pinned PDFium library");
        Pdfium::new(bindings)
    })
}

fn render_rgba_in_process(bytes: &[u8]) -> Vec<u8> {
    let document = pdfium()
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium must load the synthetic native-text PDF");
    let page = document.pages().get(0).expect("first page");
    let text = page.text().expect("native text page").all();
    assert_eq!(text.trim(), NATIVE_TEXT, "both variants retain native text");

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
    fs::write(output_path, render_rgba_in_process(&bytes)).expect("write PDFium raster");
}

#[test]
fn reversing_overlapping_text_and_image_paint_order_changes_semantics() {
    let text_then_image = native_text_pdf(true);
    let image_then_text = native_text_pdf(false);

    assert_eq!(text_then_image.text_paint, image_then_text.text_paint);
    assert_eq!(text_then_image.image_paint, image_then_text.image_paint);
    assert_eq!(text_then_image.image_xobject, image_then_text.image_xobject);
    assert_eq!(
        text_then_image.content,
        format!("{TEXT_PAINT}\n{IMAGE_PAINT}").as_bytes()
    );
    assert_eq!(
        image_then_text.content,
        format!("{IMAGE_PAINT}\n{TEXT_PAINT}").as_bytes()
    );

    assert_pinned_pdfium();
    let text_then_image_fingerprint = fingerprint(&text_then_image.bytes);
    let image_then_text_fingerprint = fingerprint(&image_then_text.bytes);

    let text_then_image_raster = render_rgba(&text_then_image.bytes);
    let image_then_text_raster = render_rgba(&image_then_text.bytes);
    assert_ne!(
        text_then_image_raster, image_then_text_raster,
        "fixture proof: PDFium 151.0.7881.0 must render the reversed overlapping paint order differently"
    );

    assert_ne!(
        text_then_image_fingerprint, image_then_text_fingerprint,
        "semantic contract: overlapping text/image paint order changes reader-visible content"
    );
}
