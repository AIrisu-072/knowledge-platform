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
        let property_names = if renamed {
            ["TagX", "TagY"]
        } else {
            ["P0", "P1"]
        };

        document.objects.insert(
            catalog,
            dictionary! {
                "Type" => "Catalog",
                "Pages" => Object::Reference(pages),
                "MarkInfo" => dictionary! { "Marked" => true },
                "StructTreeRoot" => Object::Reference(structure_root),
            }
            .into(),
        );
        document.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
            }
            .into(),
        );
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
        document.objects.insert(
            page,
            dictionary! {
                "Type" => "Page",
                "Parent" => Object::Reference(pages),
                "MediaBox" => vec![
                    Object::Integer(0), Object::Integer(0),
                    Object::Integer(200), Object::Integer(200),
                ],
                "Resources" => resources,
                "Contents" => Object::Reference(content),
                "StructParents" => 0,
            }
            .into(),
        );
        document.objects.insert(
            font,
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
            }
            .into(),
        );
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
            document.objects.insert(
                properties[index],
                dictionary! {
                    "MCID" => mcids[index] as i64,
                }
                .into(),
            );
            document.objects.insert(
                paragraphs[index],
                dictionary! {
                    "Type" => "StructElem",
                    "S" => "P",
                    "P" => Object::Reference(document_element),
                    "Pg" => Object::Reference(page),
                    "K" => mcids[index] as i64,
                }
                .into(),
            );
        }
        document.objects.insert(
            content,
            Stream::new(Dictionary::new(), operators.into_bytes()).into(),
        );
        document.objects.insert(
            structure_root,
            dictionary! {
                "Type" => "StructTreeRoot",
                "K" => vec![Object::Reference(document_element)],
                "ParentTree" => Object::Reference(parent_tree),
                "ParentTreeNextKey" => 1,
            }
            .into(),
        );
        document.objects.insert(
            document_element,
            dictionary! {
                "Type" => "StructElem",
                "S" => "Document",
                "P" => Object::Reference(structure_root),
                "K" => paragraphs.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            }
            .into(),
        );
        let mut parent_entries = vec![Object::Null; *mcids.iter().max().unwrap() + 1];
        for index in 0..2 {
            parent_entries[mcids[index]] = Object::Reference(paragraphs[index]);
        }
        document.objects.insert(
            parent_tree,
            dictionary! {
                "Nums" => vec![Object::Integer(0), Object::Array(parent_entries)],
            }
            .into(),
        );
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

    fn two_page(shared_paragraph: bool, renumbered: bool) -> Self {
        let mcids = if renumbered { [9, 3] } else { [0, 1] };
        let mut fixture = Self::new(mcids, false, renumbered);
        let page_tree = fixture
            .dictionary(fixture.page)
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let original_content = fixture.content_text();
        let (first, second) = original_content.split_once("EMC\n").unwrap();
        fixture.replace_content(format!("{first}EMC\n"));
        let second_content = fixture.document.add_object(Stream::new(
            Dictionary::new(),
            second.as_bytes().to_vec(),
        ));
        let mut second_page = fixture.dictionary(fixture.page).clone();
        second_page.set("Contents", Object::Reference(second_content));
        second_page.set("StructParents", 1);
        let second_page = fixture.document.add_object(second_page);
        let first_page = fixture.page;
        fixture.dictionary(page_tree).set(
            "Kids",
            vec![Object::Reference(first_page), Object::Reference(second_page)],
        );
        fixture.dictionary(page_tree).set("Count", 2);
        fixture
            .dictionary(fixture.structure_root)
            .set("ParentTreeNextKey", 2);

        let first_mcr = Object::Dictionary(dictionary! {
            "Type" => "MCR", "Pg" => Object::Reference(first_page), "MCID" => mcids[0] as i64,
        });
        let second_mcr = Object::Dictionary(dictionary! {
            "Type" => "MCR", "Pg" => Object::Reference(second_page), "MCID" => mcids[1] as i64,
        });
        let [first_paragraph, second_paragraph] = fixture.paragraphs;
        fixture.dictionary(first_paragraph).remove(b"Pg");
        fixture.dictionary(second_paragraph).remove(b"Pg");
        if shared_paragraph {
            fixture
                .dictionary(first_paragraph)
                .set("K", vec![first_mcr, second_mcr]);
            fixture.dictionary(fixture.document_element).set(
                "K",
                vec![Object::Reference(first_paragraph)],
            );
        } else {
            fixture.dictionary(first_paragraph).set("K", first_mcr);
            fixture.dictionary(second_paragraph).set("K", second_mcr);
        }
        let second_owner = if shared_paragraph {
            first_paragraph
        } else {
            second_paragraph
        };
        let mut first_parents = vec![Object::Null; mcids[0] + 1];
        first_parents[mcids[0]] = Object::Reference(first_paragraph);
        let mut second_parents = vec![Object::Null; mcids[1] + 1];
        second_parents[mcids[1]] = Object::Reference(second_owner);
        fixture.dictionary(fixture.parent_tree).set(
            "Nums",
            vec![
                Object::Integer(0),
                Object::Array(first_parents),
                Object::Integer(1),
                Object::Array(second_parents),
            ],
        );
        fixture
    }

    fn dictionary(&mut self, id: ObjectId) -> &mut Dictionary {
        self.document
            .get_dictionary_mut(id)
            .expect("fixture dictionary")
    }

    fn content_text(&self) -> String {
        let stream = self
            .document
            .get_object(self.content)
            .unwrap()
            .as_stream()
            .unwrap();
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
        self.document
            .save_to(&mut bytes)
            .expect("serialize synthetic tagged PDF");
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
    let content = document.get_page_content(page);
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

fn painted_page_text(bytes: &[u8]) -> Vec<Vec<String>> {
    let document = Document::load_mem(bytes).expect("fixture must parse independently");
    document
        .get_pages()
        .into_values()
        .map(|page| {
            Content::decode_strict(&document.get_page_content(page))
                .expect("fixture page operations")
                .operations
                .iter()
                .filter(|operation| operation.operator == "Tj")
                .map(|operation| match operation.operands.as_slice() {
                    [Object::String(text, _)] => String::from_utf8(text.clone()).unwrap(),
                    _ => panic!("synthetic Tj must have one string operand"),
                })
                .collect()
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
    let text = projection["pages"][0]["text"]
        .as_str()
        .expect("native text projection");
    assert!(text.contains(FIRST) && text.contains(SECOND));
    assert!(
        text.find(FIRST) < text.find(SECOND),
        "baseline read order is preserved"
    );
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
    changed
        .dictionary(changed.paragraphs[0])
        .set("ActualText", Object::string_literal("FIRST REPLACEMENT"));
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
    changed
        .dictionary(changed.properties[0])
        .set("ActualText", Object::string_literal("FIRST REPLACEMENT"));
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
    changed
        .dictionary(changed.document_element)
        .set("K", reversed);
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
    broken
        .dictionary(broken.structure_root)
        .set("K", Object::Reference((999, 0)));
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn cyclic_structure_children_fail_closed() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    let self_reference = Object::Reference(broken.document_element);
    broken
        .dictionary(broken.document_element)
        .set("K", vec![self_reference]);
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
    unsupported.dictionary(unsupported.catalog).set(
        "OCProperties",
        dictionary! {
            "OCGs" => vec![Object::Reference(ocg)],
            "D" => dictionary! {
                "BaseState" => "ON",
                "OFF" => vec![Object::Reference(ocg)],
            },
        },
    );
    unsupported
        .dictionary(unsupported.page)
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Properties",
            dictionary! { "Layer" => Object::Reference(ocg) },
        );
    unsupported.replace_content(format!(
        "/OC /Layer BDC\n{}EMC\n",
        unsupported.content_text()
    ));
    assert_rejected(
        &unsupported.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn tag_only_page_rejects_unqualified_colour_and_compositing_context() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut colors = TaggedPdf::baseline();
    colors
        .dictionary(colors.page)
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("ColorSpace", dictionary! {"DefaultGray" => "DeviceGray"});
    assert_rejected(
        &colors.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
    let mut grouped = TaggedPdf::baseline();
    grouped.dictionary(grouped.page).set(
        "Group",
        dictionary! {
            "Type" => "Group", "S" => "Transparency", "CS" => "DeviceRGB", "K" => true,
        },
    );
    assert_rejected(
        &grouped.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
    let mut output = TaggedPdf::baseline();
    output
        .dictionary(output.catalog)
        .set("OutputIntents", Object::Array(vec![]));
    assert_rejected(
        &output.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn unowned_empty_parent_tree_key_is_rejected() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut broken = TaggedPdf::baseline();
    let nums = broken
        .dictionary(broken.parent_tree)
        .get_mut(b"Nums")
        .unwrap()
        .as_array_mut()
        .unwrap();
    nums.extend([Object::Integer(99), Object::Array(vec![])]);
    assert_rejected(&broken.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn redundant_unicode_actual_text_preserves_tagged_identity() {
    let baseline = TaggedPdf::baseline().bytes();
    let mut tagged = TaggedPdf::baseline();
    let mut encoded = vec![0xfe, 0xff];
    for unit in FIRST.encode_utf16() {
        encoded.extend(unit.to_be_bytes());
    }
    tagged
        .dictionary(tagged.paragraphs[0])
        .set("ActualText", Object::string_literal(encoded));
    assert_eq!(fingerprint(&baseline), fingerprint(&tagged.bytes()));
}

#[test]
fn tables_and_ambiguous_nested_mcid_ownership_are_rejected() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut table = TaggedPdf::baseline();
    table.dictionary(table.document_element).set("S", "Table");
    assert_rejected(
        &table.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
    let mut nested = TaggedPdf::baseline();
    nested.replace_content(format!(
        "/P << /MCID 0 >> BDC\n{}EMC\n",
        nested.content_text()
    ));
    assert_rejected(
        &nested.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}


#[test]
fn one_paragraph_across_pages_differs_from_two_page_local_paragraphs() {
    let shared = TaggedPdf::two_page(true, false).bytes();
    let split = TaggedPdf::two_page(false, false).bytes();
    assert_eq!(painted_page_text(&shared), vec![vec![FIRST], vec![SECOND]]);
    assert_eq!(painted_page_text(&shared), painted_page_text(&split));
    let page_contents = |bytes: &[u8]| {
        let document = Document::load_mem(bytes).unwrap();
        document
            .get_pages()
            .into_values()
            .map(|page| document.get_page_content(page))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        page_contents(&shared),
        page_contents(&split),
        "the exact text and paint operator streams remain unchanged"
    );
    let (shared_output, shared_projection) = PdfAdapter
        .inspect_with_projection(&shared, &AdapterProfile::default())
        .expect("one paragraph with two page-scoped MCR leaves must be accepted");
    let (split_output, split_projection) = PdfAdapter
        .inspect_with_projection(&split, &AdapterProfile::default())
        .expect("two paragraphs with separate page-scoped MCR leaves must be accepted");
    assert_eq!(shared_projection["pages"].as_array().unwrap().len(), 2);
    assert_eq!(split_projection["pages"].as_array().unwrap().len(), 2);
    for page in 0..2 {
        assert_eq!(
            shared_projection["pages"][page]["text"],
            split_projection["pages"][page]["text"],
            "only cross-page paragraph continuity changes"
        );
    }
    assert_ne!(
        shared_output.semantic_fingerprint(),
        split_output.semantic_fingerprint(),
        "per-page projection must retain whether a paragraph continues across pages"
    );
}

#[test]
fn cross_page_paragraph_identity_survives_object_resource_and_mcid_renumbering() {
    let baseline = TaggedPdf::two_page(true, false).bytes();
    let renumbered = TaggedPdf::two_page(true, true).bytes();
    assert_ne!(baseline, renumbered);
    assert_eq!(painted_page_text(&baseline), painted_page_text(&renumbered));
    assert_eq!(fingerprint(&baseline), fingerprint(&renumbered));
}

#[test]
fn catalog_language_default_changes_identity_without_local_language_overrides() {
    let mut english = TaggedPdf::baseline();
    english
        .dictionary(english.catalog)
        .set("Lang", Object::string_literal("en-US"));
    let mut french = TaggedPdf::baseline();
    french
        .dictionary(french.catalog)
        .set("Lang", Object::string_literal("fr-FR"));
    let english = english.bytes();
    let french = french.bytes();
    assert_eq!(painted_text(&english), vec![FIRST, SECOND]);
    assert_eq!(painted_text(&english), painted_text(&french));
    let (english_output, english_projection) = PdfAdapter
        .inspect_with_projection(&english, &AdapterProfile::default())
        .expect("tagged PDF with an English catalog language must be accepted");
    let (french_output, french_projection) = PdfAdapter
        .inspect_with_projection(&french, &AdapterProfile::default())
        .expect("tagged PDF with a French catalog language must be accepted");
    assert_eq!(
        english_projection["pages"][0]["text"],
        french_projection["pages"][0]["text"],
        "only the inherited catalog language changes"
    );
    assert_ne!(
        english_output.semantic_fingerprint(),
        french_output.semantic_fingerprint(),
        "catalog language is the effective language when no local override exists"
    );
}

#[test]
fn association_marker_spelling_does_not_override_resolved_structure_role() {
    let baseline = TaggedPdf::baseline().bytes();
    let mut generic_spans = TaggedPdf::baseline();
    generic_spans.replace_content(
        generic_spans
            .content_text()
            .replace("/P << /MCID", "/Span << /MCID"),
    );
    let generic_spans = generic_spans.bytes();
    assert_ne!(baseline, generic_spans);
    assert_eq!(painted_text(&baseline), painted_text(&generic_spans));
    assert_eq!(
        fingerprint(&baseline),
        fingerprint(&generic_spans),
        "MCID association markers do not replace their unchanged StructElem P role"
    );
}

#[test]
fn excessive_structure_depth_fails_closed_through_the_real_adapter() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut deep = TaggedPdf::baseline();
    let sections: Vec<_> = (0..64)
        .map(|_| deep.document.new_object_id())
        .collect();
    for (index, section) in sections.iter().copied().enumerate() {
        let parent = if index == 0 {
            deep.document_element
        } else {
            sections[index - 1]
        };
        let children = if let Some(next) = sections.get(index + 1) {
            Object::Reference(*next)
        } else {
            Object::Array(deep.paragraphs.iter().copied().map(Object::Reference).collect())
        };
        deep.document.objects.insert(
            section,
            dictionary! {
                "Type" => "StructElem", "S" => "Sect",
                "P" => Object::Reference(parent), "K" => children,
            }
            .into(),
        );
    }
    let last_section = *sections.last().unwrap();
    for paragraph in deep.paragraphs {
        deep.dictionary(paragraph)
            .set("P", Object::Reference(last_section));
    }
    deep.dictionary(deep.document_element)
        .set("K", Object::Reference(sections[0]));
    let bytes = deep.bytes();
    assert_eq!(painted_text(&bytes), vec![FIRST, SECOND]);
    assert_rejected(&bytes, WorkerFailureCode::InspectionResourceLimitExceeded);
}

#[test]
fn explicit_root_language_shadows_the_catalog_language_default() {
    let bytes = |catalog_language| {
        let mut tagged = TaggedPdf::baseline();
        tagged
            .dictionary(tagged.catalog)
            .set("Lang", Object::string_literal(catalog_language));
        tagged
            .dictionary(tagged.document_element)
            .set("Lang", Object::string_literal("ja-JP"));
        tagged.bytes()
    };
    let english_catalog = bytes("en-US");
    let french_catalog = bytes("fr-FR");
    assert_eq!(painted_text(&english_catalog), painted_text(&french_catalog));
    assert_eq!(fingerprint(&english_catalog), fingerprint(&french_catalog));
}

#[test]
fn malformed_catalog_language_default_is_rejected() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut malformed = TaggedPdf::baseline();
    malformed.dictionary(malformed.catalog).set("Lang", 7);
    assert_rejected(&malformed.bytes(), WorkerFailureCode::ParserDisagreement);
}

#[test]
fn oversized_catalog_language_default_is_rejected() {
    fingerprint(&TaggedPdf::baseline().bytes());
    let mut oversized = TaggedPdf::baseline();
    oversized
        .dictionary(oversized.catalog)
        .set("Lang", Object::string_literal("a".repeat(257)));
    assert_rejected(
        &oversized.bytes(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn artifact_language_cannot_be_accepted_without_semantic_representation() {
    let mut baseline = TaggedPdf::baseline();
    let content = format!(
        "{}/Artifact << >> BDC\nBT /F1 12 Tf 12 20 Td (ARTIFACT NOTE) Tj ET\nEMC\n",
        baseline.content_text()
    );
    baseline.replace_content(content.clone());
    let baseline = baseline.bytes();
    fingerprint(&baseline);
    let mut changed = TaggedPdf::baseline();
    changed.replace_content(content.replace(
        "/Artifact << >>",
        "/Artifact << /Lang (fr-FR) >>",
    ));
    let changed = changed.bytes();
    assert_eq!(painted_text(&baseline), painted_text(&changed));
    assert_rejected(&changed, WorkerFailureCode::UnsupportedSemanticConstruct);
}

#[test]
fn catalog_language_on_artifact_text_is_not_shadowed_by_structured_root_language() {
    let bytes = |catalog_language| {
        let mut tagged = TaggedPdf::baseline();
        tagged
            .dictionary(tagged.catalog)
            .set("Lang", Object::string_literal(catalog_language));
        tagged
            .dictionary(tagged.document_element)
            .set("Lang", Object::string_literal("ja-JP"));
        tagged.replace_content(format!(
            "{}/Artifact << >> BDC\nBT /F1 12 Tf 12 20 Td (ARTIFACT NOTE) Tj ET\nEMC\n",
            tagged.content_text()
        ));
        tagged.bytes()
    };
    let english = bytes("en-US");
    let french = bytes("fr-FR");
    assert_eq!(painted_text(&english), vec![FIRST, SECOND, "ARTIFACT NOTE"]);
    assert_eq!(painted_text(&english), painted_text(&french));
    assert_ne!(
        fingerprint(&english),
        fingerprint(&french),
        "the structured root language does not govern unowned artifact text"
    );
}

#[test]
fn catalog_language_on_artifact_only_pages_changes_identity_without_a_structure_root() {
    let bytes = |catalog_language| {
        let mut artifact = TaggedPdf::baseline();
        artifact.dictionary(artifact.catalog).remove(b"StructTreeRoot");
        artifact.dictionary(artifact.catalog).remove(b"MarkInfo");
        artifact
            .dictionary(artifact.catalog)
            .set("Lang", Object::string_literal(catalog_language));
        artifact.dictionary(artifact.page).remove(b"StructParents");
        artifact.replace_content(
            "/Artifact << >> BDC\nBT /F1 12 Tf 12 100 Td (ARTIFACT NOTE) Tj ET\nEMC\n".into(),
        );
        artifact.bytes()
    };
    let english = bytes("en-US");
    let french = bytes("fr-FR");
    assert_eq!(painted_text(&english), vec!["ARTIFACT NOTE"]);
    assert_eq!(painted_text(&english), painted_text(&french));
    assert_ne!(
        fingerprint(&english),
        fingerprint(&french),
        "catalog language applies to nonempty artifact text without a structure tree"
    );
}

#[test]
fn adding_an_effective_catalog_language_changes_tagged_identity() {
    let missing = TaggedPdf::baseline().bytes();
    let mut present = TaggedPdf::baseline();
    present
        .dictionary(present.catalog)
        .set("Lang", Object::string_literal("en-US"));
    let present = present.bytes();
    assert_eq!(painted_text(&missing), painted_text(&present));
    assert_ne!(
        fingerprint(&missing),
        fingerprint(&present),
        "introducing an effective tagged-content language changes semantics"
    );
}

#[test]
fn legacy_unmarked_pdf_does_not_gain_catalog_language_identity() {
    let bytes = |language: Option<&str>| {
        let mut legacy = TaggedPdf::baseline();
        legacy.dictionary(legacy.catalog).remove(b"StructTreeRoot");
        legacy.dictionary(legacy.catalog).remove(b"MarkInfo");
        legacy.dictionary(legacy.page).remove(b"StructParents");
        if let Some(language) = language {
            legacy
                .dictionary(legacy.catalog)
                .set("Lang", Object::string_literal(language));
        }
        legacy.replace_content(
            legacy
                .content_text()
                .replace("/P << /MCID 0 >> BDC\n", "")
                .replace("/P << /MCID 1 >> BDC\n", "")
                .replace("EMC\n", ""),
        );
        legacy.bytes()
    };
    let missing = bytes(None);
    let english = bytes(Some("en-US"));
    let french = bytes(Some("fr-FR"));
    assert_eq!(painted_text(&missing), vec![FIRST, SECOND]);
    assert_eq!(painted_text(&missing), painted_text(&english));
    assert_eq!(painted_text(&missing), painted_text(&french));
    assert_eq!(fingerprint(&missing), fingerprint(&english));
    assert_eq!(fingerprint(&missing), fingerprint(&french));
}

#[test]
fn structure_root_cannot_own_a_marked_content_reference_directly() {
    fingerprint(&TaggedPdf::baseline().bytes());
    for array_wrapped in [false, true] {
        let mut malformed = TaggedPdf::baseline();
        let content = malformed.content_text();
        let (first, _) = content.split_once("EMC\n").unwrap();
        malformed.replace_content(format!("{first}EMC\n"));
        let page = malformed.page;
        let root = malformed.structure_root;
        let mcr = Object::Dictionary(dictionary! {
            "Type" => "MCR", "Pg" => Object::Reference(page), "MCID" => 0,
        });
        let child = if array_wrapped {
            Object::Array(vec![mcr])
        } else {
            mcr
        };
        malformed.dictionary(root).set("K", child);
        malformed.dictionary(malformed.parent_tree).set(
            "Nums",
            vec![
                Object::Integer(0),
                Object::Array(vec![Object::Reference(root)]),
            ],
        );
        let bytes = malformed.bytes();
        assert_eq!(painted_text(&bytes), vec![FIRST]);
        assert_rejected(&bytes, WorkerFailureCode::ParserDisagreement);
    }
}
