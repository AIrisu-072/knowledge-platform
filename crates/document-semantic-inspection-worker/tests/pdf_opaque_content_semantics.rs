use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use lopdf::{Dictionary, Document, Object, Stream};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");

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
    document.save_to(&mut bytes).expect("write valid xref");
    bytes
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
            .is_err(),
        "lopdf cannot prove what the opaque stream contains"
    );

    let failure = match PdfAdapter.inspect(&bytes, &AdapterProfile::default()) {
        Ok(_) => panic!("an opaque page content stream was accepted"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.code(),
        WorkerFailureCode::ParserDisagreement,
        "{failure}"
    );
}
