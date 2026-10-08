use std::collections::BTreeMap;
use std::sync::Arc;

use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::knowledge_unit::{
    ArchiveProfilePlan, ArchiveReaderNode, BudgetKey, ContentPartRef, DocxStep, EmbeddingCacheKey,
    ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId, FormatSettings, KnowledgeUnit,
    NativeLocator, PptxTextSlot, RawBinding, ResourceVersionRef, UnitAuthorityBinding, UnitId,
    UnitKind, UnitProvenance, VectorAuthorityInput, VectorHitRef, cache_key_matches_authority,
    matches_pinned_unit, restamp_source_snapshot, share_unit_context, validate_archive_member,
    validate_logical_path, validate_part_units,
};
use search_core::knowledge_unit::{TextSpan, normalize_unit_text, text_sha256};
use search_core::projection::ProjectionGenerationKey;
use search_core::source::RetentionMode;
use time::OffsetDateTime;
use uuid::Uuid;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn normalization_and_span() {
    assert_eq!(normalize_unit_text("東京\r\n"), "東京\n");
    assert_eq!(
        hex(&text_sha256("東京\n")),
        "866bff0df548a00eaad416ca1fc987f20d94dae000fee8abd356dfb29bd15934"
    );
    assert_eq!(normalize_unit_text("e\u{301}"), "é");
    assert_eq!(normalize_unit_text("Ａあア,。\rB"), "Ａあア,。\nB");

    let text = "東京\né";
    assert_eq!(
        TextSpan::new(text, 0, 3).unwrap(),
        TextSpan {
            start_byte: 0,
            end_byte: 3
        }
    );
    assert!(TextSpan::new(text, 1, 3).is_err());
    assert!(TextSpan::new(text, 0, 0).is_err());
    assert!(TextSpan::new(text, 0, 99).is_err());
    assert!(TextSpan::new("e\u{301}", 0, 3).is_err());
    assert!(TextSpan::new("a\r\nb", 0, 1).is_err());
}

#[test]
fn locator_golden_and_rejections() {
    let text = NativeLocator::Text {
        line_start: 0,
        line_end: 1,
    };
    assert_eq!(
        hex(&text.encode().unwrap()),
        "6e61746976652d6c6f6361746f723a763100050000000000000001"
    );
    let all = [
        NativeLocator::Docx {
            steps: vec![
                DocxStep::BodyBlock(0),
                DocxStep::Row(1),
                DocxStep::Cell(2),
                DocxStep::CellBlock(3),
            ],
        },
        NativeLocator::Spreadsheet {
            sheet_ordinal: 2,
            row: 3,
            col: 4,
        },
        NativeLocator::Pptx {
            slide_ordinal: 0,
            shape_path: vec![2, 1],
            text_slot: PptxTextSlot::TableCellParagraph {
                row: 1,
                col: 2,
                paragraph: 3,
            },
        },
        NativeLocator::Pdf {
            page_index: 3,
            char_start: 5,
            char_end: 9,
        },
        text.clone(),
        NativeLocator::Csv {
            record: 7,
            field: 8,
        },
        NativeLocator::Html {
            text_node_path: vec![0, 3],
        },
        NativeLocator::Archive {
            members: vec!["outer.zip".into(), "nested/leaf.txt".into()],
            inner: Box::new(text.clone()),
        },
    ];
    for locator in all {
        let bytes = locator.encode().unwrap();
        assert_eq!(NativeLocator::decode(&bytes).unwrap(), locator);
    }
    for invalid in [
        NativeLocator::Docx { steps: vec![] },
        NativeLocator::Docx {
            steps: vec![DocxStep::Row(0)],
        },
        NativeLocator::Docx {
            steps: vec![DocxStep::BodyBlock(0), DocxStep::Cell(0)],
        },
        NativeLocator::Pptx {
            slide_ordinal: 0,
            shape_path: vec![],
            text_slot: PptxTextSlot::ShapeParagraph { paragraph: 0 },
        },
        NativeLocator::Html {
            text_node_path: vec![],
        },
        NativeLocator::Pdf {
            page_index: 0,
            char_start: 2,
            char_end: 2,
        },
        NativeLocator::Text {
            line_start: 1,
            line_end: 0,
        },
        NativeLocator::Archive {
            members: vec![],
            inner: Box::new(text.clone()),
        },
        NativeLocator::Archive {
            members: vec!["a".into()],
            inner: Box::new(NativeLocator::Archive {
                members: vec!["b".into()],
                inner: Box::new(text.clone()),
            }),
        },
    ] {
        assert!(invalid.encode().is_err(), "{invalid:?}");
    }
    for path in [
        "",
        "/absolute",
        "trailing/",
        "a//b",
        "a/./b",
        "a/../b",
        "a\\b",
        "a\0b",
        "e\u{301}",
    ] {
        assert!(validate_logical_path(path).is_err(), "{path:?}");
        assert!(validate_archive_member(path).is_err(), "{path:?}");
    }
    assert!(validate_logical_path("primary/東京").is_ok());
    assert!(validate_archive_member("folder/東京.txt").is_ok());

    let bytes = text.encode().unwrap();
    let mut unknown = bytes.clone();
    unknown[b"native-locator:v1\0".len()] = 99;
    assert!(NativeLocator::decode(&unknown).is_err());
    assert!(NativeLocator::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(NativeLocator::decode(&trailing).is_err());
    let mut huge = b"native-locator:v1\0".to_vec();
    huge.push(8);
    huge.extend_from_slice(&1_u32.to_be_bytes());
    huge.extend_from_slice(&u32::MAX.to_be_bytes());
    assert!(NativeLocator::decode(&huge).is_err());
    let mut non_nfc = b"native-locator:v1\0".to_vec();
    non_nfc.push(8);
    non_nfc.extend_from_slice(&1_u32.to_be_bytes());
    non_nfc.extend_from_slice(&3_u32.to_be_bytes());
    non_nfc.extend_from_slice("e\u{301}".as_bytes());
    non_nfc.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    non_nfc.extend_from_slice(&bytes);
    assert!(NativeLocator::decode(&non_nfc).is_err());
}

fn all_budgets() -> BTreeMap<BudgetKey, u64> {
    [
        BudgetKey::InputBytes,
        BudgetKey::ZipEntries,
        BudgetKey::ZipEntryBytes,
        BudgetKey::ZipTotalBytes,
        BudgetKey::ZipDepth,
        BudgetKey::Units,
        BudgetKey::UnitUtf8Bytes,
        BudgetKey::WorkerOutputBytes,
        BudgetKey::XmlDepth,
        BudgetKey::XmlNodes,
        BudgetKey::PdfPages,
        BudgetKey::PdfOperations,
        BudgetKey::HtmlNodes,
        BudgetKey::CsvRecords,
        BudgetKey::CsvFieldBytes,
    ]
    .into_iter()
    .map(|key| (key, 0))
    .collect()
}

fn definition(format: FormatId) -> ExtractionProfileDefinitionV1 {
    ExtractionProfileDefinitionV1 {
        format,
        parser_name: "p".into(),
        parser_version: "1".into(),
        parser_build_sha256: [0; 32],
        native_binary_sha256: (format == FormatId::Pdf).then_some([1; 32]),
        scope_revision: 1,
        segmentation_revision: 2,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: match format {
            FormatId::Text => FormatSettings::Text {
                charset: "utf-8".into(),
            },
            FormatId::Csv => FormatSettings::Csv {
                charset: "utf-8".into(),
                delimiter: b',',
                quote: b'"',
            },
            FormatId::Zip => FormatSettings::Archive {
                member_decoder: "utf-8".into(),
            },
            _ => FormatSettings::None,
        },
        limits: all_budgets(),
    }
}

fn framed(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    output.extend_from_slice(bytes);
}

fn archive_plan() -> ArchiveProfilePlan {
    ArchiveProfilePlan {
        nodes: vec![
            ArchiveReaderNode {
                members: vec![],
                parser_build_id: "zip-root".into(),
                definition: definition(FormatId::Zip),
            },
            ArchiveReaderNode {
                members: vec!["bundle.zip".into()],
                parser_build_id: "zip-nested".into(),
                definition: definition(FormatId::Zip),
            },
            ArchiveReaderNode {
                members: vec!["bundle.zip".into(), "report.pdf".into()],
                parser_build_id: "pdf-leaf".into(),
                definition: definition(FormatId::Pdf),
            },
            ArchiveReaderNode {
                members: vec!["data.csv".into()],
                parser_build_id: "csv-leaf".into(),
                definition: definition(FormatId::Csv),
            },
            ArchiveReaderNode {
                members: vec!["notes.txt".into()],
                parser_build_id: "text-leaf".into(),
                definition: definition(FormatId::Text),
            },
        ],
        used_leaf_chains: vec![
            vec!["bundle.zip".into(), "report.pdf".into()],
            vec!["data.csv".into()],
            vec!["notes.txt".into()],
        ],
    }
}

#[test]
fn profile_codec_and_archive() {
    let base = definition(FormatId::Text);
    let mut expected = b"extraction-profile:v1\0".to_vec();
    framed(&mut expected, &[6]);
    framed(&mut expected, b"p");
    framed(&mut expected, b"1");
    framed(&mut expected, &[0; 32]);
    framed(&mut expected, &[0]);
    for revision in [1_u32, 2, 1, 1] {
        framed(&mut expected, &revision.to_be_bytes());
    }
    let mut settings = vec![1];
    framed(&mut settings, b"utf-8");
    framed(&mut expected, &settings);
    let mut budgets = 15_u32.to_be_bytes().to_vec();
    for tag in 1..=15 {
        budgets.push(tag);
        budgets.extend_from_slice(&0_u64.to_be_bytes());
    }
    framed(&mut expected, &budgets);
    assert_eq!(base.encode().unwrap(), expected);
    assert_eq!(
        ExtractionProfileDefinitionV1::decode(&expected).unwrap(),
        base
    );
    let id = ExtractionProfileId::for_definition(&base).unwrap();
    assert_eq!(ExtractionProfileId::parse(id.as_str()).unwrap(), id);
    for malformed in [
        "sha256:",
        "SHA256:",
        "sha256:ABCDEF",
        "sha256:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
    ] {
        assert!(ExtractionProfileId::parse(malformed).is_err());
    }
    for changed in [
        {
            let mut x = base.clone();
            x.parser_build_sha256[0] = 1;
            x
        },
        {
            let mut x = base.clone();
            x.scope_revision += 1;
            x
        },
        {
            let mut x = base.clone();
            x.segmentation_revision += 1;
            x
        },
        {
            let mut x = base.clone();
            x.format_settings = FormatSettings::Text {
                charset: "utf-16le".into(),
            };
            x
        },
        {
            let mut x = base.clone();
            x.limits.insert(BudgetKey::InputBytes, 1);
            x
        },
    ] {
        assert_ne!(ExtractionProfileId::for_definition(&changed).unwrap(), id);
    }
    let mut bad_revision = base.clone();
    bad_revision.normalization_revision = 2;
    assert!(bad_revision.encode().is_err());
    bad_revision.normalization_revision = 1;
    bad_revision.locator_revision = 2;
    assert!(bad_revision.encode().is_err());
    let mut bad_parser = base.clone();
    bad_parser.parser_name = "日本語".into();
    assert!(bad_parser.encode().is_err());
    let mut no_pin = definition(FormatId::Pdf);
    no_pin.native_binary_sha256 = None;
    assert!(no_pin.encode().is_err());
    let pinned = ExtractionProfileId::for_definition(&definition(FormatId::Pdf)).unwrap();
    let mut changed_pin = definition(FormatId::Pdf);
    changed_pin.native_binary_sha256 = Some([2; 32]);
    assert_ne!(
        ExtractionProfileId::for_definition(&changed_pin).unwrap(),
        pinned
    );
    let map_start = expected.len() - budgets.len();
    let mut duplicate = expected.clone();
    duplicate[map_start + 4 + 9] = 1;
    assert!(ExtractionProfileDefinitionV1::decode(&duplicate).is_err());
    let mut unknown = expected.clone();
    unknown[map_start + 4] = 16;
    assert!(ExtractionProfileDefinitionV1::decode(&unknown).is_err());
    let mut missing = base.clone();
    missing.limits.remove(&BudgetKey::CsvFieldBytes);
    assert!(missing.encode().is_err());
    let mut malformed_option = expected.clone();
    let option_discriminant =
        b"extraction-profile:v1\0".len() + (4 + 1) + (4 + 1) + (4 + 1) + (4 + 32) + 4;
    malformed_option[option_discriminant] = 2;
    assert!(ExtractionProfileDefinitionV1::decode(&malformed_option).is_err());
    let mut trailing = expected.clone();
    trailing.push(0);
    assert!(ExtractionProfileDefinitionV1::decode(&trailing).is_err());

    let plan = archive_plan();
    let composite = ExtractionProfileId::for_archive(&plan).unwrap();
    let encoded = plan.encode().unwrap();
    assert!(encoded.starts_with(b"extraction-profile:archive:v2\0"));
    assert_eq!(
        ArchiveProfilePlan::decode(&encoded, plan.used_leaf_chains.clone()).unwrap(),
        plan
    );
    for changed in [
        {
            let mut x = plan.clone();
            x.nodes[4].definition.format_settings = FormatSettings::Text {
                charset: "utf-16le".into(),
            };
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[3].definition.format_settings = FormatSettings::Csv {
                charset: "utf-8".into(),
                delimiter: b';',
                quote: b'"',
            };
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[1].definition.format_settings = FormatSettings::Archive {
                member_decoder: "cp932".into(),
            };
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[2].parser_build_id = "pdf-new".into();
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[2].definition.native_binary_sha256 = Some([2; 32]);
            x
        },
    ] {
        assert_ne!(
            ExtractionProfileId::for_archive(&changed).unwrap(),
            composite
        );
    }
    for invalid in [
        {
            let mut x = plan.clone();
            x.nodes.swap(1, 2);
            x
        },
        {
            let mut x = plan.clone();
            x.nodes.push(x.nodes[4].clone());
            x
        },
        {
            let mut x = plan.clone();
            x.nodes.remove(1);
            x
        },
        {
            let mut x = plan.clone();
            x.nodes.push(ArchiveReaderNode {
                members: vec!["unused.txt".into()],
                parser_build_id: "extra".into(),
                definition: definition(FormatId::Text),
            });
            x
        },
        {
            let mut x = plan.clone();
            x.used_leaf_chains.pop();
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[4].members = vec!["e\u{301}".into()];
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[0].definition = definition(FormatId::Text);
            x
        },
        {
            let mut x = plan.clone();
            x.nodes[4].definition = definition(FormatId::Zip);
            x
        },
    ] {
        assert!(
            ExtractionProfileId::for_archive(&invalid).is_err(),
            "{invalid:?}"
        );
    }
    let mut wrong_prefix = encoded.clone();
    wrong_prefix[0] = b'X';
    assert!(ArchiveProfilePlan::decode(&wrong_prefix, plan.used_leaf_chains.clone()).is_err());
    let mut trailing_v2 = encoded.clone();
    trailing_v2.push(0);
    assert!(ArchiveProfilePlan::decode(&trailing_v2, plan.used_leaf_chains.clone()).is_err());
    let mut unknown_v2 = encoded.clone();
    let root_format_tag = b"extraction-profile:archive:v2\0".len()
        + 4 + 4 // node count and first node frame
        + (4 + 4) // empty root chain frame
        + (4 + b"zip-root".len()) // root build ID frame
        + 4 // definition frame
        + b"extraction-profile:v1\0".len()
        + 4; // first definition field frame
    unknown_v2[root_format_tag] = 255;
    assert!(ArchiveProfilePlan::decode(&unknown_v2, plan.used_leaf_chains.clone()).is_err());
}

fn version() -> ResourceVersionRef {
    ResourceVersionRef {
        source_id: SourceId::from_uuid(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        ),
        resource_id: ResourceId::from_uuid(
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
        ),
        source_native_version: "00000000-0000-0000-0000-000000000003".into(),
    }
}

fn part(id: &str, logical_path: &str, ordinal: u32) -> ContentPartRef {
    ContentPartRef {
        source_native_part_id: id.into(),
        logical_path: logical_path.into(),
        ordinal,
    }
}

fn synthetic_profile() -> ExtractionProfileId {
    ExtractionProfileId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap()
}

fn text_locator(start: u32) -> NativeLocator {
    NativeLocator::Text {
        line_start: start,
        line_end: start + 1,
    }
}

#[test]
fn unit_id_golden_and_invalid() {
    let first = UnitId::derive(
        &version(),
        &part("00000000-0000-0000-0000-000000000004", "primary", 0),
        &synthetic_profile(),
        &text_locator(0),
        0,
    )
    .unwrap();
    assert_eq!(
        first.to_string(),
        "ku1:c6b9bfcc8a9d53ee19966146ccfce5a8b2f6f792f7cab53d4a9154377e867ca1"
    );
    let second = UnitId::derive(
        &version(),
        &part("00000000-0000-0000-0000-000000000005", "attachment", 1),
        &synthetic_profile(),
        &text_locator(0),
        0,
    )
    .unwrap();
    assert_eq!(
        second.to_string(),
        "ku1:3b01330d059d71802ec8b3bc216ff9739b3765843892fe4b8fa9bdfa987b115e"
    );
    assert_ne!(first, second);
    assert_eq!(UnitId::parse(&first.to_string()).unwrap(), first);
    for bad in [
        "KU1:abc",
        "ku1:",
        "ku1:ABCDEF",
        "ku1:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
    ] {
        assert!(UnitId::parse(bad).is_err());
    }
    let mut bad_version = version();
    bad_version.source_native_version = "e\u{301}".into();
    assert!(
        UnitId::derive(
            &bad_version,
            &part("p", "primary", 0),
            &synthetic_profile(),
            &text_locator(0),
            0
        )
        .is_err()
    );
    assert!(
        UnitId::derive(
            &version(),
            &part("p", "e\u{301}", 0),
            &synthetic_profile(),
            &text_locator(0),
            0
        )
        .is_err()
    );
    assert!(
        UnitId::derive(
            &version(),
            &part("p", "primary", 0),
            &synthetic_profile(),
            &NativeLocator::Text {
                line_start: 1,
                line_end: 1
            },
            0
        )
        .is_err()
    );
    let plan = archive_plan();
    let locator = NativeLocator::Archive {
        members: vec!["notes.txt".into()],
        inner: Box::new(text_locator(0)),
    };
    let archived = UnitId::derive(
        &version(),
        &part("p", "archive.zip", 0),
        &ExtractionProfileId::for_archive(&plan).unwrap(),
        &locator,
        0,
    )
    .unwrap();
    let mut changed = plan.clone();
    changed.nodes[4].definition.format_settings = FormatSettings::Text {
        charset: "utf-16le".into(),
    };
    assert_ne!(
        UnitId::derive(
            &version(),
            &part("p", "archive.zip", 0),
            &ExtractionProfileId::for_archive(&changed).unwrap(),
            &locator,
            0
        )
        .unwrap(),
        archived
    );
}

fn text_binding() -> UnitAuthorityBinding {
    UnitAuthorityBinding {
        version: version(),
        part: part("00000000-0000-0000-0000-000000000004", "primary", 0),
        source_snapshot: "snapshot-1".into(),
        authoritative_representation_ref: "representation-1".into(),
        raw: RawBinding {
            sha256: [7; 32],
            size_bytes: 13,
            media_type: "text/plain".into(),
        },
        detected_format: FormatId::Text,
        archive_inner_format: None,
        profile: synthetic_profile(),
        parser_build_id: "reader-1".into(),
        archive_plan: None,
    }
}

fn unit(
    binding: &UnitAuthorityBinding,
    ordinal: u32,
    text: &str,
    locator: NativeLocator,
) -> KnowledgeUnit {
    let unit_id = UnitId::derive(
        &binding.version,
        &binding.part,
        &binding.profile,
        &locator,
        ordinal,
    )
    .unwrap();
    KnowledgeUnit {
        unit_id,
        version: binding.version.clone().into(),
        part: binding.part.clone().into(),
        parent_unit_id: None,
        ordinal,
        kind: UnitKind::PlainText,
        text: text.into(),
        locator,
        text_sha256: text_sha256(text),
        provenance: UnitProvenance {
            source_snapshot: binding.source_snapshot.clone(),
            authoritative_representation_ref: binding.authoritative_representation_ref.clone(),
            raw: binding.raw.clone(),
            detected_format: binding.detected_format,
            archive_inner_format: binding.archive_inner_format,
            profile: binding.profile.clone(),
            parser_build_id: binding.parser_build_id.clone(),
        }
        .into(),
    }
}

#[test]
fn shared_unit_context_keeps_values_and_encoding() {
    let binding = text_binding();
    let mut units = vec![
        unit(&binding, 0, "alpha", text_locator(0)),
        unit(&binding, 1, "beta", text_locator(1)),
        unit(&binding, 2, "gamma", text_locator(2)),
    ];
    let mut other = binding.clone();
    other.parser_build_id = "reader-2".into();
    units[2].provenance = unit(&other, 2, "gamma", text_locator(2)).provenance;
    let encoded = serde_json::to_string(&units).unwrap();
    let before = units.clone();

    share_unit_context(&mut units);
    assert_eq!(units, before);
    assert_eq!(serde_json::to_string(&units).unwrap(), encoded);
    assert!(Arc::ptr_eq(&units[0].version, &units[2].version));
    assert!(Arc::ptr_eq(&units[0].part, &units[2].part));
    assert!(Arc::ptr_eq(&units[0].provenance, &units[1].provenance));
    assert!(!Arc::ptr_eq(&units[1].provenance, &units[2].provenance));

    restamp_source_snapshot(&mut units, "snapshot-2");
    assert!(
        units
            .iter()
            .all(|unit| unit.provenance.source_snapshot == "snapshot-2")
    );
    assert!(Arc::ptr_eq(&units[0].provenance, &units[1].provenance));
    assert_eq!(units[2].provenance.parser_build_id, "reader-2");
    assert_eq!(before[0].provenance.source_snapshot, "snapshot-1");
}

#[test]
fn unit_sequence_rejections() {
    let binding = text_binding();
    assert!(validate_part_units(&binding, &[]).is_ok());
    let first = unit(&binding, 0, "alpha", text_locator(0));
    let mut second = unit(&binding, 1, "beta", text_locator(1));
    second.parent_unit_id = Some(first.unit_id);
    assert!(validate_part_units(&binding, &[first.clone(), second.clone()]).is_ok());

    assert!(validate_part_units(&binding, &[second.clone(), first.clone()]).is_err());
    let mut duplicate_ordinal = second.clone();
    duplicate_ordinal.ordinal = 0;
    assert!(validate_part_units(&binding, &[first.clone(), duplicate_ordinal]).is_err());
    let mut duplicate_locator = second.clone();
    duplicate_locator.locator = first.locator.clone();
    duplicate_locator.unit_id = UnitId::derive(
        &binding.version,
        &binding.part,
        &binding.profile,
        &duplicate_locator.locator,
        1,
    )
    .unwrap();
    assert!(validate_part_units(&binding, &[first.clone(), duplicate_locator]).is_err());
    let mut self_parent = first.clone();
    self_parent.parent_unit_id = Some(first.unit_id);
    assert!(validate_part_units(&binding, &[self_parent]).is_err());
    let mut forward_parent = first.clone();
    forward_parent.parent_unit_id = Some(second.unit_id);
    assert!(validate_part_units(&binding, &[forward_parent, second.clone()]).is_err());
    let mut cross_part_parent = second.clone();
    cross_part_parent.parent_unit_id = Some(
        UnitId::derive(
            &binding.version,
            &part("other", "other", 1),
            &binding.profile,
            &text_locator(0),
            0,
        )
        .unwrap(),
    );
    assert!(validate_part_units(&binding, &[first.clone(), cross_part_parent]).is_err());
    let mut non_nfc_text = first.clone();
    non_nfc_text.text = "e\u{301}".into();
    non_nfc_text.text_sha256 = text_sha256(&non_nfc_text.text);
    assert!(validate_part_units(&binding, &[non_nfc_text]).is_err());
    let mut bad_digest = first.clone();
    bad_digest.text_sha256[0] ^= 1;
    assert!(validate_part_units(&binding, &[bad_digest]).is_err());
    let mut bad_id = first.clone();
    bad_id.unit_id = second.unit_id;
    assert!(validate_part_units(&binding, &[bad_id]).is_err());
    let mut bad_kind = first.clone();
    bad_kind.kind = UnitKind::PdfText;
    assert!(validate_part_units(&binding, &[bad_kind]).is_err());
    let mut bad_version = first.clone();
    Arc::make_mut(&mut bad_version.version).source_native_version = "other".into();
    assert!(validate_part_units(&binding, &[bad_version]).is_err());
    let mut bad_part = first.clone();
    Arc::make_mut(&mut bad_part.part).ordinal = 1;
    assert!(validate_part_units(&binding, &[bad_part]).is_err());
    let mut bad_representation = first.clone();
    Arc::make_mut(&mut bad_representation.provenance).authoritative_representation_ref =
        "other".into();
    assert!(validate_part_units(&binding, &[bad_representation]).is_err());
    let mut bad_snapshot = first.clone();
    Arc::make_mut(&mut bad_snapshot.provenance).source_snapshot = "other".into();
    assert!(validate_part_units(&binding, &[bad_snapshot]).is_err());
    let mut bad_raw = first.clone();
    Arc::make_mut(&mut bad_raw.provenance).raw.size_bytes += 1;
    assert!(validate_part_units(&binding, &[bad_raw]).is_err());
    let mut bad_profile = first.clone();
    Arc::make_mut(&mut bad_profile.provenance).profile =
        ExtractionProfileId::for_definition(&definition(FormatId::Text)).unwrap();
    assert!(validate_part_units(&binding, &[bad_profile]).is_err());
    let mut bad_format = first.clone();
    Arc::make_mut(&mut bad_format.provenance).detected_format = FormatId::Csv;
    assert!(validate_part_units(&binding, &[bad_format]).is_err());
    let mut bad_build = first.clone();
    Arc::make_mut(&mut bad_build.provenance).parser_build_id = "other".into();
    assert!(validate_part_units(&binding, &[bad_build]).is_err());
    let mut bad_path_binding = binding.clone();
    bad_path_binding.part.logical_path = "e\u{301}".into();
    assert!(validate_part_units(&bad_path_binding, &[]).is_err());
    let mut bad_mime_binding = binding.clone();
    bad_mime_binding.raw.media_type = "Text/Plain; charset=UTF-8".into();
    assert!(validate_part_units(&bad_mime_binding, &[]).is_err());
    let mut bad_build_binding = binding.clone();
    bad_build_binding.parser_build_id = "日本語".into();
    assert!(validate_part_units(&bad_build_binding, &[]).is_err());

    let mut docx_binding = text_binding();
    docx_binding.detected_format = FormatId::Docx;
    docx_binding.raw.media_type =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into();
    let body = NativeLocator::Docx {
        steps: vec![DocxStep::BodyBlock(0)],
    };
    let mut bad_body_kind = unit(&docx_binding, 0, "alpha", body);
    bad_body_kind.kind = UnitKind::TableCell;
    assert!(validate_part_units(&docx_binding, &[bad_body_kind]).is_err());
    let cell = NativeLocator::Docx {
        steps: vec![
            DocxStep::BodyBlock(0),
            DocxStep::Row(0),
            DocxStep::Cell(0),
            DocxStep::CellBlock(0),
        ],
    };
    let mut bad_cell_kind = unit(&docx_binding, 0, "alpha", cell);
    bad_cell_kind.kind = UnitKind::Paragraph;
    assert!(validate_part_units(&docx_binding, &[bad_cell_kind]).is_err());

    let mut archived_binding = text_binding();
    archived_binding.part.logical_path = "archive.zip".into();
    archived_binding.raw.media_type = "application/zip".into();
    archived_binding.detected_format = FormatId::Zip;
    archived_binding.archive_inner_format = Some(FormatId::Text);
    archived_binding.archive_plan = Some(archive_plan());
    archived_binding.profile =
        ExtractionProfileId::for_archive(archived_binding.archive_plan.as_ref().unwrap()).unwrap();
    archived_binding.parser_build_id = "zip-root".into();
    let archived_locator = NativeLocator::Archive {
        members: vec!["notes.txt".into()],
        inner: Box::new(text_locator(0)),
    };
    let archived_unit = unit(&archived_binding, 0, "alpha", archived_locator);
    assert!(validate_part_units(&archived_binding, std::slice::from_ref(&archived_unit)).is_ok());
    let mut mixed_binding = archived_binding.clone();
    mixed_binding.archive_inner_format = None;
    let mut text_leaf = unit(
        &mixed_binding,
        0,
        "alpha",
        NativeLocator::Archive {
            members: vec!["notes.txt".into()],
            inner: Box::new(text_locator(0)),
        },
    );
    Arc::make_mut(&mut text_leaf.provenance).archive_inner_format = Some(FormatId::Text);
    let mut csv_leaf = unit(
        &mixed_binding,
        1,
        "value",
        NativeLocator::Archive {
            members: vec!["data.csv".into()],
            inner: Box::new(NativeLocator::Csv {
                record: 0,
                field: 0,
            }),
        },
    );
    Arc::make_mut(&mut csv_leaf.provenance).archive_inner_format = Some(FormatId::Csv);
    csv_leaf.kind = UnitKind::CsvField;
    assert!(validate_part_units(&mixed_binding, &[text_leaf.clone(), csv_leaf.clone()]).is_ok());
    let mut false_leaf = csv_leaf.clone();
    Arc::make_mut(&mut false_leaf.provenance).archive_inner_format = Some(FormatId::Text);
    assert!(validate_part_units(&mixed_binding, &[text_leaf, false_leaf]).is_err());
    let mut wrong_chain = archived_unit.clone();
    wrong_chain.locator = NativeLocator::Archive {
        members: vec!["missing.txt".into()],
        inner: Box::new(text_locator(0)),
    };
    wrong_chain.unit_id = UnitId::derive(
        &archived_binding.version,
        &archived_binding.part,
        &archived_binding.profile,
        &wrong_chain.locator,
        0,
    )
    .unwrap();
    assert!(validate_part_units(&archived_binding, &[wrong_chain]).is_err());
    let mut wrong_inner = archived_unit.clone();
    Arc::make_mut(&mut wrong_inner.provenance).archive_inner_format = Some(FormatId::Pdf);
    assert!(validate_part_units(&archived_binding, &[wrong_inner]).is_err());
    let mut no_plan = archived_binding.clone();
    no_plan.archive_plan = None;
    assert!(validate_part_units(&no_plan, &[archived_unit]).is_err());
}

#[test]
fn vector_binding_inputs() {
    let binding = text_binding();
    let unit = unit(&binding, 0, "alpha", text_locator(0));
    let generation = ProjectionGenerationKey {
        source_id: binding.version.source_id,
        generation_id: ProjectionGenerationId::from_uuid(
            Uuid::parse_str("00000000-0000-0000-0000-000000000009").unwrap(),
        ),
    };
    let pinned = VectorAuthorityInput {
        generation,
        version: binding.version.clone(),
        part: binding.part.clone(),
        authoritative_representation_ref: binding.authoritative_representation_ref.clone(),
        raw: binding.raw.clone(),
        profile: binding.profile.clone(),
        authority_scope_key: "tenant-service-owner".into(),
        retention_lease_id: "lease-1".into(),
        lifetime_scope_id: "request-1".into(),
        retention_mode: RetentionMode::NoRetention,
        lease_expires_at: Some(OffsetDateTime::UNIX_EPOCH),
    };
    let hit = VectorHitRef {
        generation,
        unit_id: unit.unit_id,
        version: binding.version.clone(),
        part: binding.part.clone(),
        authoritative_representation_ref: binding.authoritative_representation_ref.clone(),
        raw: binding.raw.clone(),
        profile: binding.profile.clone(),
        text_sha256: unit.text_sha256,
    };
    let key = EmbeddingCacheKey {
        embedding_model_id: "model-1".into(),
        unit_id: unit.unit_id,
        text_sha256: unit.text_sha256,
        profile: binding.profile.clone(),
        source_id: binding.version.source_id,
        authority_scope_key: pinned.authority_scope_key.clone(),
        retention_lease_id: pinned.retention_lease_id.clone(),
        lifetime_scope_id: pinned.lifetime_scope_id.clone(),
    };
    assert_eq!(
        serde_json::from_str::<EmbeddingCacheKey>(&serde_json::to_string(&key).unwrap()).unwrap(),
        key
    );
    assert_eq!(
        serde_json::from_str::<VectorHitRef>(&serde_json::to_string(&hit).unwrap()).unwrap(),
        hit
    );
    assert_eq!(
        serde_json::from_str::<VectorAuthorityInput>(&serde_json::to_string(&pinned).unwrap())
            .unwrap(),
        pinned
    );
    assert!(matches_pinned_unit(&hit, &unit, &pinned));
    assert!(cache_key_matches_authority(&key, &unit, &pinned));
    for changed in [
        {
            let mut x = key.clone();
            x.embedding_model_id = "model-2".into();
            x
        },
        {
            let mut x = key.clone();
            x.unit_id = UnitId::derive(
                &binding.version,
                &binding.part,
                &binding.profile,
                &text_locator(1),
                1,
            )
            .unwrap();
            x
        },
        {
            let mut x = key.clone();
            x.text_sha256[0] ^= 1;
            x
        },
        {
            let mut x = key.clone();
            x.profile = ExtractionProfileId::for_definition(&definition(FormatId::Text)).unwrap();
            x
        },
        {
            let mut x = key.clone();
            x.source_id = SourceId::from_uuid(
                Uuid::parse_str("00000000-0000-0000-0000-000000000010").unwrap(),
            );
            x
        },
        {
            let mut x = key.clone();
            x.authority_scope_key = "other-scope".into();
            x
        },
        {
            let mut x = key.clone();
            x.retention_lease_id = "other-lease".into();
            x
        },
        {
            let mut x = key.clone();
            x.lifetime_scope_id = "other-request".into();
            x
        },
    ] {
        assert_ne!(changed, key);
    }
    for changed in [
        {
            let mut x = hit.clone();
            x.generation.generation_id = ProjectionGenerationId::from_uuid(
                Uuid::parse_str("00000000-0000-0000-0000-000000000011").unwrap(),
            );
            x
        },
        {
            let mut x = hit.clone();
            x.unit_id = UnitId::derive(
                &binding.version,
                &binding.part,
                &binding.profile,
                &text_locator(1),
                1,
            )
            .unwrap();
            x
        },
        {
            let mut x = hit.clone();
            x.version.source_native_version = "other".into();
            x
        },
        {
            let mut x = hit.clone();
            x.part.logical_path = "other".into();
            x
        },
        {
            let mut x = hit.clone();
            x.authoritative_representation_ref = "other".into();
            x
        },
        {
            let mut x = hit.clone();
            x.raw.sha256[0] ^= 1;
            x
        },
        {
            let mut x = hit.clone();
            x.profile = ExtractionProfileId::for_definition(&definition(FormatId::Text)).unwrap();
            x
        },
        {
            let mut x = hit.clone();
            x.text_sha256[0] ^= 1;
            x
        },
    ] {
        assert!(!matches_pinned_unit(&changed, &unit, &pinned));
    }
    for changed in [
        {
            let mut x = pinned.clone();
            x.authority_scope_key = "other".into();
            x
        },
        {
            let mut x = pinned.clone();
            x.retention_lease_id = "other".into();
            x
        },
        {
            let mut x = pinned.clone();
            x.lifetime_scope_id = "other".into();
            x
        },
        {
            let mut x = pinned.clone();
            x.version.source_native_version = "other".into();
            x
        },
        {
            let mut x = pinned.clone();
            x.part.logical_path = "other".into();
            x
        },
        {
            let mut x = pinned.clone();
            x.raw.size_bytes += 1;
            x
        },
        {
            let mut x = pinned.clone();
            x.profile = ExtractionProfileId::for_definition(&definition(FormatId::Text)).unwrap();
            x
        },
    ] {
        assert!(!cache_key_matches_authority(&key, &unit, &changed));
    }
}

#[test]
fn unit_id_text_bytes_equal_its_display() {
    for text in [
        format!("ku1:{}", "0".repeat(64)),
        format!("ku1:{}", "f".repeat(64)),
        format!("ku1:{}", "0123456789abcdef".repeat(4)),
    ] {
        let id = search_core::knowledge_unit::UnitId::parse(&text).unwrap();
        assert_eq!(id.text_bytes().as_slice(), id.to_string().as_bytes());
        assert_eq!(id.to_string(), text);
    }
}
