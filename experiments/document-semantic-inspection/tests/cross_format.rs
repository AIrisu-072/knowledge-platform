use document_semantic_inspection_poc::{
    assess_authority_migration, AuthorityMigrationDecision, CapabilityEvidence, CapabilityState,
};

fn cap(
    id: &str,
    state: CapabilityState,
    version_significant: bool,
    fingerprint: Option<&str>,
) -> CapabilityEvidence {
    CapabilityEvidence::new(
        id,
        state,
        version_significant,
        fingerprint.map(str::to_owned),
    )
}

#[test]
fn xlsm_to_pdf_is_denied_when_formula_vba_and_hidden_semantics_are_not_representable() {
    let source = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-v1")),
        cap("formula_logic", CapabilityState::Present, true, Some("formula-v1")),
        cap("vba_logic", CapabilityState::Present, true, Some("vba-v1")),
        cap("hidden_content", CapabilityState::Present, true, Some("hidden-v1")),
    ];
    let target = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-v1")),
        cap("formula_logic", CapabilityState::NotRepresentable, true, None),
        cap("vba_logic", CapabilityState::NotRepresentable, true, None),
        cap("hidden_content", CapabilityState::NotRepresentable, true, None),
    ];

    assert_eq!(
        assess_authority_migration(&source, &target),
        AuthorityMigrationDecision::Denied {
            missing: vec![
                "formula_logic".to_owned(),
                "hidden_content".to_owned(),
                "vba_logic".to_owned(),
            ],
        }
    );
}

#[test]
fn docx_to_pdf_has_no_blanket_allowlist_and_depends_on_fixture_capabilities() {
    let simple_docx = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-v1")),
    ];
    let matching_pdf = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-v1")),
        cap("footnotes", CapabilityState::NotRepresentable, true, None),
    ];

    assert_eq!(
        assess_authority_migration(&simple_docx, &matching_pdf),
        AuthorityMigrationDecision::Eligible
    );

    let docx_with_footnotes = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-v1")),
        cap("footnotes", CapabilityState::Present, true, Some("footnote-v1")),
    ];
    assert_eq!(
        assess_authority_migration(&docx_with_footnotes, &matching_pdf),
        AuthorityMigrationDecision::Denied {
            missing: vec!["footnotes".to_owned()],
        }
    );
}

#[test]
fn mismatched_equivalence_fingerprint_denies_authority_migration() {
    let source = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-source")),
    ];
    let target = vec![
        cap("reader_content", CapabilityState::Present, true, Some("reader-target")),
    ];

    assert_eq!(
        assess_authority_migration(&source, &target),
        AuthorityMigrationDecision::Denied {
            missing: vec!["reader_content".to_owned()],
        }
    );
}

#[test]
fn non_version_significant_capabilities_do_not_block_migration() {
    let source = vec![
        cap("reader_content", CapabilityState::Present, true, Some("same")),
        cap("comments", CapabilityState::Present, false, Some("comment-a")),
    ];
    let target = vec![
        cap("reader_content", CapabilityState::Present, true, Some("same")),
        cap("comments", CapabilityState::Absent, false, None),
    ];

    assert_eq!(
        assess_authority_migration(&source, &target),
        AuthorityMigrationDecision::Eligible
    );
}
