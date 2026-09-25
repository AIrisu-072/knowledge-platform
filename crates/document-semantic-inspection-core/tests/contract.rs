use std::collections::BTreeMap;

use document_semantic_inspection_core::{
    CapabilityEvidence, CapabilityState, CommentEvidence, CoreError, Diagnostic,
    DigitalSignatureEvidence, EditorialProvenance, ExternalDependency, ExtractorProvenance,
    FingerprintAlgorithm, FormatId, InspectionProfileVersion, NativeDependencyIdentity,
    ParserLibraryIdentity, SemanticFingerprint, SignatureValidity, TraceContext,
    TrackedChangeEvidence, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
    canonical_worker_response_bytes, decode_worker_response_bounded,
};
use serde_json::json;

fn fp(byte: u8) -> SemanticFingerprint {
    SemanticFingerprint::sha256_from_slice(&[byte; 32]).expect("32-byte digest")
}

fn empty_response() -> WorkerResponse {
    WorkerResponse {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        observed_raw_content_hash: [7; 32],
        observed_size_bytes: 3,
        detected_format: FormatId::Txt,
        semantic_fingerprint: fp(1),
        semantic_capabilities: Vec::new(),
        editorial_provenance: EditorialProvenance::default(),
        external_dependencies: Vec::new(),
        digital_signature_evidence: Vec::new(),
        extractor_provenance: ExtractorProvenance {
            worker_build_id: "build-1".into(),
            adapter_id: "txt".into(),
            adapter_version: "1".into(),
            parser_libraries: Vec::new(),
            native_dependency_identity: Vec::new(),
        },
        diagnostics: Vec::new(),
    }
}

#[test]
fn dsi_v0_profile_and_required_format_set_are_frozen() {
    assert_eq!(
        serde_json::to_string(&InspectionProfileVersion::DsiV0).unwrap(),
        r#""dsi-v0""#
    );
    assert!(serde_json::from_str::<InspectionProfileVersion>(r#""dsi-v1""#).is_err());

    assert_eq!(
        FormatId::REQUIRED_V0,
        [
            FormatId::Docx,
            FormatId::Xlsx,
            FormatId::Xlsm,
            FormatId::Pptx,
            FormatId::Pdf,
            FormatId::Txt,
            FormatId::Csv,
            FormatId::Html,
        ]
    );
}

#[test]
fn semantic_fingerprint_is_exactly_sha256_32_bytes() {
    let fingerprint = fp(0x5a);
    assert_eq!(fingerprint.algorithm(), FingerprintAlgorithm::Sha256);
    assert_eq!(fingerprint.digest(), &[0x5a; 32]);
    assert!(SemanticFingerprint::sha256_from_slice(&[0_u8; 31]).is_err());
    assert!(SemanticFingerprint::sha256_from_slice(&[0_u8; 33]).is_err());
}

#[test]
fn capability_and_evidence_wire_shape_is_stable() {
    let states = [
        (CapabilityState::Present, "present"),
        (CapabilityState::Absent, "absent"),
        (CapabilityState::NotRepresentable, "not_representable"),
        (CapabilityState::NotVerifiable, "not_verifiable"),
    ];
    for (state, expected) in states {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
    }

    let mut response = empty_response();
    response.semantic_capabilities.push(CapabilityEvidence {
        capability_id: "reader_content".into(),
        presence: CapabilityState::Present,
        version_significant: true,
        equivalence_fingerprint: Some(fp(2)),
    });
    response.editorial_provenance = EditorialProvenance {
        tracked_changes: vec![TrackedChangeEvidence {
            kind: "insertion".into(),
            author_label: Some("Author A".into()),
            timestamp: Some("2026-09-25T00:00:00Z".into()),
            source_locator: "word/document.xml#p1".into(),
            unresolved: true,
        }],
        comments: vec![CommentEvidence {
            author_label: Some("Reviewer B".into()),
            timestamp: None,
            resolved_state: "unresolved".into(),
            source_locator: "word/comments.xml#1".into(),
            content: "review note".into(),
        }],
        document_author_labels: vec!["Author A".into()],
        last_modified_by: Some("Author A".into()),
        modification_metadata: BTreeMap::from([("revision".into(), "7".into())]),
    };
    response.external_dependencies.push(ExternalDependency {
        dependency_kind: "external_workbook".into(),
        normalized_reference: "book.xlsx".into(),
        source_locator: "xl/externalLinks/externalLink1.xml".into(),
        version_significant: true,
    });
    response
        .digital_signature_evidence
        .push(DigitalSignatureEvidence {
            signature_type: "cms".into(),
            signer_claim: Some("Signer".into()),
            certificate_subject: Some("CN=Signer".into()),
            certificate_issuer: Some("CN=Issuer".into()),
            certificate_fingerprint: Some("aa".repeat(32)),
            signed_at: None,
            cryptographic_validity: SignatureValidity::Unverifiable,
            covered_content: vec!["bytes:0-99".into()],
            validation_diagnostics: vec!["offline-trust".into()],
        });
    response
        .extractor_provenance
        .parser_libraries
        .push(ParserLibraryIdentity {
            name: "encoding_rs".into(),
            version: "0.8.41".into(),
        });
    response
        .extractor_provenance
        .native_dependency_identity
        .push(NativeDependencyIdentity {
            name: "pdfium".into(),
            version: Some("151.0.7881.0".into()),
            sha256: Some([3; 32]),
        });
    response.diagnostics.push(Diagnostic {
        code: "hidden_content".into(),
        message: "hidden content present".into(),
    });

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        value["semantic_capabilities"][0]["capability_id"],
        "reader_content"
    );
    assert_eq!(
        value["editorial_provenance"]["tracked_changes"][0]["unresolved"],
        true
    );
    assert_eq!(
        value["external_dependencies"][0]["version_significant"],
        true
    );
    assert_eq!(
        value["digital_signature_evidence"][0]["cryptographic_validity"],
        "unverifiable"
    );
    assert_eq!(
        value["extractor_provenance"]["parser_libraries"][0]["name"],
        "encoding_rs"
    );
}

#[test]
fn worker_request_contains_only_worker_safe_identity_fields() {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: "text/plain".into(),
        expected_raw_content_hash: [9; 32],
        expected_size_bytes: 3,
        trace_context: Some(TraceContext {
            traceparent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
            tracestate: None,
        }),
    };
    let json = serde_json::to_string(&request).unwrap();

    for forbidden in [
        "file_id",
        "document_id",
        "document_version_id",
        "principal",
        "storage_key",
        "database_url",
        "storage_credential",
    ] {
        assert!(
            !json.contains(forbidden),
            "worker request leaked {forbidden}: {json}"
        );
    }
}

#[test]
fn canonical_response_normalizes_semantically_unordered_collections() {
    let mut left = empty_response();
    left.semantic_capabilities = vec![
        CapabilityEvidence {
            capability_id: "zeta".into(),
            presence: CapabilityState::Absent,
            version_significant: true,
            equivalence_fingerprint: None,
        },
        CapabilityEvidence {
            capability_id: "alpha".into(),
            presence: CapabilityState::Present,
            version_significant: true,
            equivalence_fingerprint: Some(fp(4)),
        },
    ];
    left.external_dependencies = vec![
        ExternalDependency {
            dependency_kind: "url".into(),
            normalized_reference: "https://b.invalid".into(),
            source_locator: "b".into(),
            version_significant: true,
        },
        ExternalDependency {
            dependency_kind: "url".into(),
            normalized_reference: "https://a.invalid".into(),
            source_locator: "a".into(),
            version_significant: true,
        },
    ];
    left.diagnostics = vec![
        Diagnostic {
            code: "z".into(),
            message: "z".into(),
        },
        Diagnostic {
            code: "a".into(),
            message: "a".into(),
        },
    ];

    let mut right = left.clone();
    right.semantic_capabilities.reverse();
    right.external_dependencies.reverse();
    right.diagnostics.reverse();

    assert_eq!(
        canonical_worker_response_bytes(&left).unwrap(),
        canonical_worker_response_bytes(&right).unwrap()
    );
}

#[test]
fn worker_protocol_rejects_unknown_versions_and_oversized_results() {
    let valid = canonical_worker_response_bytes(&empty_response()).unwrap();

    let mut value: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    value["protocol_version"] = json!("dsi-worker-v999");
    let unknown = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        decode_worker_response_bounded(&unknown, unknown.len() + 1),
        Err(CoreError::UnknownProtocolVersion(version)) if version == "dsi-worker-v999"
    ));

    assert!(matches!(
        decode_worker_response_bounded(&valid, valid.len() - 1),
        Err(CoreError::ResultTooLarge { .. })
    ));

    let decoded = decode_worker_response_bounded(&valid, valid.len()).unwrap();
    assert_eq!(decoded, empty_response());
}

#[test]
fn worker_response_matches_versioned_golden_snapshot() {
    let actual = canonical_worker_response_bytes(&empty_response()).unwrap();
    let expected = include_str!("golden/worker-response-v0.json")
        .trim()
        .as_bytes();
    assert_eq!(actual, expected);
}
