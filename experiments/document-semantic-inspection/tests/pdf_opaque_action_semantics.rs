use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PdfAdapter, fingerprint,
};
use lopdf::{Dictionary, Document, Object, Stream};

const BASE: &[u8] = include_bytes!("../fixtures/pdf/base.pdf");

fn remote_link(destination: &str) -> Vec<u8> {
    let mut document = Document::load_mem(BASE).expect("qualified native-text PDF");
    let link = document
        .get_object_mut((9, 0))
        .expect("link annotation")
        .as_dict_mut()
        .expect("link dictionary");
    let mut action = Dictionary::new();
    action.set("S", Object::Name(b"GoToR".to_vec()));
    action.set("F", Object::string_literal("linked.pdf"));
    action.set("D", Object::string_literal(destination));
    link.set("A", Object::Dictionary(action));
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("valid PDF xref");
    bytes
}

fn with_opaque_content_stream() -> Vec<u8> {
    let mut document = Document::load_mem(BASE).expect("qualified native-text PDF");
    let page_id = *document.get_pages().values().next().expect("first page");
    let original_contents = document
        .get_dictionary(page_id)
        .expect("page dictionary")
        .get(b"Contents")
        .expect("page Contents")
        .clone();
    let mut dictionary = Dictionary::new();
    dictionary.set("Filter", Object::Name(b"UnrecognizedDecode".to_vec()));
    let opaque_id = document.add_object(Stream::new(dictionary, b"opaque content".to_vec()));
    document
        .get_object_mut(page_id)
        .expect("page object")
        .as_dict_mut()
        .expect("page dictionary")
        .set(
            "Contents",
            Object::Array(vec![original_contents, Object::Reference(opaque_id)]),
        );
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("valid PDF xref");
    bytes
}

#[test]
fn remote_link_destination_changes_identity_or_fails_closed() {
    let left = remote_link("chapter-one");
    let right = remote_link("chapter-two");
    let left_result = PdfAdapter.inspect(&left, &InspectionProfile::default());
    let right_result = PdfAdapter.inspect(&right, &InspectionProfile::default());

    match (left_result, right_result) {
        (Ok(left), Ok(right)) => assert_ne!(
            fingerprint(&left.semantic_projection),
            fingerprint(&right.semantic_projection),
            "remote GoToR destination changes the PDF link target"
        ),
        (Err(left), Err(right)) => {
            assert_eq!(left.code(), right.code());
            assert_eq!(left.code(), ErrorCode::UnsupportedSemanticConstruct);
        }
        _ => panic!("both remote links must have the same parser outcome"),
    }
}

#[test]
fn unreadable_page_content_stream_fails_closed() {
    let bytes = with_opaque_content_stream();
    let document = Document::load_mem(&bytes).expect("valid PDF object structure");
    let page_id = *document.get_pages().values().next().expect("first page");
    let content_ids = document.get_page_contents(page_id);
    let opaque = document
        .get_object(*content_ids.last().expect("opaque content stream"))
        .expect("opaque content object")
        .as_stream()
        .expect("opaque content stream");
    assert!(
        opaque
            .decompressed_content_with_limit(64 * 1024 * 1024)
            .is_err()
    );

    let failure = match PdfAdapter.inspect(&bytes, &InspectionProfile::default()) {
        Ok(_) => panic!("an opaque page content stream was accepted"),
        Err(failure) => failure,
    };
    assert_eq!(failure.code(), ErrorCode::ParserDisagreement);
}
