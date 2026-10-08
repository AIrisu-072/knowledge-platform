use document_semantic_inspection_core::SemanticFingerprint;
use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, WorkerFailureCode,
};
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

// Entirely synthetic native text. No official source bytes are embedded here.
const FIRST: &str = "FIRST PARAGRAPH";
const SECOND: &str = "SECOND PARAGRAPH";

struct TaggedPdf {
    document: Document,
    catalog: ObjectId,
    page: ObjectId,
    content: ObjectId,
    structure_root: ObjectId,
    document_element: ObjectId,
    paragraphs: [ObjectId; 2],
    parent_tree: ObjectId,
    properties: [ObjectId; 2],
}

impl TaggedPdf {
    fn new(mcids: [usize; 2], named_properties: bool, renamed: bool) -> Self {
        assert_ne!(mcids[0], mcids[1]);
        assert!(mcids.iter().all(|mcid| *mcid < 32));
        let mut document = Document::with_version("1.7");
        if renamed {
            // Reserve unused object numbers, changing every indirect identity.
            for _ in 0..17 {
                document.new_object_id();
            }
        }
        let catalog = document.new_object_id();
        let pages = document.new_object_id();
        let page = document.new_object_id();
        let content = document.new_object_id();
        let font = document.new_object_id();
        let structure_root = document.new_object_id();
        let document_element = document.new_object_id();
        let paragraphs = [document.new_object_id(), document.new_object_id()];
        let parent_tree = document.new_object_id();
        let properties = [document.new_object_id(), document.new_object_id()];
        let font_name = if renamed { "FontRenamed" } else { "F1" };
        let property_names = if renamed { ["TagX", "TagY"] } else { ["P0", "P1"] };

        document.objects.insert(catalog, dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
            "MarkInfo" => dictionary! { "Marked" => true },
            "StructTreeRoot" => Object::Reference(structure_root),
        }.into());
        document.objects.insert(pages, dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page)],
            "Count" => 1,
        }.into());
        let mut fonts = Dictionary::new();
        fonts.set(font_name, Object::Reference(font));
        let mut resources = dictionary! { "Font" => fonts };
        if named_properties {
            let mut property_resources = Dictionary::new();
            for (name, id) in property_names.iter().zip(properties) {
                property_resources.set(*name, Object::Reference(id));
            }
            resources.set("Properties", property_resources);
        }
        document.objects.insert(page, dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages),
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(200), Object::Integer(200),
            ],
            "Resources" => resources,
            "Contents" => Object::Reference(content),
            "StructParents" => 0,
        }.into());
        document.objects.insert(font, dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        }.into());
        let mut operators = String::new();
        for index in 0..2 {
            let property = if named_properties {
                format!("/{}", property_names[index])
            } else {
                format!("<< /MCID {} >>", mcids[index])
            };
            let text = [FIRST, SECOND][index];
            let y = [180, 140][index];
            operators.push_str(&format!(
                "/P {property} BDC\nBT /{font_name} 12 Tf 12 {y} Td ({text}) Tj ET\nEMC\n"
            ));
            document.objects.insert(properties[index], dictionary! {
                "MCID" => mcids[index] as i64,
            }.into());
            document.objects.insert(paragraphs[index], dictionary! {
                "Type" => "StructElem",
                "S" => "P",
                "P" => Object::Reference(document_element),
                "Pg" => Object::Reference(page),
                "K" => mcids[index] as i64,
            }.into());
        }
        document.objects.insert(content, Stream::new(Dictionary::new(), operators.into_bytes()).into());
        document.objects.insert(structure_root, dictionary! {
            "Type" => "StructTreeRoot",
            "K" => vec![Object::Reference(document_element)],
            "ParentTree" => Object::Reference(parent_tree),
            "ParentTreeNextKey" => 1,
        }.into());
        document.objects.insert(document_element, dictionary! {
            "Type" => "StructElem",
            "S" => "Document",
            "P" => Object::Reference(structure_root),
            "K" => paragraphs.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        }.into());
        let mut parent_entries = vec![Object::Null; *mcids.iter().max().unwrap() + 1];
        for index in 0..2 {
            parent_entries[mcids[index]] = Object::Reference(paragraphs[index]);
        }
        document.objects.insert(parent_tree, dictionary! {
            "Nums" => vec![Object::Integer(0), Object::Array(parent_entries)],
        }.into());
        document.trailer.set("Root", Object::Reference(catalog));
        Self {
            document,
            catalog,
            page,
            content,
            structure_root,
            document_element,
            paragraphs,
            parent_tree,
            properties,
        }
    }

    fn baseline() -> Self {
        Self::new([0, 1], false, false)
    }

    fn dictionary(&mut self, id: ObjectId) -> &mut Dictionary {
        self.document.get_dictionary_mut(id).expect("fixture dictionary")
    }

    fn content_text(&self) -> String {
        let stream = self.document.get_object(self.content).unwrap().as_stream().unwrap();
        String::from_utf8(stream.content.clone()).expect("synthetic ASCII content")
    }

    fn replace_content(&mut self, content: String) {
        self.document.objects.insert(
            self.content,
            Stream::new(Dictionary::new(), content.into_bytes()).into(),
        );
    }

    fn bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.document.save_to(&mut bytes).expect("serialize synthetic tagged PDF");
        bytes
    }
}

fn fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("bounded valid tagged native-text PDF must be accepted")
        .semantic_fingerprint()
}

fn painted_text(bytes: &[u8]) -> Vec<String> {
    let document = Document::load_mem(bytes).expect("fixture must parse independently");
    let page = *document.get_pages().values().next().unwrap();
    let content = document.get_page_content(page).expect("fixture page content");
    Content::decode_strict(&content)
        .expect("fixture operations")
        .operations
        .iter()
        .filter(|operation| operation.operator == "Tj")
        .map(|operation| match operation.operands.as_slice() {
            [Object::String(text, _)] => String::from_utf8(text.clone()).unwrap(),
            _ => panic!("synthetic Tj must have one string operand"),
        })
        .collect()
}

fn assert_meaning_changes_or_is_explicitly_unsupported(baseline: &[u8], changed: &[u8]) {
    // A blanket rejection of every tagged PDF cannot satisfy this contract.
    let accepted_baseline = fingerprint(baseline);
    assert_eq!(painted_text(baseline), vec![FIRST, SECOND]);
    assert_eq!(painted_text(baseline), painted_text(changed));
    match PdfAdapter.inspect(changed, &AdapterProfile::default()) {
        Ok(output) => assert_ne!(
            accepted_baseline,
            output.semantic_fingerprint(),
            "changed replacement text/read order cannot be reported unchanged"
        ),
        Err(failure) => assert_eq!(
            failure.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "only explicit unsupported semantics can substitute for detecting this valid change: {failure}"
        ),
    }
}

fn assert_rejected(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("malformed or unsupported tagged semantics must fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

#[test]
fn balanced_mcid_paragraphs_with_resolved_structure_are_accepted() {
    let bytes = TaggedPdf::baseline().bytes();
    assert_eq!(painted_text(&bytes), vec![FIRST, SECOND]);
    let (_, projection) = PdfAdapter
        .inspect_with_projection(&bytes, &AdapterProfile::default())
        .expect("balanced BDC/EMC and an unambiguous ParentTree must be supported");
    let text = projection["pages"][0]["text"].as_str().expect("native text projection");
    assert!(text.contains(FIRST) && text.contains(SECOND));
    assert!(text.find(FIRST) < text.find(SECOND), "baseline read order is preserved");
}

#[test]
fn renumbering_mcids_and_parent_tree_slots_preserves_identity() {
    let baseline = TaggedPdf::baseline().bytes();
    let changed = TaggedPdf::new([9, 3], false, false).bytes();
    assert_eq!(painted_text(&baseline), painted_text(&changed));
    assert_eq!(fingerprint(&baseline), fingerprint(&changed));
}

#[test]
fn named_properties_and_resource_and_object_renumbering_preserve_identity() {
    let inline = TaggedPdf::baseline().bytes();
    let named = TaggedPdf::new([0, 1], true, false).bytes();
    let renamed = TaggedPdf::new([0, 1], true, true).bytes();
    assert_ne!(inline, named);
    assert_ne!(named, renamed);
    assert_eq!(fingerprint(&inline), fingerprint(&named));
    assert_eq!(fingerprint(&named), fingerprint(&renamed));
}

#[test]
fn structure_actual_text_is_not_ignored_when_native_glyphs_are_unchanged() {
    let baseline = TaggedPdf::baseline().bytes();
    let mut changed = TaggedPdf::baseline();
    changed.dictionary(changed.paragraphs[0]).set(
        "ActualText",
        Object::string_literal("FIRST REPLACEMENT"),
    );
    assert_meaning_changes_or_is_explicitly_unsupported(&baseline, &changed.bytes());
}

#[test]
fn marked_content_actual_text_is_not_ignored_when_native_glyphs_are_unchanged() {
    let baseline = TaggedPdf::baseline().bytes();
    let mut changed = TaggedPdf::baseline();
    changed.replace_content(changed.content_text().replacen(
        "<< /MCID 0 >>",
        "<< /MCID 0 /ActualText (FIRST REPLACEMENT) >>",
        1,
    ));
    assert_meaning_changes_or_is_explicitly_unsupported(&baseline, &changed.bytes());
}

#[test]
fn named_property_actual_text_is_not_ignored() {
    let baseline = TaggedPdf::new([0, 1], true, false).bytes();
    let mut changed = TaggedPdf::new([0, 1], true, false);
    changed.dictionary(changed.properties[0]).set(
        "ActualText",
        Object::string_literal("FIRST REPLACEMENT"),
    );
    assert_meaning_changes_or_is_explicitly_unsupported(&baseline, &changed.bytes());
}

#[test]
fn reversing_structure_read_order_cannot_report_unchanged() {
    let baseline = TaggedPdf::baseline().bytes();
    let mut changed = TaggedPdf::baseline();
    let reversed = vec![
        Object::Reference(changed.paragraphs[1]),
        Object::Reference(changed.paragraphs[0]),
    ];
    changed.dictionary(changed.document_element).set("K", reversed);
    assert_meaning_changes_or_is_explicitly_unsupported(&baseline, &changed.bytes());
}

#[test]
fn unmatched_marked_content_delimiters_fail_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut missing_end = TaggedPdf::baseline();
    missing_end.replace_content(missing_end.content_text().replacen("EMC\n", "", 1));
    assert_rejected(&missing_end.bytes(), WorkerFailureCode::ParserDisagreement);
    let mut extra_end = TaggedPdf::baseline();
    extra_end.replace_content(format!("EMC\n{}", extra_end.content_text()));
    assert_rejected(&extra_end.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn mcid_without_structure_root_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    broken.dictionary(broken.catalog).remove(b"StructTreeRoot");
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn mcid_without_a_matching_structure_leaf_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    broken.dictionary(broken.paragraphs[0]).set("K", 7);
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn unresolved_structure_reference_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    broken.dictionary(broken.structure_root).set("K", Object::Reference((999, 0)));
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn cyclic_structure_children_fail_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    let self_reference = Object::Reference(broken.document_element);
    broken.dictionary(broken.document_element).set("K", vec![self_reference]);
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn inconsistent_parent_tree_mapping_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    let wrong_parents = vec![
        Object::Reference(broken.paragraphs[1]),
        Object::Reference(broken.paragraphs[0]),
    ];
    broken.dictionary(broken.parent_tree).set(
        "Nums",
        vec![Object::Integer(0), Object::Array(wrong_parents)],
    );
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn duplicate_mcid_ownership_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    broken.dictionary(broken.paragraphs[1]).set("K", 0);
    broken.replace_content(broken.content_text().replacen("/MCID 1", "/MCID 0", 1));
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn optional_content_marked_sequence_fails_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut unsupported = TaggedPdf::baseline();
    let ocg = unsupported.document.add_object(dictionary! {
        "Type" => "OCG",
        "Name" => Object::string_literal("Synthetic optional layer"),
    });
    unsupported.dictionary(unsupported.catalog).set("OCProperties", dictionary! {
        "OCGs" => vec![Object::Reference(ocg)],
        "D" => dictionary! {
            "BaseState" => "ON",
            "OFF" => vec![Object::Reference(ocg)],
        },
    });
    unsupported.dictionary(unsupported.page)
        .get_mut(b"Resources").unwrap().as_dict_mut().unwrap()
        .set("Properties", dictionary! { "Layer" => Object::Reference(ocg) });
    unsupported.replace_content(format!("/OC /Layer BDC\n{}EMC\n", unsupported.content_text()));
    assert_rejected(&unsupported.bytes(), WorkerFailureCode::UnsupportedSemanticConstruct);
}
