use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use lopdf::{Dictionary, Document, Object};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");

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

#[test]
fn remote_link_destination_changes_identity_or_fails_closed() {
    let left = remote_link("chapter-one");
    let right = remote_link("chapter-two");
    let left_result = PdfAdapter.inspect(&left, &AdapterProfile::default());
    let right_result = PdfAdapter.inspect(&right, &AdapterProfile::default());

    match (left_result, right_result) {
        (Ok(left), Ok(right)) => assert_ne!(
            left.semantic_fingerprint(),
            right.semantic_fingerprint(),
            "remote GoToR destination changes the PDF link target"
        ),
        (Err(left), Err(right)) => {
            assert_eq!(left.code(), right.code());
            assert_eq!(left.code(), WorkerFailureCode::UnsupportedSemanticConstruct);
        }
        _ => panic!("both remote links must have the same parser outcome"),
    }
}
