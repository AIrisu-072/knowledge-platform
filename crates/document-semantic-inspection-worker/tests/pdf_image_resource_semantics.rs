use std::io::Write as _;
use std::ops::Range;

use document_semantic_inspection_worker::{AdapterProfile, PdfAdapter, SemanticAdapter};
use lopdf::Document;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

const PAGE_WIDTH: u32 = 612;
const PAGE_HEIGHT: u32 = 792;
const IMAGE_LEFT: u32 = 100;
const IMAGE_BOTTOM: u32 = 500;
const IMAGE_WIDTH: u32 = 96;
const IMAGE_HEIGHT: u32 = 96;
const IMAGE_SAMPLE_X: u32 = IMAGE_LEFT + IMAGE_WIDTH / 2;
const IMAGE_SAMPLE_Y: u32 = PAGE_HEIGHT - IMAGE_BOTTOM - IMAGE_HEIGHT / 2;

#[derive(Clone, Copy)]
enum ResourcePlacement {
    Page,
    ParentPages,
}

#[derive(Clone, Copy)]
enum ImagePlacement {
    Direct,
    FormXObject,
}

#[derive(Clone, Copy)]
enum ColorSpace {
    DeviceGray,
    DeviceRgb,
}

struct Fixture {
    bytes: Vec<u8>,
    image_sample: Vec<u8>,
    image_sample_range: Range<usize>,
    decode_array_range: Option<Range<usize>>,
    text_content: Vec<u8>,
    xref_offset: usize,
}

struct PdfObject {
    id: u32,
    body: Vec<u8>,
    image_sample_range: Option<Range<usize>>,
    decode_array_range: Option<Range<usize>>,
}

fn build_fixture(
    resource_placement: ResourcePlacement,
    image_placement: ImagePlacement,
    color_space: ColorSpace,
    image_sample: &[u8],
    decode: Option<[u8; 2]>,
) -> Fixture {
    assert!(IMAGE_LEFT + IMAGE_WIDTH <= PAGE_WIDTH);
    assert!(IMAGE_BOTTOM + IMAGE_HEIGHT <= PAGE_HEIGHT);
    assert_eq!(
        image_sample.len(),
        match color_space {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRgb => 3,
        },
        "one 8-bit pixel is expected"
    );

    let decode_array = decode.map(|values| format!("[{} {}]", values[0], values[1]));
    let decode_clause = decode_array
        .as_ref()
        .map(|array| format!(" /Decode {array}"))
        .unwrap_or_default();
    let image_prefix = format!(
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /{} /BitsPerComponent 8{}",
        match color_space {
            ColorSpace::DeviceGray => "DeviceGray",
            ColorSpace::DeviceRgb => "DeviceRGB",
        },
        decode_clause,
    );
    let (image_body, image_sample_range) = stream_body(&image_prefix, image_sample);
    let decode_array_range = decode_array.as_ref().and_then(|decode_text| {
        let decode_bytes = decode_text.as_bytes();
        let start = image_body
            .windows(decode_bytes.len())
            .position(|window| window == decode_bytes)?;
        Some(start..start + decode_bytes.len())
    });

    let image_resource = match image_placement {
        ImagePlacement::Direct => "/Im0 6 0 R".to_owned(),
        ImagePlacement::FormXObject => "/Fm0 7 0 R".to_owned(),
    };
    let resources = format!("<< /Font << /F1 5 0 R >> /XObject << {image_resource} >> >>");
    let pages_resources = match resource_placement {
        ResourcePlacement::Page => String::new(),
        ResourcePlacement::ParentPages => format!(" /Resources {resources}"),
    };
    let page_resources = match resource_placement {
        ResourcePlacement::Page => format!(" /Resources {resources}"),
        ResourcePlacement::ParentPages => String::new(),
    };

    let draw_operation = match image_placement {
        ImagePlacement::Direct => {
            format!("q {IMAGE_WIDTH} 0 0 {IMAGE_HEIGHT} {IMAGE_LEFT} {IMAGE_BOTTOM} cm /Im0 Do Q")
        }
        ImagePlacement::FormXObject => "q /Fm0 Do Q".to_owned(),
    };
    let text_content =
        format!("BT /F1 18 Tf 72 720 Td (PDF NATIVE TEXT FIXTURE) Tj ET\n{draw_operation}\n")
            .into_bytes();
    let (content_body, _) = stream_body("<<", &text_content);

    let mut objects = vec![
        PdfObject {
            id: 1,
            body: b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            image_sample_range: None,
            decode_array_range: None,
        },
        PdfObject {
            id: 2,
            body: format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1{pages_resources} >>"
            )
            .into_bytes(),
            image_sample_range: None,
            decode_array_range: None,
        },
        PdfObject {
            id: 3,
            body: format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] /Contents 4 0 R{page_resources} >>"
            )
            .into_bytes(),
            image_sample_range: None,
            decode_array_range: None,
        },
    ];
    objects.push(PdfObject {
        id: 4,
        body: content_body,
        image_sample_range: None,
        decode_array_range: None,
    });
    objects.push(PdfObject {
        id: 5,
        body: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        image_sample_range: None,
        decode_array_range: None,
    });
    objects.push(PdfObject {
        id: 6,
        body: image_body,
        image_sample_range: Some(image_sample_range),
        decode_array_range,
    });

    if matches!(image_placement, ImagePlacement::FormXObject) {
        let form_content = format!(
            "q {IMAGE_WIDTH} 0 0 {IMAGE_HEIGHT} {IMAGE_LEFT} {IMAGE_BOTTOM} cm /Im0 Do Q\n"
        );
        let form_prefix = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] /Resources << /XObject << /Im0 6 0 R >> >>"
        );
        let (form_body, _) = stream_body(&form_prefix, form_content.as_bytes());
        objects.push(PdfObject {
            id: 7,
            body: form_body,
            image_sample_range: None,
            decode_array_range: None,
        });
    }

    let (bytes, image_sample_range, decode_array_range, xref_offset) = serialize_pdf(objects);
    Fixture {
        bytes,
        image_sample: image_sample.to_vec(),
        image_sample_range: image_sample_range.expect("image stream sample range"),
        decode_array_range,
        text_content,
        xref_offset,
    }
}

fn stream_body(prefix: &str, data: &[u8]) -> (Vec<u8>, Range<usize>) {
    let mut body = format!("{prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
    let start = body.len();
    body.extend_from_slice(data);
    let range = start..body.len();
    body.extend_from_slice(b"\nendstream");
    (body, range)
}

fn serialize_pdf(
    objects: Vec<PdfObject>,
) -> (Vec<u8>, Option<Range<usize>>, Option<Range<usize>>, usize) {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize];
    let mut image_sample_range = None;
    let mut decode_array_range = None;

    for object in objects {
        assert_eq!(
            object.id as usize,
            offsets.len(),
            "PDF object ids are dense"
        );
        let object_offset = bytes.len();
        write!(bytes, "{} 0 obj\n", object.id).expect("write to Vec");
        let body_offset = bytes.len();
        bytes.extend_from_slice(&object.body);
        bytes.extend_from_slice(b"\nendobj\n");
        offsets.push(object_offset);

        if let Some(range) = object.image_sample_range {
            image_sample_range = Some(body_offset + range.start..body_offset + range.end);
        }
        if let Some(range) = object.decode_array_range {
            decode_array_range = Some(body_offset + range.start..body_offset + range.end);
        }
    }

    let xref_offset = bytes.len();
    write!(bytes, "xref\n0 {}\n0000000000 65535 f \n", offsets.len()).expect("write xref header");
    for offset in offsets.iter().skip(1) {
        write!(bytes, "{offset:010} 00000 n \n").expect("write xref entry");
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        offsets.len()
    )
    .expect("write trailer");

    (bytes, image_sample_range, decode_array_range, xref_offset)
}

fn assert_valid_pdfium_lopdf_input(fixture: &Fixture) {
    assert!(fixture.bytes[fixture.xref_offset..].starts_with(b"xref\n"));
    let parsed = Document::load_mem(&fixture.bytes).expect("fixture has a valid xref for lopdf");
    assert_eq!(parsed.get_pages().len(), 1);
}

fn inspect(fixture: &Fixture) -> document_semantic_inspection_core::SemanticFingerprint {
    assert_valid_pdfium_lopdf_input(fixture);
    PdfAdapter
        .inspect(&fixture.bytes, &AdapterProfile::default())
        .expect("both PDFium and lopdf should accept the native-text fixture")
        .semantic_fingerprint()
}

fn differing_offsets(left: &[u8], right: &[u8]) -> Vec<usize> {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect()
}

fn assert_only_pixel_sample_changed(left: &Fixture, right: &Fixture) {
    assert_eq!(
        left.text_content, right.text_content,
        "native text stream changed"
    );
    assert_eq!(left.image_sample.len(), right.image_sample.len());
    assert_ne!(left.image_sample, right.image_sample);
    let expected_changed_byte = left.image_sample_range.end - 1;
    assert_eq!(
        differing_offsets(&left.bytes, &right.bytes),
        vec![expected_changed_byte],
        "only the final raw image sample byte may change"
    );
}

#[test]
fn visible_image_inherited_from_parent_pages_resources_changes_fingerprint() {
    let baseline = build_fixture(
        ResourcePlacement::ParentPages,
        ImagePlacement::Direct,
        ColorSpace::DeviceRgb,
        &[0x00, 0x00, 0x00],
        None,
    );
    let mutant = build_fixture(
        ResourcePlacement::ParentPages,
        ImagePlacement::Direct,
        ColorSpace::DeviceRgb,
        &[0x00, 0x00, 0xFF],
        None,
    );
    assert_only_pixel_sample_changed(&baseline, &mutant);

    let baseline_fingerprint = inspect(&baseline);
    let mutant_fingerprint = inspect(&mutant);
    assert_eq!(
        render_rgb_pixel(&baseline.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y),
        [0, 0, 0],
        "inherited page-tree resources render the baseline image inside the page"
    );
    assert_eq!(
        render_rgb_pixel(&mutant.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y),
        [0, 0, 255],
        "inherited page-tree resources render the changed image inside the page"
    );
    assert_ne!(
        baseline_fingerprint, mutant_fingerprint,
        "a visible Image XObject referenced through inherited /Pages /Resources is semantic"
    );
}

#[test]
fn visible_image_nested_in_form_xobject_changes_fingerprint() {
    let baseline = build_fixture(
        ResourcePlacement::Page,
        ImagePlacement::FormXObject,
        ColorSpace::DeviceRgb,
        &[0x00, 0x00, 0x00],
        None,
    );
    let mutant = build_fixture(
        ResourcePlacement::Page,
        ImagePlacement::FormXObject,
        ColorSpace::DeviceRgb,
        &[0x00, 0x00, 0xFF],
        None,
    );
    assert_only_pixel_sample_changed(&baseline, &mutant);

    let baseline_fingerprint = inspect(&baseline);
    let mutant_fingerprint = inspect(&mutant);
    assert_eq!(
        render_rgb_pixel(&baseline.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y),
        [0, 0, 0],
        "Form XObject's image renders within its BBox and the page"
    );
    assert_eq!(
        render_rgb_pixel(&mutant.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y),
        [0, 0, 255],
        "Form XObject renders the changed image at the same visible position"
    );
    assert_ne!(
        baseline_fingerprint, mutant_fingerprint,
        "a visible Image XObject nested in a Form XObject is semantic"
    );
}

#[test]
fn image_decode_array_changes_visible_pixels_and_fingerprint() {
    let black = build_fixture(
        ResourcePlacement::Page,
        ImagePlacement::Direct,
        ColorSpace::DeviceGray,
        &[0x00],
        Some([0, 1]),
    );
    let white = build_fixture(
        ResourcePlacement::Page,
        ImagePlacement::Direct,
        ColorSpace::DeviceGray,
        &[0x00],
        Some([1, 0]),
    );
    assert_eq!(black.text_content, white.text_content);
    assert_eq!(black.image_sample, [0x00]);
    assert_eq!(black.image_sample, white.image_sample);
    let decode_range = black
        .decode_array_range
        .as_ref()
        .expect("baseline Decode array");
    assert_eq!(white.decode_array_range.as_ref(), Some(decode_range));
    assert_eq!(
        differing_offsets(&black.bytes, &white.bytes),
        vec![decode_range.start + 1, decode_range.start + 3],
        "only /Decode's two sample mapping values may change"
    );

    let black_fingerprint = inspect(&black);
    let white_fingerprint = inspect(&white);
    let rendered_black = render_rgb_pixel(&black.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y);
    let rendered_white = render_rgb_pixel(&white.bytes, IMAGE_SAMPLE_X, IMAGE_SAMPLE_Y);
    assert_eq!(rendered_black, [0, 0, 0], "[0 1] maps sample 0x00 to black");
    assert_eq!(
        rendered_white,
        [255, 255, 255],
        "[1 0] maps sample 0x00 to white"
    );
    assert_ne!(
        black_fingerprint, white_fingerprint,
        "visible black/white changes from /Decode are semantic"
    );
}

fn render_rgb_pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 3] {
    let pdfium = Pdfium::default();
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium loads rendered fixture");
    let page = document.pages().get(0).expect("first page");
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(612)
                .set_format(pdfium_render::prelude::PdfBitmapFormat::BGRA),
        )
        .expect("render image fixture");
    let width = bitmap.width() as usize;
    let pixels = bitmap.as_rgba_bytes();
    let offset = (y as usize * width + x as usize) * 4;
    pixels[offset..offset + 3]
        .try_into()
        .expect("RGB pixel from PDFium bitmap")
}
