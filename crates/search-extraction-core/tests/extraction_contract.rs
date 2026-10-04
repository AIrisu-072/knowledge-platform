use std::collections::BTreeMap;

use search_core::knowledge_unit::{
    ArchiveProfilePlan, ArchiveReaderNode, BudgetKey, ExtractionProfileDefinitionV1,
    ExtractionProfileId, FormatId, FormatSettings, NativeLocator, RawBinding, UnitKind,
};
use search_extraction_core::{
    BodyCoverage, BudgetMeter, CoverageReason, ExtractionBudgets, ItemOperationState,
    NativeOmission, PermanentFailureCode, ReaderFailure, RegisteredProfile, RetryableFailureCode,
    WorkerFragment, WorkerOperation, WorkerReport, WorkerRequest, WorkerResponse,
    checked_completed_report, decode_request, decode_response, encode_request, encode_response,
    resource_limit_failure, validate_worker_report, validate_worker_request,
};

fn limits() -> BTreeMap<BudgetKey, u64> {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 100);
    limits.insert(BudgetKey::Units, 4);
    limits.insert(BudgetKey::UnitUtf8Bytes, 32);
    limits.insert(BudgetKey::WorkerOutputBytes, 4_096);
    limits
}

fn definition(format: FormatId) -> ExtractionProfileDefinitionV1 {
    ExtractionProfileDefinitionV1 {
        format,
        parser_name: "qualified-reader".into(),
        parser_version: "1".into(),
        parser_build_sha256: [7; 32],
        native_binary_sha256: (format == FormatId::Pdf).then_some([8; 32]),
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: match format {
            FormatId::Text => FormatSettings::Text {
                charset: "utf-8".into(),
            },
            FormatId::Zip => FormatSettings::Archive {
                member_decoder: "utf-8".into(),
            },
            _ => FormatSettings::None,
        },
        limits: limits(),
    }
}

fn text_profile() -> RegisteredProfile {
    RegisteredProfile::register_definition(definition(FormatId::Text)).unwrap()
}

fn text_fragment(ordinal: u32) -> WorkerFragment {
    WorkerFragment {
        ordinal,
        parent_ordinal: None,
        kind: UnitKind::PlainText,
        text: "東京".into(),
        locator: NativeLocator::Text {
            line_start: ordinal,
            line_end: ordinal + 1,
        },
    }
}

fn report() -> WorkerReport {
    WorkerReport {
        coverage: BodyCoverage::Supported,
        fragments: vec![text_fragment(0)],
        reader_use: vec![],
        scope_items: 1,
        known_omissions: vec![],
        traversal_complete: true,
    }
}

fn request(profile: &RegisteredProfile) -> WorkerRequest {
    WorkerRequest {
        operation: WorkerOperation::Extract,
        format: FormatId::Text,
        profile: profile.id().clone(),
        profile_bytes: profile.profile_bytes().to_vec(),
        expected_raw: RawBinding {
            sha256: [1; 32],
            size_bytes: 6,
            media_type: "text/plain".into(),
        },
        budgets: profile.budgets().clone(),
    }
}

#[test]
fn registry_recomputes_canonical_profile_and_rejects_worker_minting() {
    let profile = text_profile();
    assert_eq!(
        ExtractionProfileId::for_definition(profile.definition()).unwrap(),
        profile.id().clone()
    );
    assert_eq!(profile.parser_build_sha256(), &[7; 32]);
    assert_eq!(profile.native_binary_sha256(), None);
    let mut changed = definition(FormatId::Text);
    changed.limits.insert(BudgetKey::Units, 3);
    assert_ne!(
        RegisteredProfile::register_definition(changed)
            .unwrap()
            .id(),
        profile.id()
    );
    let mut bad = definition(FormatId::Pdf);
    bad.native_binary_sha256 = None;
    assert!(RegisteredProfile::register_definition(bad).is_err());

    let original = request(&profile);
    validate_worker_request(&original, &profile).unwrap();
    let mut forged = original.clone();
    forged.profile = ExtractionProfileId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    assert!(validate_worker_request(&forged, &profile).is_err());
    forged = original.clone();
    forged.profile_bytes.push(0);
    assert!(validate_worker_request(&forged, &profile).is_err());
    forged = original.clone();
    forged.expected_raw.size_bytes = 101;
    assert!(validate_worker_request(&forged, &profile).is_err());
}

#[test]
fn supported_empty_and_partial_positive_require_complete_traversal_and_known_omission() {
    let profile = text_profile();
    let mut valid = report();
    validate_worker_report(&valid, &profile).unwrap();
    valid.fragments.clear();
    valid.scope_items = 0;
    validate_worker_report(&valid, &profile).unwrap(); // genuinely empty body

    valid = report();
    valid.coverage = BodyCoverage::Partial {
        reasons: vec![CoverageReason::UnsupportedStructure],
    };
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.known_omissions = vec![NativeOmission {
        package_path: None,
        physical_child_path: vec![3],
        reason: CoverageReason::UnsupportedStructure,
    }];
    validate_worker_report(&valid, &profile).unwrap(); // verified fragment + blocking gap
    valid.traversal_complete = false;
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.traversal_complete = true;
    valid.fragments.clear();
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments = vec![text_fragment(0)];
    valid.coverage = BodyCoverage::Partial { reasons: vec![] };
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.coverage = BodyCoverage::Supported;
    assert!(validate_worker_report(&valid, &profile).is_err());
}

#[test]
fn partial_rejects_resource_limit_even_with_a_valid_fragment_and_complete_traversal() {
    let profile = text_profile();
    let mut partial = report();
    partial.coverage = BodyCoverage::Partial {
        reasons: vec![CoverageReason::UnsupportedStructure],
    };
    partial.known_omissions = vec![NativeOmission {
        package_path: None,
        physical_child_path: vec![3],
        reason: CoverageReason::UnsupportedStructure,
    }];
    validate_worker_report(&partial, &profile).unwrap();

    partial.coverage = BodyCoverage::Partial {
        reasons: vec![CoverageReason::ResourceLimit],
    };
    partial.known_omissions[0].reason = CoverageReason::ResourceLimit;
    assert!(validate_worker_report(&partial, &profile).is_err());
    assert!(checked_completed_report(&WorkerResponse::Report(partial.clone()), &profile).is_err());

    partial.coverage = BodyCoverage::Partial {
        reasons: vec![
            CoverageReason::UnsupportedStructure,
            CoverageReason::ResourceLimit,
        ],
    };
    partial.known_omissions[0].reason = CoverageReason::UnsupportedStructure;
    partial.known_omissions.push(NativeOmission {
        package_path: None,
        physical_child_path: vec![4],
        reason: CoverageReason::ResourceLimit,
    });
    assert!(validate_worker_report(&partial, &profile).is_err());
}

#[test]
fn terminal_unsupported_has_no_traversal_or_positive_witness() {
    let profile = text_profile();
    for reason in [CoverageReason::ResourceLimit, CoverageReason::Encrypted] {
        let terminal = WorkerReport {
            coverage: BodyCoverage::Unsupported { reason },
            fragments: vec![],
            reader_use: vec![],
            scope_items: 0,
            known_omissions: vec![],
            traversal_complete: false,
        };
        validate_worker_report(&terminal, &profile).unwrap();
        let mut forged = terminal.clone();
        forged.fragments.push(text_fragment(0));
        assert!(validate_worker_report(&forged, &profile).is_err());
        let mut forged = terminal.clone();
        forged.scope_items = 1;
        assert!(validate_worker_report(&forged, &profile).is_err());
        let mut forged = terminal.clone();
        forged.reader_use.push(ArchiveReaderNode {
            members: vec![],
            parser_build_id: "unvisited".into(),
            definition: definition(FormatId::Text),
        });
        assert!(validate_worker_report(&forged, &profile).is_err());
        let mut forged = terminal.clone();
        forged.coverage = BodyCoverage::Supported;
        assert!(validate_worker_report(&forged, &profile).is_err());
        let mut forged = terminal;
        forged.known_omissions.push(NativeOmission {
            package_path: None,
            physical_child_path: vec![1],
            reason,
        });
        assert!(validate_worker_report(&forged, &profile).is_err());
    }
}

#[test]
fn hard_budget_abort_keeps_unsupported_resource_limit_at_zero_units() {
    let profile = text_profile();
    let failure = resource_limit_failure();
    assert_eq!(
        failure,
        ReaderFailure::Unsupported(CoverageReason::ResourceLimit)
    );
    assert_eq!(
        failure.item_outcome(),
        (
            ItemOperationState::Completed,
            Some(BodyCoverage::Unsupported {
                reason: CoverageReason::ResourceLimit
            })
        )
    );
    assert!(checked_completed_report(&WorkerResponse::Failure(failure), &profile).is_err());

    let mut unsupported = report();
    unsupported.coverage = BodyCoverage::Unsupported {
        reason: CoverageReason::ResourceLimit,
    };
    assert!(validate_worker_report(&unsupported, &profile).is_err());
    unsupported.fragments.clear();
    validate_worker_report(&unsupported, &profile).unwrap();
}

#[test]
fn partial_missing_formula_cache_keeps_a_verified_spreadsheet_fragment() {
    let profile = RegisteredProfile::register_definition(definition(FormatId::Xlsx)).unwrap();
    let mut partial = report();
    partial.fragments[0].kind = UnitKind::SpreadsheetCell;
    partial.fragments[0].locator = NativeLocator::Spreadsheet {
        sheet_ordinal: 0,
        row: 1,
        col: 1,
    };
    partial.scope_items = 2;
    partial.coverage = BodyCoverage::Partial {
        reasons: vec![CoverageReason::MissingFormulaCache],
    };
    partial.known_omissions = vec![NativeOmission {
        package_path: None,
        physical_child_path: vec![0, 1, 2],
        reason: CoverageReason::MissingFormulaCache,
    }];
    validate_worker_report(&partial, &profile).unwrap();
}

#[test]
fn unsupported_and_broken_fragments_cannot_publish_units() {
    let profile = text_profile();
    let mut valid = report();
    valid.coverage = BodyCoverage::Unsupported {
        reason: CoverageReason::RequiresOcr,
    };
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments.clear();
    validate_worker_report(&valid, &profile).unwrap();

    valid = report();
    valid.fragments[0].text = "e\u{301}".into();
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments[0].text = "東京".into();
    valid.fragments[0].kind = UnitKind::PdfText;
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments[0].kind = UnitKind::PlainText;
    valid.fragments[0].parent_ordinal = Some(0);
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments[0].parent_ordinal = None;
    valid.fragments.push(text_fragment(1));
    valid.fragments[1].locator = valid.fragments[0].locator.clone();
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.fragments[1].locator = NativeLocator::Text {
        line_start: 1,
        line_end: 2,
    };
    valid.fragments[1].ordinal = 3;
    assert!(validate_worker_report(&valid, &profile).is_err());
}

#[test]
fn full_failure_matrix_retains_publish_boundary() {
    for reason in [
        CoverageReason::RequiresOcr,
        CoverageReason::UnsupportedFormat,
        CoverageReason::Encrypted,
        CoverageReason::UnsupportedStructure,
        CoverageReason::UnsupportedEncoding,
        CoverageReason::UnsupportedDialect,
        CoverageReason::UnsupportedCodec,
        CoverageReason::MissingFormulaCache,
        CoverageReason::AmbiguousReadingOrder,
        CoverageReason::DynamicVisibility,
        CoverageReason::ResourceLimit,
    ] {
        let failure = ReaderFailure::Unsupported(reason);
        assert_eq!(
            failure.item_outcome(),
            (
                ItemOperationState::Completed,
                Some(BodyCoverage::Unsupported { reason })
            )
        );
        assert_eq!(
            decode_response(&encode_response(&WorkerResponse::Failure(failure)).unwrap()).unwrap(),
            WorkerResponse::Failure(failure)
        );
    }
    for code in [
        PermanentFailureCode::CorruptDocument,
        PermanentFailureCode::MalformedArchive,
        PermanentFailureCode::TextExtractionFailed,
        PermanentFailureCode::WorkerOutputLimit,
    ] {
        let failure = ReaderFailure::Permanent(code);
        assert_eq!(
            failure.item_outcome(),
            (ItemOperationState::FailedPermanent { code }, None)
        );
        assert!(failure.item_outcome().0.publishable());
        assert_eq!(
            decode_response(&encode_response(&WorkerResponse::Failure(failure)).unwrap()).unwrap(),
            WorkerResponse::Failure(failure)
        );
    }
    for code in [
        RetryableFailureCode::SourceIo,
        RetryableFailureCode::WorkerUnavailable,
        RetryableFailureCode::WorkerKilled,
        RetryableFailureCode::Timeout,
    ] {
        let failure = ReaderFailure::Retryable(code);
        assert_eq!(
            failure.item_outcome(),
            (ItemOperationState::Retryable { code }, None)
        );
        assert!(!failure.item_outcome().0.publishable());
        assert_eq!(
            decode_response(&encode_response(&WorkerResponse::Failure(failure)).unwrap()).unwrap(),
            WorkerResponse::Failure(failure)
        );
        assert!(
            checked_completed_report(&WorkerResponse::Failure(failure), &text_profile()).is_err()
        );
    }
    let profile = text_profile();
    let mut interrupted = report();
    interrupted.traversal_complete = false; // kill, panic, truncation, or missing final marker
    assert!(checked_completed_report(&WorkerResponse::Report(interrupted), &profile).is_err());
}

#[test]
fn fifteen_budget_keys_have_ceiling_applicability_and_distinct_peak_semantics() {
    assert_eq!(BudgetKey::ALL.len(), 15);
    let all = limits();
    let budgets = ExtractionBudgets::new(all.clone()).unwrap();
    budgets.validate_for(FormatId::Text).unwrap();
    for key in BudgetKey::ALL {
        let mut missing = all.clone();
        missing.remove(&key);
        assert!(ExtractionBudgets::new(missing).is_err(), "{key:?}");
        let mut excess = all.clone();
        excess.insert(key, u64::MAX);
        assert!(ExtractionBudgets::new(excess).is_err(), "{key:?}");
    }
    let mut inappropriate = all.clone();
    inappropriate.insert(BudgetKey::PdfPages, 1);
    assert!(
        ExtractionBudgets::new(inappropriate)
            .unwrap()
            .validate_for(FormatId::Text)
            .is_err()
    );

    let mut meter = BudgetMeter::new(budgets).unwrap();
    meter.charge_input_once(6).unwrap();
    assert!(meter.charge_input_once(6).is_err());
    meter.charge(BudgetKey::Units, 2).unwrap();
    meter.charge(BudgetKey::Units, 2).unwrap();
    assert!(meter.charge(BudgetKey::Units, 1).is_err());
    assert_eq!(meter.charged(BudgetKey::Units), 4);
    meter.charge(BudgetKey::UnitUtf8Bytes, 30).unwrap();
    meter.charge(BudgetKey::UnitUtf8Bytes, 30).unwrap(); // two individually valid Units
    assert_eq!(meter.charged(BudgetKey::UnitUtf8Bytes), 30);
    assert!(meter.charge(BudgetKey::UnitUtf8Bytes, 33).is_err());
    meter.charge(BudgetKey::WorkerOutputBytes, 4_000).unwrap();
    assert!(meter.charge(BudgetKey::WorkerOutputBytes, 97).is_err());
    assert_eq!(meter.charged(BudgetKey::WorkerOutputBytes), 4_000);
    assert!(
        meter
            .charge(BudgetKey::WorkerOutputBytes, u64::MAX)
            .is_err()
    );

    let mut zip_limits = all;
    zip_limits.insert(BudgetKey::ZipEntryBytes, 80);
    zip_limits.insert(BudgetKey::ZipTotalBytes, 100);
    zip_limits.insert(BudgetKey::ZipDepth, 2);
    let mut zip_meter = BudgetMeter::new(ExtractionBudgets::new(zip_limits).unwrap()).unwrap();
    zip_meter.charge(BudgetKey::ZipEntryBytes, 60).unwrap();
    zip_meter.charge(BudgetKey::ZipEntryBytes, 60).unwrap(); // per entry, not combined
    assert_eq!(zip_meter.charged(BudgetKey::ZipEntryBytes), 60);
    zip_meter.charge(BudgetKey::ZipTotalBytes, 60).unwrap();
    assert!(zip_meter.charge(BudgetKey::ZipTotalBytes, 60).is_err());
    zip_meter.charge(BudgetKey::ZipDepth, 2).unwrap();
    assert!(zip_meter.charge(BudgetKey::ZipDepth, 3).is_err());

    for key in BudgetKey::ALL {
        let mut each_limits = limits();
        each_limits.insert(key, 1);
        let mut each = BudgetMeter::new(ExtractionBudgets::new(each_limits).unwrap()).unwrap();
        each.charge(key, 1).unwrap();
        let peak = matches!(
            key,
            BudgetKey::ZipEntryBytes
                | BudgetKey::ZipDepth
                | BudgetKey::UnitUtf8Bytes
                | BudgetKey::XmlDepth
                | BudgetKey::CsvFieldBytes
        );
        assert_eq!(each.charge(key, 1).is_ok(), peak, "{key:?}");
        assert!(each.charge(key, 2).is_err(), "{key:?}");
        if !peak {
            assert!(each.check_value(key, 0).is_err(), "{key:?}");
        }
    }
}

#[test]
fn two_valid_unit_texts_can_exceed_aggregate_worker_output_budget() {
    let mut one = report();
    one.fragments[0].text = "x".repeat(30);
    let one_size = encode_response(&WorkerResponse::Report(one.clone()))
        .unwrap()
        .len();
    let mut definition = definition(FormatId::Text);
    definition
        .limits
        .insert(BudgetKey::WorkerOutputBytes, one_size as u64);
    let profile = RegisteredProfile::register_definition(definition).unwrap();
    validate_worker_report(&one, &profile).unwrap();
    one.fragments.push(WorkerFragment {
        text: "y".repeat(30),
        ..text_fragment(1)
    });
    one.scope_items = 2;
    assert!(validate_worker_report(&one, &profile).is_err());
}

#[test]
fn wire_rejects_unknown_tags_truncation_trailing_bytes_and_oversize() {
    let profile = text_profile();
    let original = request(&profile);
    let bytes = encode_request(&original).unwrap();
    assert_eq!(decode_request(&bytes).unwrap(), original);
    let response = WorkerResponse::Report(report());
    let response_bytes = encode_response(&response).unwrap();
    assert_eq!(decode_response(&response_bytes).unwrap(), response);

    let body_at = b"search-extraction:v1\0".len() + 1 + 4;
    let mut unknown = bytes.clone();
    unknown[body_at] = 99;
    assert!(decode_request(&unknown).is_err());
    let mut unknown = bytes.clone();
    unknown[body_at + 1] = 99;
    assert!(decode_request(&unknown).is_err());
    let mut unknown = response_bytes.clone();
    unknown[body_at] = 99;
    assert!(decode_response(&unknown).is_err());
    assert!(decode_request(&bytes[..bytes.len() - 1]).is_err());
    assert!(decode_response(&response_bytes[..response_bytes.len() - 1]).is_err());
    let mut trailing = response_bytes.clone();
    trailing.push(0);
    assert!(decode_response(&trailing).is_err());
    let mut huge_frame = bytes.clone();
    huge_frame[b"search-extraction:v1\0".len() + 1..body_at]
        .copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(decode_request(&huge_frame).is_err());
    assert!(decode_response(&vec![0; 16_777_217]).is_err());
    let mut unknown_failure = encode_response(&WorkerResponse::Failure(ReaderFailure::Permanent(
        PermanentFailureCode::CorruptDocument,
    )))
    .unwrap();
    unknown_failure[body_at] = 99;
    assert!(decode_response(&unknown_failure).is_err());
}

#[test]
fn archive_dispatch_must_match_all_registered_nodes_and_leaf_kind() {
    let mut root = definition(FormatId::Zip);
    root.limits.insert(BudgetKey::ZipEntries, 2);
    root.limits.insert(BudgetKey::ZipEntryBytes, 100);
    root.limits.insert(BudgetKey::ZipTotalBytes, 100);
    root.limits.insert(BudgetKey::ZipDepth, 1);
    let plan = ArchiveProfilePlan {
        nodes: vec![
            ArchiveReaderNode {
                members: vec![],
                parser_build_id: "outer".into(),
                definition: root,
            },
            ArchiveReaderNode {
                members: vec!["leaf.txt".into()],
                parser_build_id: "inner".into(),
                definition: definition(FormatId::Text),
            },
        ],
        used_leaf_chains: vec![vec!["leaf.txt".into()]],
    };
    let profile = RegisteredProfile::register_archive(plan.clone()).unwrap();
    assert_eq!(
        ExtractionProfileId::for_archive(&plan).unwrap(),
        profile.id().clone()
    );
    let mut valid = report();
    valid.reader_use = plan.nodes.clone();
    valid.fragments[0].locator = NativeLocator::Archive {
        members: vec!["leaf.txt".into()],
        inner: Box::new(valid.fragments[0].locator.clone()),
    };
    validate_worker_report(&valid, &profile).unwrap();
    valid.reader_use.pop();
    assert!(validate_worker_report(&valid, &profile).is_err());
    valid.reader_use = plan.nodes;
    valid.fragments[0].locator = NativeLocator::Archive {
        members: vec!["other.txt".into()],
        inner: Box::new(NativeLocator::Text {
            line_start: 0,
            line_end: 1,
        }),
    };
    assert!(validate_worker_report(&valid, &profile).is_err());
}
