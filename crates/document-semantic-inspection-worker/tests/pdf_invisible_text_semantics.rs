use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use lopdf::{Document, Object, Stream};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");

fn with_invisible_text(hidden: &str) -> Vec<u8> {
    let mut document = Document::load_mem(BASE).expect("qualified native-text PDF");
    let page_id = *document.get_pages().values().next().expect("first page");
    let content = format!("BT /F1 18 Tf 72 720 Td (Stable) Tj 3 Tr 0 -22 Td ({hidden}) Tj ET");
    let content_id =
        document.add_object(Stream::new(lopdf::Dictionary::new(), content.into_bytes()));
    document
        .get_object_mut(page_id)
        .expect("first page object")
        .as_dict_mut()
        .expect("first page dictionary")
        .set("Contents", Object::Reference(content_id));
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("valid PDF xref");
    bytes
}

#[test]
fn invisible_text_change_is_noise_or_fails_closed() {
    let left = with_invisible_text("HiddenAlpha");
    let right = with_invisible_text("HiddenBeta");
    let left_result = PdfAdapter.inspect(&left, &AdapterProfile::default());
    let right_result = PdfAdapter.inspect(&right, &AdapterProfile::default());

    match (left_result, right_result) {
        (Ok(left), Ok(right)) => {
            assert_eq!(
                left.semantic_fingerprint(),
                right.semantic_fingerprint(),
                "PDF text rendering mode 3 is invisible and cannot change reader-visible identity"
            )
        }
        (Err(left), Err(right)) => {
            assert_eq!(left.code(), right.code());
            assert_eq!(left.code(), WorkerFailureCode::UnsupportedSemanticConstruct);
        }
        _ => panic!("both invisible-text variants must have the same parser outcome"),
    }
}
