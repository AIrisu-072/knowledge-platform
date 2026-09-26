use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use lopdf::{Document, Object, Stream, dictionary};
use pdfium_render::prelude::Pdfium;
use serde_json::{Value, json};

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

fn direct_destination_pdf(target_page_index: usize) -> Vec<u8> {
    assert!(target_page_index < 2, "two-page fixture destination");

    let mut document = Document::with_version("1.7");
    let pages_id = document.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 2,
    });
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    document.trailer.set("Root", Object::Reference(catalog_id));

    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content_id = document.add_object(Stream::new(
        lopdf::Dictionary::new(),
        b"BT /F1 12 Tf 12 180 Td (SAME NATIVE TEXT) Tj ET\n".to_vec(),
    ));
    let resources = dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font_id) },
    };
    let page_one_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(200),
            Object::Integer(200),
        ],
        "Resources" => resources.clone(),
        "Contents" => Object::Reference(content_id),
        "Annots" => Vec::<Object>::new(),
    });
    let page_two_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(200),
            Object::Integer(200),
        ],
        "Resources" => resources,
        "Contents" => Object::Reference(content_id),
    });
    document
        .get_dictionary_mut(pages_id)
        .expect("page tree dictionary")
        .set(
            "Kids",
            vec![
                Object::Reference(page_one_id),
                Object::Reference(page_two_id),
            ],
        );

    let target_page_id = if target_page_index == 0 {
        page_one_id
    } else {
        page_two_id
    };
    let link_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(100),
            Object::Integer(30),
        ],
        "Border" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(0)],
        "Dest" => vec![
            Object::Reference(target_page_id),
            Object::Name(b"Fit".to_vec()),
        ],
    });
    document
        .get_dictionary_mut(page_one_id)
        .expect("first page dictionary")
        .set("Annots", vec![Object::Reference(link_id)]);

    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .expect("serialize valid two-page PDF");
    bytes
}

fn lopdf_destination_page_index(bytes: &[u8]) -> usize {
    let document = Document::load_mem(bytes).expect("lopdf parses direct-destination PDF");
    let pages = document.get_pages();
    assert_eq!(pages.len(), 2, "lopdf sees both pages");

    let first_page_id = pages.get(&1).copied().expect("first page");
    let annotations = document
        .get_page_annotations(first_page_id)
        .expect("lopdf reads first-page annotations");
    assert_eq!(annotations.len(), 1, "one link annotation");
    let annotation = &annotations[0];
    assert!(annotation.get(b"A").is_err(), "link has no /A action");
    let destination = annotation
        .get(b"Dest")
        .expect("direct /Dest entry")
        .as_array()
        .expect("direct destination array");
    let target_page_id = destination[0]
        .as_reference()
        .expect("destination directly references its target page");

    pages
        .iter()
        .find_map(|(page_index, page_id)| {
            (*page_id == target_page_id).then_some(*page_index as usize - 1)
        })
        .expect("direct destination points to one of the two pages")
}

fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .expect("focused PDF test requires the pinned PDFIUM_DYNAMIC_LIB_PATH");
        let library = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
        let bindings = Pdfium::bind_to_library(library).expect("bind pinned PDFium library");
        Pdfium::new(bindings)
    })
}

fn pdfium_probe(bytes: &[u8]) -> Value {
    let document = pdfium()
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium parses direct-destination PDF");
    let pages = document.pages();
    let visible_text = pages
        .iter()
        .map(|page| {
            page.text()
                .expect("PDFium extracts native page text")
                .all()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let first_page = pages.get(0).expect("first page");
    let links = first_page.links().iter().collect::<Vec<_>>();
    assert_eq!(links.len(), 1, "PDFium sees the first-page link");
    let link = &links[0];
    let action_present = link.action().is_some();
    let destination_page_index = link
        .destination()
        .expect("PDFium resolves direct /Dest even with no action")
        .page_index()
        .expect("PDFium resolves direct destination page index");

    json!({
        "page_count": pages.len(),
        "visible_text": visible_text,
        "first_page_link_count": links.len(),
        "action_present": action_present,
        "destination_page_index": destination_page_index,
    })
}

fn probe_pair_in_isolated_pdfium_process(left: &[u8], right: &[u8]) -> (Value, Value) {
    let directory = tempfile::tempdir().expect("temporary direct-destination probe directory");
    let left_path = directory.path().join("left.pdf");
    let right_path = directory.path().join("right.pdf");
    let output_path = directory.path().join("pdfium-probe.json");
    fs::write(&left_path, left).expect("write left direct-destination PDF");
    fs::write(&right_path, right).expect("write right direct-destination PDF");

    // Keep PDFium's process-global binding separate from PdfAdapter's binding.
    let child = Command::new(env::current_exe().expect("integration-test executable"))
        .args([
            "--exact",
            "pdfium_direct_destination_probe_entrypoint",
            "--ignored",
        ])
        .env("DSI_PDF_DIRECT_DEST_LEFT", &left_path)
        .env("DSI_PDF_DIRECT_DEST_RIGHT", &right_path)
        .env("DSI_PDF_DIRECT_DEST_OUTPUT", &output_path)
        .output()
        .expect("start isolated PDFium direct-destination probe");
    assert!(
        child.status.success(),
        "PDFium direct-destination probe failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );

    let probes: Value = serde_json::from_slice(
        &fs::read(output_path).expect("read PDFium direct-destination observations"),
    )
    .expect("PDFium observation JSON");
    (
        probes.get("left").expect("left PDFium observation").clone(),
        probes
            .get("right")
            .expect("right PDFium observation")
            .clone(),
    )
}

#[test]
#[ignore = "invoked in a child process to isolate pdfium-render's global binding"]
fn pdfium_direct_destination_probe_entrypoint() {
    let left = fs::read(env::var_os("DSI_PDF_DIRECT_DEST_LEFT").expect("left PDF path"))
        .expect("read left PDF");
    let right = fs::read(env::var_os("DSI_PDF_DIRECT_DEST_RIGHT").expect("right PDF path"))
        .expect("read right PDF");
    let observations = json!({
        "left": pdfium_probe(&left),
        "right": pdfium_probe(&right),
    });
    fs::write(
        env::var_os("DSI_PDF_DIRECT_DEST_OUTPUT").expect("observation output path"),
        serde_json::to_vec(&observations).expect("serialize PDFium observations"),
    )
    .expect("write PDFium observations");
}

#[test]
fn direct_destination_page_change_changes_identity_or_fails_closed() {
    let left = direct_destination_pdf(0);
    let right = direct_destination_pdf(1);

    assert_eq!(lopdf_destination_page_index(&left), 0);
    assert_eq!(lopdf_destination_page_index(&right), 1);

    let (left_probe, right_probe) = probe_pair_in_isolated_pdfium_process(&left, &right);
    for probe in [&left_probe, &right_probe] {
        assert_eq!(probe["page_count"], 2);
        assert_eq!(probe["first_page_link_count"], 1);
        assert_eq!(probe["action_present"], false);
        assert_eq!(
            probe["visible_text"],
            json!(["SAME NATIVE TEXT", "SAME NATIVE TEXT"]),
            "the two PDFs have identical reader-visible text"
        );
    }
    assert_eq!(left_probe["destination_page_index"], 0);
    assert_eq!(right_probe["destination_page_index"], 1);
    assert_eq!(
        left_probe["visible_text"], right_probe["visible_text"],
        "only the direct link destination changes"
    );

    let left_result = PdfAdapter.inspect(&left, &AdapterProfile::default());
    let right_result = PdfAdapter.inspect(&right, &AdapterProfile::default());
    match (left_result, right_result) {
        (Ok(left), Ok(right)) => assert_ne!(
            left.semantic_fingerprint(),
            right.semantic_fingerprint(),
            "direct /Dest target page changes the PDF link semantics"
        ),
        (Err(left), Err(right)) => {
            assert_eq!(left.code(), right.code());
            assert_eq!(
                left.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "unmodeled direct destinations must fail closed"
            );
        }
        _ => panic!("both direct-destination PDFs must have the same parser outcome"),
    }
}
