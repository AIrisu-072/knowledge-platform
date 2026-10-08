//! Opaque-state acceptance and explicit fail-closed boundaries, synthetic only.
use document_semantic_inspection_worker::{AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode};
use lopdf::{Document, Object, Stream, dictionary};

fn pdf(state: lopdf::Dictionary, group: Option<lopdf::Dictionary>) -> Vec<u8> {
    let mut document = Document::with_version("1.7");
    let pages = document.new_object_id();
    let font = document.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let contents = document.add_object(Stream::new(lopdf::Dictionary::new(),
        b"/State gs BT /F1 12 Tf 20 180 Td (OPAQUE STATE) Tj ET 20 20 m 80 20 l S".to_vec()));
    let mut page = dictionary! {"Type"=>"Page", "Parent"=>pages,
        "MediaBox"=>vec![Object::Integer(0),Object::Integer(0),Object::Integer(200),Object::Integer(200)],
        "Resources"=>dictionary! {"Font"=>dictionary! {"F1"=>font}, "ExtGState"=>dictionary! {"State"=>state}},
        "Contents"=>contents};
    if let Some(group) = group { page.set("Group", group); }
    let page_id = document.add_object(page);
    document.objects.insert(pages, dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page_id)], "Count"=>1}.into());
    let root = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    document.trailer.set("Root", root);
    let mut bytes = Vec::new(); document.save_to(&mut bytes).unwrap(); bytes
}
fn opaque() -> lopdf::Dictionary {
    dictionary! {"Type"=>"ExtGState", "BM"=>"Normal", "ca"=>1, "CA"=>1}
}
fn accepted(bytes: &[u8]) -> document_semantic_inspection_core::SemanticFingerprint {
    PdfAdapter.inspect(bytes, &AdapterProfile::default()).expect("qualified opaque state").semantic_fingerprint()
}
fn rejected(bytes: &[u8]) {
    let error = PdfAdapter.inspect(bytes, &AdapterProfile::default()).expect_err("unqualified graphics state");
    assert_eq!(error.code(), WorkerFailureCode::UnsupportedSemanticConstruct);
}
#[test]
fn explicit_opaque_extgstate_matches_redundant_opaque_settings() {
    let baseline = accepted(&pdf(opaque(), None));
    let mut state = opaque(); state.remove(b"Type"); state.remove(b"ca"); state.remove(b"CA");
    assert_eq!(baseline, accepted(&pdf(state, None)));
}
#[test]
fn opaque_default_page_group_preserves_identity() {
    let baseline = accepted(&pdf(opaque(), None));
    let group = dictionary! {"Type"=>"Group", "S"=>"Transparency", "CS"=>"DeviceRGB"};
    assert_eq!(baseline, accepted(&pdf(opaque(), Some(group))));
}
#[test]
fn fractional_alpha_blend_and_softmask_are_rejected() {
    accepted(&pdf(opaque(), None));
    for (key, value) in [("ca", Object::Real(0.5)), ("CA", Object::Real(0.5)),
        ("BM", Object::Name(b"Multiply".to_vec())), ("SMask", Object::Name(b"Unknown".to_vec()))] {
        let mut state = opaque(); state.set(key, value); rejected(&pdf(state, None));
    }
}
#[test]
fn unknown_state_and_page_group_keys_are_rejected() {
    accepted(&pdf(opaque(), None));
    let mut state = opaque(); state.set("UnknownFutureState", true); rejected(&pdf(state, None));
    let group = dictionary! {"Type"=>"Group", "S"=>"Transparency", "CS"=>"DeviceRGB", "K"=>true};
    rejected(&pdf(opaque(), Some(group)));
}
