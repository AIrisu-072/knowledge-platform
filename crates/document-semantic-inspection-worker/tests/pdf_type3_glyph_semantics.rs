use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

const PAGE_PIXELS: i32 = 200;
const RED_SAMPLE: [u8; 3] = [255, 0, 0];
const BLUE_SAMPLE: [u8; 3] = [0, 0, 255];

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

#[test]
fn type3_glyph_image_changes_pdfium_raster_and_semantic_fingerprint() {
    let red = pdf_with_type3_glyph_image(RED_SAMPLE);
    let blue = pdf_with_type3_glyph_image(BLUE_SAMPLE);
    assert_only_glyph_image_sample_differs(&red, &blue);
    assert_pinned_pdfium_binary();

    let (red_text, red_raster, blue_text, blue_raster) = pdfium_probe_pair(&red, &blue);
    assert_eq!(
        red_text.trim(),
        "A",
        "PDFium must extract native Type 3 text"
    );
    assert_eq!(
        blue_text.trim(),
        "A",
        "the image mutation must preserve extracted text"
    );
    assert_eq!(red_raster.len(), blue_raster.len());
    assert_ne!(
        red_raster, blue_raster,
        "PDFium 151.0.7881.0 must render the invoked Type 3 glyph image mutation"
    );

    let red_result = PdfAdapter.inspect(&red, &AdapterProfile::default());
    let blue_result = PdfAdapter.inspect(&blue, &AdapterProfile::default());
    match (red_result, blue_result) {
        (Ok(red), Ok(blue)) => assert_ne!(
            red.semantic_fingerprint(),
            blue.semantic_fingerprint(),
            "an invoked image inside a Type 3 glyph is visible PDF image content"
        ),
        (Err(red), Err(blue)) => {
            assert_eq!(red.code(), WorkerFailureCode::UnsupportedSemanticConstruct);
            assert_eq!(blue.code(), WorkerFailureCode::UnsupportedSemanticConstruct);
        }
        (red, blue) => panic!(
            "Type 3 glyph image variants must both be distinguished or both fail closed: {red:?} / {blue:?}"
        ),
    }
}

fn assert_only_glyph_image_sample_differs(red: &[u8], blue: &[u8]) {
    assert_eq!(red.len(), blue.len());
    let normalize = |input: &[u8], sample: [u8; 3]| {
        let matches: Vec<_> = input
            .windows(sample.len())
            .enumerate()
            .filter_map(|(index, window)| (window == sample).then_some(index))
            .collect();
        assert_eq!(matches.len(), 1, "one raw Type 3 image sample is expected");
        let mut normalized = input.to_vec();
        normalized[matches[0]..matches[0] + sample.len()].fill(0x7f);
        normalized
    };
    assert_eq!(normalize(red, RED_SAMPLE), normalize(blue, BLUE_SAMPLE));
}

fn pdfium_probe_pair(red: &[u8], blue: &[u8]) -> (String, Vec<u8>, String, Vec<u8>) {
    let directory = tempfile::tempdir().expect("temporary Type 3 fixture directory");
    let red_input = directory.path().join("red.pdf");
    let blue_input = directory.path().join("blue.pdf");
    let red_text = directory.path().join("red.txt");
    let blue_text = directory.path().join("blue.txt");
    let red_raster = directory.path().join("red.rgba");
    let blue_raster = directory.path().join("blue.rgba");
    fs::write(&red_input, red).expect("write red Type 3 fixture");
    fs::write(&blue_input, blue).expect("write blue Type 3 fixture");

    // Isolate PDFium's process-global native binding from the adapter's binding.
    let child = Command::new(env::current_exe().expect("integration test executable"))
        .args([
            "--exact",
            "pdfium_type3_probe_entrypoint",
            "--ignored",
            "--nocapture",
        ])
        .env("DSI_TYPE3_RED_INPUT", &red_input)
        .env("DSI_TYPE3_BLUE_INPUT", &blue_input)
        .env("DSI_TYPE3_RED_TEXT", &red_text)
        .env("DSI_TYPE3_BLUE_TEXT", &blue_text)
        .env("DSI_TYPE3_RED_RASTER", &red_raster)
        .env("DSI_TYPE3_BLUE_RASTER", &blue_raster)
        .output()
        .expect("start isolated pinned PDFium Type 3 probe");
    assert!(
        child.status.success(),
        "isolated PDFium probe failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );

    (
        fs::read_to_string(red_text).expect("red PDFium text output"),
        fs::read(red_raster).expect("red PDFium raster output"),
        fs::read_to_string(blue_text).expect("blue PDFium text output"),
        fs::read(blue_raster).expect("blue PDFium raster output"),
    )
}

#[test]
#[ignore = "invoked in a child process to isolate PDFium's native binding"]
fn pdfium_type3_probe_entrypoint() {
    assert_pinned_pdfium_binary();
    for (input_key, text_key, raster_key) in [
        (
            "DSI_TYPE3_RED_INPUT",
            "DSI_TYPE3_RED_TEXT",
            "DSI_TYPE3_RED_RASTER",
        ),
        (
            "DSI_TYPE3_BLUE_INPUT",
            "DSI_TYPE3_BLUE_TEXT",
            "DSI_TYPE3_BLUE_RASTER",
        ),
    ] {
        let input = fs::read(env::var_os(input_key).expect("child PDF input path"))
            .expect("read Type 3 PDF fixture");
        let (text, raster) = extract_and_render(&input);
        fs::write(env::var_os(text_key).expect("child text output path"), text)
            .expect("write PDFium text output");
        fs::write(
            env::var_os(raster_key).expect("child raster output path"),
            raster,
        )
        .expect("write PDFium raster output");
    }
}

fn extract_and_render(input: &[u8]) -> (String, Vec<u8>) {
    let document = pdfium()
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium must load the synthetic Type 3 native-text PDF");
    let page = document.pages().get(0).expect("first page");
    let text = page.text().expect("PDFium native text page").all();
    let raster = page
        .render_with_config(&PdfRenderConfig::new().set_fixed_size(PAGE_PIXELS, PAGE_PIXELS))
        .expect("PDFium must render the synthetic Type 3 page")
        .as_rgba_bytes();
    (text, raster)
}

fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("focused PDF test requires pinned PDFIUM_DYNAMIC_LIB_PATH");
        let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
        let bindings = Pdfium::bind_to_library(library).expect("load pinned PDFium 7881");
        Pdfium::new(bindings)
    })
}

fn assert_pinned_pdfium_binary() {
    let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
        .expect("focused PDF test requires pinned PDFIUM_DYNAMIC_LIB_PATH");
    let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
    let observed: String = Sha256::digest(fs::read(library).expect("read PDFium native library"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let expected = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7"
    } else {
        panic!("no qualified PDFium binary exists for this test platform")
    };
    assert_eq!(
        observed, expected,
        "test must use the exact PDFium 7881 binary"
    );
}

fn pdf_with_type3_glyph_image(sample: [u8; 3]) -> Vec<u8> {
    let page_content = b"BT /F1 72 Tf 20 20 Td <41> Tj ET";
    let char_proc = b"500 0 d0\nq 500 0 0 500 0 0 cm /Im0 Do Q";
    let cmap = b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend";

    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
    );
    objects.insert(4, stream_object("", page_content));
    objects.insert(
        5,
        b"<< /Type /Font /Subtype /Type3 /Name /DSIType3 /FontBBox [0 0 500 500] /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /g 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /g] >> /FirstChar 65 /LastChar 65 /Widths [500] /Resources << /ProcSet [/PDF /ImageC] /XObject << /Im0 8 0 R >> >> /ToUnicode 9 0 R >>".to_vec(),
    );
    objects.insert(6, stream_object("", char_proc));
    objects.insert(7, b"<< /Producer (DSI Type 3 fixture) >>".to_vec());
    objects.insert(
        8,
        stream_object(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &sample,
        ),
    );
    objects.insert(9, stream_object("", cmap));
    write_pdf(objects, 1, Some(7))
}

fn stream_object(dictionary: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>, root: u32, info: Option<u32>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-Type3\n".to_vec();
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
            Some(offset) => bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()),
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
