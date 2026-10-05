//! P1-I05 PDF and explicit ZIP container readers. PDF cases need the pinned
//! PDFium library in `PDFIUM_DYNAMIC_LIB_PATH`; they fail rather than skip.

mod support;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};
use search_core::knowledge_unit::{
    ArchiveProfilePlan, ArchiveReaderNode, BudgetKey, FormatId, FormatSettings, NativeLocator,
    UnitKind,
};
use search_extraction_core::{
    BodyCoverage, CoverageReason, PermanentFailureCode, ReaderFailure, RegisteredProfile,
    WorkerReport, validate_worker_report,
};
use search_extraction_worker::readers::{
    extract_archive_for_test, extract_for_test, warm_up_pdfium,
};

fn pdf(pages: &[Vec<Operation>]) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut kids = Vec::new();
    for operations in pages {
        let content = Content {
            operations: operations.clone(),
        };
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        kids.push(Object::from(page_id));
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => pages.len() as i64,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    document.save_to(&mut out).unwrap();
    out
}

fn text_ops(text: &str) -> Vec<Operation> {
    vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 24.into()]),
        Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                72.into(),
                720.into(),
            ],
        ),
        Operation::new("Tj", vec![Object::string_literal(text)]),
        Operation::new("ET", vec![]),
    ]
}

fn read_pdf(raw: &[u8]) -> Result<WorkerReport, ReaderFailure> {
    warm_up_pdfium().expect("set PDFIUM_DYNAMIC_LIB_PATH to the pinned PDFium 7881 library");
    extract_for_test(
        FormatId::Pdf,
        &FormatSettings::None,
        raw,
        &mut support::meter(FormatId::Pdf),
    )
}

#[test]
fn pdf_native_character_range_single_page() {
    let report = read_pdf(&pdf(&[text_ops("Tokyo 2026")])).unwrap();
    assert_eq!(report.coverage, BodyCoverage::Supported);
    assert_eq!(report.fragments.len(), 1);
    let fragment = &report.fragments[0];
    assert_eq!(fragment.kind, UnitKind::PdfText);
    assert_eq!(fragment.text, "Tokyo 2026");
    let NativeLocator::Pdf {
        page_index,
        char_start,
        char_end,
    } = fragment.locator
    else {
        panic!("PDF locator expected");
    };
    assert_eq!((page_index, char_start), (0, 0));
    assert!(char_end >= 10, "native character index covers the run");
    support::assert_valid(FormatId::Pdf, &report);
}

#[test]
fn pdf_reading_order_image_operator_and_corruption_fail_closed() {
    assert_eq!(
        read_pdf(&pdf(&[text_ops("one"), text_ops("two")])),
        Err(ReaderFailure::Unsupported(
            CoverageReason::AmbiguousReadingOrder
        ))
    );
    let image_only = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                100.into(),
                0.into(),
                0.into(),
                100.into(),
                0.into(),
                0.into(),
            ],
        ),
        Operation::new("Do", vec!["Im1".into()]),
        Operation::new("Q", vec![]),
    ];
    assert_eq!(
        read_pdf(&pdf(&[image_only])),
        Err(ReaderFailure::Unsupported(CoverageReason::RequiresOcr))
    );
    let mut unknown = text_ops("x");
    unknown.insert(3, Operation::new("Td", vec![1.into(), 1.into()]));
    assert_eq!(
        read_pdf(&pdf(&[unknown])),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    assert_eq!(
        read_pdf(b"%PDF-1.5\nnot a document"),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::CorruptDocument
        ))
    );
    let mut meter = support::meter_with(FormatId::Pdf, BudgetKey::PdfOperations, 3);
    assert_eq!(
        extract_for_test(
            FormatId::Pdf,
            &FormatSettings::None,
            &pdf(&[text_ops("x")]),
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
}

fn node(members: &[&str], format: FormatId) -> ArchiveReaderNode {
    ArchiveReaderNode {
        members: members.iter().map(|member| (*member).to_owned()).collect(),
        parser_build_id: format!("reader-{format:?}").to_lowercase(),
        definition: support::definition(format),
    }
}

fn plan(nodes: Vec<ArchiveReaderNode>) -> ArchiveProfilePlan {
    let used_leaf_chains = nodes
        .iter()
        .filter(|node| node.definition.format != FormatId::Zip)
        .map(|node| node.members.clone())
        .collect();
    ArchiveProfilePlan {
        nodes,
        used_leaf_chains,
    }
}

fn read_zip(raw: &[u8], plan: &ArchiveProfilePlan) -> Result<WorkerReport, ReaderFailure> {
    extract_archive_for_test(raw, plan, &mut support::meter(FormatId::Zip))
}

#[test]
fn zip_plan_dispatches_nested_leaves_with_member_chains() {
    let inner = support::zip(&[("c.html", b"<html><body><p>\xe5\x86\x85</p></body></html>")]);
    let raw = support::zip(&[
        ("a.txt", "東京\n".as_bytes()),
        ("b.csv", "同文。,x\n".as_bytes()),
        ("inner.zip", &inner),
    ]);
    let plan = plan(vec![
        node(&[], FormatId::Zip),
        node(&["a.txt"], FormatId::Text),
        node(&["b.csv"], FormatId::Csv),
        node(&["inner.zip"], FormatId::Zip),
        node(&["inner.zip", "c.html"], FormatId::Html),
    ]);
    let report = read_zip(&raw, &plan).unwrap();
    assert_eq!(report.coverage, BodyCoverage::Supported);
    let archive = |members: &[&str], inner: NativeLocator| NativeLocator::Archive {
        members: members.iter().map(|member| (*member).to_owned()).collect(),
        inner: Box::new(inner),
    };
    assert_eq!(
        report
            .fragments
            .iter()
            .map(|fragment| (fragment.text.as_str(), fragment.locator.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                "東京",
                archive(
                    &["a.txt"],
                    NativeLocator::Text {
                        line_start: 0,
                        line_end: 1
                    }
                )
            ),
            (
                "同文。",
                archive(
                    &["b.csv"],
                    NativeLocator::Csv {
                        record: 0,
                        field: 0
                    }
                )
            ),
            (
                "x",
                archive(
                    &["b.csv"],
                    NativeLocator::Csv {
                        record: 0,
                        field: 1
                    }
                )
            ),
            (
                "内",
                archive(
                    &["inner.zip", "c.html"],
                    NativeLocator::Html {
                        text_node_path: vec![0, 0]
                    }
                )
            ),
        ]
    );
    assert_eq!(report.reader_use, plan.nodes);
    let profile = RegisteredProfile::register_archive(plan.clone()).unwrap();
    validate_worker_report(&report, &profile).unwrap();

    // A member missing from the registered plan, or a plan node with no member.
    let unregistered = support::zip(&[("a.txt", b"x\n"), ("extra.txt", b"y\n")]);
    let small = self::plan(vec![
        node(&[], FormatId::Zip),
        node(&["a.txt"], FormatId::Text),
    ]);
    assert_eq!(
        read_zip(&unregistered, &small),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    let unused = self::plan(vec![
        node(&[], FormatId::Zip),
        node(&["a.txt"], FormatId::Text),
        node(&["b.txt"], FormatId::Text),
    ]);
    assert_eq!(
        read_zip(&support::zip(&[("a.txt", b"x\n")]), &unused),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    // One unsupported leaf makes the whole container Unit 0.
    let dynamic = support::zip(&[
        ("a.txt", b"x\n"),
        ("b.html", b"<html><body><script>1</script></body></html>"),
    ]);
    let dynamic_plan = self::plan(vec![
        node(&[], FormatId::Zip),
        node(&["a.txt"], FormatId::Text),
        node(&["b.html"], FormatId::Html),
    ]);
    assert_eq!(
        read_zip(&dynamic, &dynamic_plan),
        Err(ReaderFailure::Unsupported(
            CoverageReason::DynamicVisibility
        ))
    );
}

#[test]
fn zip_leaf_omission_keeps_member_chain() {
    let book = support::workbook(
        support::XLSX_MAIN,
        &[(
            "一",
            r#"<row r="1"><c r="A1" t="inlineStr"><is><t>値</t></is></c><c r="B1"><f>1+1</f></c></row>"#,
            None,
        )],
        None,
        &[],
    );
    let raw = support::zip(&[("book.xlsx", &book)]);
    let plan = plan(vec![
        node(&[], FormatId::Zip),
        node(&["book.xlsx"], FormatId::Xlsx),
    ]);
    let report = read_zip(&raw, &plan).unwrap();
    assert_eq!(
        report.coverage,
        BodyCoverage::Partial {
            reasons: vec![CoverageReason::MissingFormulaCache]
        }
    );
    assert_eq!(
        report.known_omissions[0].package_path.as_deref(),
        Some("book.xlsx/xl/worksheets/sheet1.xml")
    );
    assert_eq!(report.known_omissions[0].physical_child_path, vec![0, 1]);
    let profile = RegisteredProfile::register_archive(plan).unwrap();
    validate_worker_report(&report, &profile).unwrap();
}

/// Rewrite every occurrence of `from` in raw ZIP bytes (names are stored twice).
fn patch(raw: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let mut out = raw.to_vec();
    let mut index = 0;
    while let Some(found) = out[index..]
        .windows(from.len())
        .position(|window| window == from)
    {
        let at = index + found;
        out[at..at + to.len()].copy_from_slice(to);
        index = at + to.len();
    }
    out
}

#[test]
fn zip_hostile_entries_yield_zero_units() {
    let single = plan(vec![
        node(&[], FormatId::Zip),
        node(&["a.txt"], FormatId::Text),
    ]);
    // Duplicate names after patching the second entry's name.
    let duplicate = patch(
        &support::zip(&[("a.txt", b"x\n"), ("b.txt", b"y\n")]),
        b"b.txt",
        b"a.txt",
    );
    assert_eq!(
        read_zip(&duplicate, &single),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    // Path traversal.
    let traversal = patch(
        &support::zip(&[("x/a.txt", b"x\n")]),
        b"x/a.txt",
        b"../a.tx",
    );
    assert_eq!(
        read_zip(&traversal, &single),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    // Compression bomb: highly repetitive member beyond the expansion ratio.
    let bomb = support::zip(&[("a.txt", &vec![b'a'; 4 * 1024 * 1024])]);
    assert_eq!(
        read_zip(&bomb, &single),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
    // Entry budget.
    let mut meter = support::meter_with(FormatId::Zip, BudgetKey::ZipEntries, 1);
    assert_eq!(
        extract_archive_for_test(
            &support::zip(&[("a.txt", b"x\n"), ("b.txt", b"y\n")]),
            &single,
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
    assert_eq!(
        read_zip(b"not a zip at all", &single),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::MalformedArchive
        ))
    );
}
