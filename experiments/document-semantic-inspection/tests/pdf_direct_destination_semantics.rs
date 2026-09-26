use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use lopdf::Document;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use serde_json::Value;
use std::{collections::BTreeMap, process::Command, sync::OnceLock};

const CHILD_MODE: &str = "DSI_DIRECT_DESTINATION_TEST_CHILD";

#[test]
fn direct_destination_without_action_changes_identity_or_fails_closed() {
    let target_first_page = pdf_with_direct_destination(3);
    let target_second_page = pdf_with_direct_destination(4);

    assert_only_destination_reference_changes(&target_first_page, &target_second_page);
    assert_direct_dest_fixture(&target_first_page, 3);
    assert_direct_dest_fixture(&target_second_page, 4);

    run_child_test("pdfium_direct_destination_probe", "pdfium");
    run_child_test("adapter_direct_destination_semantics", "adapter");
}

#[test]
fn pdfium_direct_destination_probe() {
    if std::env::var(CHILD_MODE).as_deref() != Ok("pdfium") {
        return;
    }

    let target_first_page = pdf_with_direct_destination(3);
    let target_second_page = pdf_with_direct_destination(4);

    assert_eq!(
        pdfium_page_texts(&target_first_page),
        vec!["stable".to_owned(), "stable".to_owned()]
    );
    assert_eq!(
        pdfium_page_texts(&target_second_page),
        vec!["stable".to_owned(), "stable".to_owned()]
    );
    assert_eq!(
        render_pages(&target_first_page),
        render_pages(&target_second_page),
        "changing only /Dest must not change either page's rendered content"
    );

    assert_eq!(pdfium_direct_destination_page(&target_first_page), 0);
    assert_eq!(pdfium_direct_destination_page(&target_second_page), 1);
}

#[test]
fn adapter_direct_destination_semantics() {
    if std::env::var(CHILD_MODE).as_deref() != Ok("adapter") {
        return;
    }

    let target_first_page = pdf_with_direct_destination(3);
    let target_second_page = pdf_with_direct_destination(4);
    let left = PdfAdapter.inspect(&target_first_page, &InspectionProfile::default());
    let right = PdfAdapter.inspect(&target_second_page, &InspectionProfile::default());

    match (left, right) {
        (Ok(left), Ok(right)) => {
            let left_fingerprint = fingerprint(&left.semantic_projection);
            let right_fingerprint = fingerprint(&right.semantic_projection);
            assert_ne!(
                left_fingerprint, right_fingerprint,
                "direct /Dest target page is semantic PDF link meaning"
            );

            let mut left_projection: Value = serde_json::from_slice(&left.semantic_projection)
                .expect("left PDF semantic projection is JSON");
            let mut right_projection: Value = serde_json::from_slice(&right.semantic_projection)
                .expect("right PDF semantic projection is JSON");
            remove_all_link_targets(&mut left_projection);
            remove_all_link_targets(&mut right_projection);
            assert_eq!(
                left_projection, right_projection,
                "all non-link semantics must stay equal when only direct /Dest changes"
            );
        }
        (Err(left), Err(right)) => assert_eq!(
            left.code(),
            right.code(),
            "the same valid direct-destination fixtures must fail closed consistently"
        ),
        (left, right) => panic!(
            "direct /Dest fixtures must either both inspect or fail closed; left={left:?}, right={right:?}"
        ),
    }
}

fn pdf_with_direct_destination(destination_page_object: u32) -> Vec<u8> {
    assert!(matches!(destination_page_object, 3 | 4));
    let page_content = b"BT /F1 12 Tf 20 160 Td (stable) Tj ET";
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(
        2,
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
    );
    objects.insert(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 7 0 R >> >> /Contents 5 0 R /Annots [8 0 R] >>".to_vec(),
    );
    objects.insert(
        4,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_vec(),
    );
    objects.insert(5, stream_object(page_content));
    objects.insert(6, stream_object(page_content));
    objects.insert(
        7,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    );
    objects.insert(
        8,
        format!(
            "<< /Type /Annot /Subtype /Link /P 3 0 R /Rect [20 20 180 60] /Border [0 0 0] /Dest [{destination_page_object} 0 R /Fit] >>"
        )
        .into_bytes(),
    );

    write_pdf(objects)
}

fn stream_object(contents: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} >>\nstream\n", contents.len()).into_bytes();
    object.extend_from_slice(contents);
    object.extend_from_slice(b"\nendstream");
    object
}

fn write_pdf(objects: BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%DSI-direct-destination\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (id, object) in &objects {
        offsets.insert(*id, bytes.len());
        bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let xref = bytes.len();
    let max_id = *objects.keys().max().expect("fixture has PDF objects");
    bytes.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..=max_id {
        let offset = offsets.get(&id).expect("fixture object IDs are contiguous");
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    bytes
}

fn assert_direct_dest_fixture(input: &[u8], destination_page_object: u32) {
    let document = Document::load_mem(input).expect("valid two-page PDF object structure");
    let pages = document.get_pages();
    assert_eq!(pages.len(), 2);

    let first_page = document
        .get_dictionary(pages[&1])
        .expect("first page dictionary");
    let annotations = first_page
        .get_deref(b"Annots", &document)
        .expect("first page annotations")
        .as_array()
        .expect("annotation array");
    assert_eq!(annotations.len(), 1);
    let link = document
        .dereference(&annotations[0])
        .expect("link annotation reference")
        .1
        .as_dict()
        .expect("link annotation dictionary");
    assert!(link.get(b"A").is_err(), "fixture must have no /A action");
    let destination = link
        .get(b"Dest")
        .expect("direct /Dest entry")
        .as_array()
        .expect("direct destination array");
    assert_eq!(
        destination[0]
            .as_reference()
            .expect("destination page reference"),
        (destination_page_object, 0)
    );
}

fn pdfium_page_texts(input: &[u8]) -> Vec<String> {
    let pdfium = pdfium();
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium loads the direct-destination fixture");
    assert_eq!(document.pages().iter().count(), 2);
    document
        .pages()
        .iter()
        .map(|page| {
            page.text()
                .expect("PDFium extracts page text")
                .all()
                .trim()
                .to_owned()
        })
        .collect()
}

fn render_pages(input: &[u8]) -> Vec<Vec<u8>> {
    let pdfium = pdfium();
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium loads the direct-destination fixture for rendering");
    document
        .pages()
        .iter()
        .map(|page| {
            page.render_with_config(&PdfRenderConfig::new().set_target_width(200))
                .expect("PDFium renders a direct-destination page")
                .as_rgba_bytes()
        })
        .collect()
}

fn pdfium_direct_destination_page(input: &[u8]) -> i32 {
    let pdfium = pdfium();
    let document = pdfium
        .load_pdf_from_byte_slice(input, None)
        .expect("PDFium loads the direct-destination fixture for link inspection");
    let page = document.pages().get(0).expect("first PDF page");
    let mut links = page.links().iter();
    let link = links.next().expect("first page has a link");
    assert!(links.next().is_none(), "first page has exactly one link");
    assert!(
        link.action().is_none(),
        "direct /Dest must not be represented as an /A action"
    );
    link.destination()
        .expect("PDFium exposes the direct /Dest entry")
        .page_index()
        .expect("PDFium resolves the direct destination page")
}

fn pdfium() -> &'static Pdfium {
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
    PDFIUM.get_or_init(|| {
        let directory = std::env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("Task 8 tests set the verified PDFIUM_DYNAMIC_LIB_PATH");
        let path = Pdfium::pdfium_platform_library_name_at_path(&directory);
        let bindings = Pdfium::bind_to_library(path).expect("load the qualified PDFium library");
        Pdfium::new(bindings)
    })
}

fn run_child_test(test_name: &str, mode: &str) {
    let executable = std::env::current_exe().expect("integration test executable path");
    let status = Command::new(executable)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MODE, mode)
        .status()
        .expect("run isolated PDFium binding probe");
    assert!(status.success(), "isolated {mode} probe failed: {status}");
}

fn assert_only_destination_reference_changes(left: &[u8], right: &[u8]) {
    assert_eq!(left.len(), right.len());
    let differences: Vec<_> = left
        .iter()
        .zip(right)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect();
    assert_eq!(
        differences.len(),
        1,
        "only the /Dest page reference changes"
    );
    let index = differences[0];
    assert_eq!(left[index], b'3');
    assert_eq!(right[index], b'4');
}

fn remove_all_link_targets(projection: &mut Value) {
    let pages = projection["pages"]
        .as_array_mut()
        .expect("PDF projection contains pages");
    for page in pages {
        page.as_object_mut()
            .expect("PDF page projection is an object")
            .remove("links");
    }
}
