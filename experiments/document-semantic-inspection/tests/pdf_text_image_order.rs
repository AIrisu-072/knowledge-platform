mod support;

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use lopdf::{Dictionary, Document, Object, Stream};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use serde_json::Value;
use std::{process::Command, sync::OnceLock};

const PDFIUM_CHILD_MODE: &str = "DSI_TEXT_IMAGE_ORDER_PDFIUM_CHILD";
const PDFIUM_CHILD_RENDER: &str = "render";
const TEXT_PAINT: &[u8] = b"BT /F1 12 Tf 12 180 Td (stable) Tj ET\n";
const IMAGE_PAINT: &[u8] = b"q 200 0 0 200 0 0 cm /Im0 Do Q\n";

#[test]
fn reversed_overlapping_text_image_paint_order_changes_identity() {
    let text_then_image = pdf_with_paint_order(true);
    let image_then_text = pdf_with_paint_order(false);
    assert_valid_reversed_fixtures(&text_then_image, &image_then_text);

    // PDFium's native binding is process-global. Keep its direct render probe
    // separate from PdfAdapter, which initializes its own pinned binding.
    run_pdfium_render_probe();

    let text_then_image_output = inspect_native_text_pdf(&text_then_image, "text then image");
    let image_then_text_output = inspect_native_text_pdf(&image_then_text, "image then text");
    let text_then_image_projection: Value =
        serde_json::from_slice(&text_then_image_output.semantic_projection)
            .expect("text-then-image projection is JSON");
    let image_then_text_projection: Value =
        serde_json::from_slice(&image_then_text_output.semantic_projection)
            .expect("image-then-text projection is JSON");

    assert_eq!(text_then_image_projection["pages"][0]["text"], "stable");
    assert_eq!(image_then_text_projection["pages"][0]["text"], "stable");
    assert_eq!(
        text_then_image_projection["pages"][0]["text"],
        image_then_text_projection["pages"][0]["text"],
        "both paint orders retain the same native text"
    );

    let text_then_image_fingerprint = fingerprint(&text_then_image_output.semantic_projection);
    let image_then_text_fingerprint = fingerprint(&image_then_text_output.semantic_projection);
    assert_ne!(
        text_then_image_fingerprint, image_then_text_fingerprint,
        "reversing the paint order of overlapping native text and an image must change PDF identity"
    );
}

#[test]
fn pdfium_render_probe() {
    if std::env::var(PDFIUM_CHILD_MODE).as_deref() != Ok(PDFIUM_CHILD_RENDER) {
        return;
    }

    let text_then_image = pdf_with_paint_order(true);
    let image_then_text = pdf_with_paint_order(false);
    let pdfium = pdfium();
    let text_then_image_raster = render_pdfium_fixture(pdfium, &text_then_image);
    let image_then_text_raster = render_pdfium_fixture(pdfium, &image_then_text);

    assert_eq!(text_then_image_raster.text, "stable");
    assert_eq!(image_then_text_raster.text, "stable");
    assert_eq!(text_then_image_raster.dimensions, (200, 200));
    assert_eq!(image_then_text_raster.dimensions, (200, 200));
    assert_eq!(text_then_image_raster.pixel(100, 100), [255, 0, 0]);
    assert_eq!(image_then_text_raster.pixel(100, 100), [255, 0, 0]);

    let changed_pixels = text_then_image_raster
        .rgba
        .chunks_exact(4)
        .zip(image_then_text_raster.rgba.chunks_exact(4))
        .filter(|(left, right)| left != right)
        .count();
    assert!(
        changed_pixels > 0,
        "pinned PDFium must render the same text/image geometry differently when their paint order is reversed"
    );
    eprintln!("PDFium raster differs at {changed_pixels} pixels");
}

fn pdf_with_paint_order(text_before_image: bool) -> Vec<u8> {
    let qualified_text_pdf = support::pdf_fixture::minimal_text_pdf("stable", "DSI fixture");
    let mut document = Document::load_mem(&qualified_text_pdf)
        .expect("qualified fixture support must produce a valid native-text PDF");
    let page_id = *document
        .get_pages()
        .values()
        .next()
        .expect("qualified native-text PDF has one page");
    let contents_id = document
        .get_dictionary(page_id)
        .expect("page dictionary")
        .get(b"Contents")
        .expect("page Contents")
        .as_reference()
        .expect("Contents is an indirect stream");

    let mut image_dictionary = Dictionary::new();
    image_dictionary.set("Type", Object::Name(b"XObject".to_vec()));
    image_dictionary.set("Subtype", Object::Name(b"Image".to_vec()));
    image_dictionary.set("Width", Object::Integer(1));
    image_dictionary.set("Height", Object::Integer(1));
    image_dictionary.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    image_dictionary.set("BitsPerComponent", Object::Integer(8));
    let image_id = document.add_object(Stream::new(image_dictionary, vec![255, 0, 0]));

    let page = document
        .get_dictionary_mut(page_id)
        .expect("mutable page dictionary");
    let resources = page
        .get_mut(b"Resources")
        .expect("page Resources")
        .as_dict_mut()
        .expect("Resources is a dictionary");
    let mut xobjects = Dictionary::new();
    xobjects.set("Im0", Object::Reference(image_id));
    resources.set("XObject", Object::Dictionary(xobjects));

    let mut page_content = Vec::new();
    if text_before_image {
        page_content.extend_from_slice(TEXT_PAINT);
        page_content.extend_from_slice(IMAGE_PAINT);
    } else {
        page_content.extend_from_slice(IMAGE_PAINT);
        page_content.extend_from_slice(TEXT_PAINT);
    }
    document
        .get_object_mut(contents_id)
        .expect("Contents stream object")
        .as_stream_mut()
        .expect("Contents is a stream")
        .set_plain_content(page_content);

    let mut pdf = Vec::new();
    document.save_to(&mut pdf).expect("write a valid PDF xref");
    pdf
}

fn assert_valid_reversed_fixtures(text_then_image: &[u8], image_then_text: &[u8]) {
    let text_then_image_document = Document::load_mem(text_then_image)
        .expect("text-then-image fixture has a valid PDF structure and xref");
    let image_then_text_document = Document::load_mem(image_then_text)
        .expect("image-then-text fixture has a valid PDF structure and xref");
    let text_then_image_page = *text_then_image_document
        .get_pages()
        .values()
        .next()
        .expect("text-then-image page");
    let image_then_text_page = *image_then_text_document
        .get_pages()
        .values()
        .next()
        .expect("image-then-text page");

    assert_eq!(
        text_then_image_document.get_pages().len(),
        image_then_text_document.get_pages().len()
    );
    assert_eq!(
        text_then_image_document.get_pages().len(),
        1,
        "qualified fixture support starts from one native-text page"
    );
    let text_then_image_page_dictionary = text_then_image_document
        .get_dictionary(text_then_image_page)
        .expect("text-then-image page dictionary");
    let image_then_text_page_dictionary = image_then_text_document
        .get_dictionary(image_then_text_page)
        .expect("image-then-text page dictionary");
    assert_eq!(
        text_then_image_page_dictionary
            .get(b"MediaBox")
            .expect("text-then-image MediaBox"),
        image_then_text_page_dictionary
            .get(b"MediaBox")
            .expect("image-then-text MediaBox"),
        "both fixtures keep the same page geometry"
    );
    assert_eq!(
        page_content(&text_then_image_document, text_then_image_page),
        [TEXT_PAINT, IMAGE_PAINT].concat()
    );
    assert_eq!(
        page_content(&image_then_text_document, image_then_text_page),
        [IMAGE_PAINT, TEXT_PAINT].concat()
    );
    let image = image_xobject(&text_then_image_document, text_then_image_page);
    assert_eq!(
        image
            .dict
            .get(b"Width")
            .expect("image Width")
            .as_i64()
            .expect("image Width is an integer"),
        1
    );
    assert_eq!(
        image
            .dict
            .get(b"Height")
            .expect("image Height")
            .as_i64()
            .expect("image Height is an integer"),
        1
    );
    assert_eq!(
        image
            .dict
            .get(b"ColorSpace")
            .expect("image ColorSpace")
            .as_name()
            .expect("image ColorSpace is a name"),
        b"DeviceRGB"
    );
    assert_eq!(
        image
            .dict
            .get(b"BitsPerComponent")
            .expect("image BitsPerComponent")
            .as_i64()
            .expect("image BitsPerComponent is an integer"),
        8
    );
    assert_eq!(image.content, [255, 0, 0]);
    assert_eq!(
        image,
        image_xobject(&image_then_text_document, image_then_text_page),
        "both fixtures use the same 1x1 red image XObject"
    );
}

fn page_content(document: &Document, page_id: (u32, u16)) -> Vec<u8> {
    let content_ids = document.get_page_contents(page_id);
    assert_eq!(content_ids.len(), 1, "fixture has one page content stream");
    document
        .get_object(content_ids[0])
        .expect("page Contents object")
        .as_stream()
        .expect("page Contents stream")
        .get_plain_content()
        .expect("page Contents decodes")
}

fn image_xobject(document: &Document, page_id: (u32, u16)) -> Stream {
    let page = document.get_dictionary(page_id).expect("page dictionary");
    let resources = page
        .get_deref(b"Resources", document)
        .expect("page Resources")
        .as_dict()
        .expect("Resources dictionary");
    let xobjects = resources
        .get_deref(b"XObject", document)
        .expect("XObject resources")
        .as_dict()
        .expect("XObject dictionary");
    let image_id = xobjects
        .get(b"Im0")
        .expect("Im0 image resource")
        .as_reference()
        .expect("Im0 is an indirect image object");
    document
        .get_object(image_id)
        .expect("image object")
        .as_stream()
        .expect("image is a stream")
        .clone()
}

fn inspect_native_text_pdf(
    input: &[u8],
    label: &str,
) -> document_semantic_inspection_poc::AdapterOutput {
    PdfAdapter
        .inspect(input, &InspectionProfile::default())
        .unwrap_or_else(|error| {
            panic!("{label} fixture must remain a valid native-text PDF: {error}")
        })
}

struct Raster {
    dimensions: (i32, i32),
    rgba: Vec<u8>,
    text: String,
}

impl Raster {
    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let offset = (y * self.dimensions.0 as usize + x) * 4;
        self.rgba[offset..offset + 3]
            .try_into()
            .expect("pixel inside rendered PDF")
    }
}

fn render_pdfium_fixture(pdfium: &Pdfium, input: &[u8]) -> Raster {
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("pinned PDFium loads the valid native-text PDF");
    let page = document.pages().get(0).expect("first page");
    let text = page
        .text()
        .expect("PDFium extracts native text")
        .all()
        .trim()
        .to_owned();
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(200))
        .expect("pinned PDFium renders the native-text PDF");
    Raster {
        dimensions: (bitmap.width(), bitmap.height()),
        rgba: bitmap.as_rgba_bytes(),
        text,
    }
}

fn pdfium() -> &'static Pdfium {
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
    PDFIUM.get_or_init(|| {
        let directory = std::env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("Task 8 tests set the verified PDFIUM_DYNAMIC_LIB_PATH");
        let path = Pdfium::pdfium_platform_library_name_at_path(&directory);
        let bindings =
            Pdfium::bind_to_library(path).expect("load the qualified PDFium 7881 library");
        Pdfium::new(bindings)
    })
}

fn run_pdfium_render_probe() {
    let executable = std::env::current_exe().expect("integration test executable path");
    let output = Command::new(executable)
        .args(["--exact", "pdfium_render_probe", "--nocapture"])
        .env(PDFIUM_CHILD_MODE, PDFIUM_CHILD_RENDER)
        .output()
        .expect("run PDFium's isolated process-global binding probe");
    assert!(
        output.status.success(),
        "isolated PDFium render probe failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
