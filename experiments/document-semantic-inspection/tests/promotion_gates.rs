use document_semantic_inspection_poc::{
    aggregate_promotion_gates, CaseReport, CaseVerdict, ExternalGateEvidence, FixtureClass,
    FixtureGateCounts, FormatId, GateCount, VerificationReport,
};
use std::collections::BTreeMap;

fn case(
    id: &str,
    class: FixtureClass,
    format: FormatId,
    verdict: CaseVerdict,
) -> CaseReport {
    CaseReport {
        id: id.to_owned(),
        class,
        format,
        verdict,
        semantic_fingerprint: None,
        error_code: None,
        message: String::new(),
    }
}

fn passing_report() -> VerificationReport {
    VerificationReport {
        passed: true,
        cases: vec![
            case("docx/base", FixtureClass::Base, FormatId::Docx, CaseVerdict::Pass),
            case("docx/semantic-a", FixtureClass::Semantic, FormatId::Docx, CaseVerdict::Pass),
            case("docx/semantic-b", FixtureClass::Semantic, FormatId::Docx, CaseVerdict::Pass),
            case("docx/noise", FixtureClass::Noise, FormatId::Docx, CaseVerdict::Pass),
            case("docx/editorial", FixtureClass::Editorial, FormatId::Docx, CaseVerdict::Pass),
            case("docx/hostile", FixtureClass::Hostile, FormatId::Docx, CaseVerdict::Pass),
        ],
    }
}

fn full_external_evidence() -> ExternalGateEvidence {
    ExternalGateEvidence {
        supplemental_fixtures: BTreeMap::new(),
        determinism: BTreeMap::from([(
            FormatId::Docx,
            GateCount {
                passed: 5,
                required: 5,
            },
        )]),
        security_resource: BTreeMap::from([(FormatId::Docx, true)]),
        license_dependency: true,
    }
}

#[test]
fn promotion_is_eligible_only_when_every_required_gate_is_complete() {
    let gates = aggregate_promotion_gates(&passing_report(), &full_external_evidence());
    let docx = gates.get(&FormatId::Docx).expect("DOCX gate");

    assert_eq!(docx.semantic_change, GateCount { passed: 2, required: 2 });
    assert_eq!(docx.noise_invariance, GateCount { passed: 1, required: 1 });
    assert_eq!(docx.editorial, GateCount { passed: 1, required: 1 });
    assert_eq!(docx.fail_closed, GateCount { passed: 1, required: 1 });
    assert_eq!(docx.determinism, GateCount { passed: 5, required: 5 });
    assert!(docx.security_resource);
    assert!(docx.license_dependency);
    assert!(docx.promotion_eligible);
}

#[test]
fn incomplete_determinism_security_or_license_gate_blocks_promotion() {
    let report = passing_report();

    let mut evidence = full_external_evidence();
    evidence.determinism.insert(
        FormatId::Docx,
        GateCount {
            passed: 4,
            required: 5,
        },
    );
    assert!(
        !aggregate_promotion_gates(&report, &evidence)[&FormatId::Docx].promotion_eligible
    );

    let mut evidence = full_external_evidence();
    evidence.security_resource.insert(FormatId::Docx, false);
    assert!(
        !aggregate_promotion_gates(&report, &evidence)[&FormatId::Docx].promotion_eligible
    );

    let mut evidence = full_external_evidence();
    evidence.license_dependency = false;
    assert!(
        !aggregate_promotion_gates(&report, &evidence)[&FormatId::Docx].promotion_eligible
    );
}

#[test]
fn any_failed_required_fixture_blocks_promotion() {
    let mut report = passing_report();
    report.passed = false;
    report.cases[1].verdict = CaseVerdict::Fail;
    let gate = &aggregate_promotion_gates(&report, &full_external_evidence())[&FormatId::Docx];
    assert_eq!(gate.semantic_change, GateCount { passed: 1, required: 2 });
    assert!(!gate.promotion_eligible);
}

#[test]
fn supplemental_xlsm_fixture_evidence_prevents_zero_over_zero_promotion() {
    let report = VerificationReport {
        passed: true,
        cases: vec![case(
            "xlsm/base",
            FixtureClass::Base,
            FormatId::Xlsm,
            CaseVerdict::Pass,
        )],
    };
    let evidence = ExternalGateEvidence {
        supplemental_fixtures: BTreeMap::from([(
            FormatId::Xlsm,
            FixtureGateCounts {
                semantic_change: GateCount { passed: 1, required: 1 },
                noise_invariance: GateCount { passed: 2, required: 2 },
                editorial: GateCount::default(),
                fail_closed: GateCount { passed: 1, required: 1 },
            },
        )]),
        determinism: BTreeMap::from([(
            FormatId::Xlsm,
            GateCount { passed: 1, required: 1 },
        )]),
        security_resource: BTreeMap::from([(FormatId::Xlsm, true)]),
        license_dependency: true,
    };
    let gate = &aggregate_promotion_gates(&report, &evidence)[&FormatId::Xlsm];
    assert_eq!(gate.semantic_change, GateCount { passed: 1, required: 1 });
    assert_eq!(gate.noise_invariance, GateCount { passed: 2, required: 2 });
    assert_eq!(gate.fail_closed, GateCount { passed: 1, required: 1 });
    assert!(gate.promotion_eligible);
}
