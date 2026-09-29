use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId,
    ResourceProfileVersion, SourceLocator, WorkerChange, WorkerDiffRequest, WorkerDiffResponse,
    WorkerProtocolVersion, decode_worker_response_bounded,
};
use serde_json::{Value, json};

fn response() -> WorkerDiffResponse {
    WorkerDiffResponse {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        base_raw_sha256: [1; 32],
        base_size_bytes: 8,
        target_raw_sha256: [2; 32],
        target_size_bytes: 10,
        format: FormatId::Txt,
        coverage: DiffCoverage::Full,
        changes: vec![WorkerChange {
            operation: Some(ChangeOperation::Modified),
            relocation: None,
            facet: "text".to_owned(),
            base: Some(SourceLocator::TextSpan {
                line: 1,
                byte_start: 0,
                byte_end: 4,
            }),
            target: Some(SourceLocator::TextSpan {
                line: 1,
                byte_start: 0,
                byte_end: 5,
            }),
            reason_code: "text.changed".to_owned(),
        }],
        unverified_regions: vec![],
        parser_provenance: "text-v0".to_owned(),
    }
}

#[test]
fn worker_request_has_no_authority_or_credentials() {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: [1; 32],
        base_size_bytes: 8,
        target_raw_sha256: [2; 32],
        target_size_bytes: 10,
    };
    let wire = serde_json::to_value(&request).unwrap();
    let object = wire.as_object().unwrap();
    for field in [
        "document_id",
        "document_version_id",
        "file_id",
        "principal",
        "storage_key",
        "db_credentials",
        "storage_credentials",
    ] {
        assert!(
            !object.contains_key(field),
            "worker request exposed {field}"
        );
    }
    let mut with_authority = wire;
    with_authority
        .as_object_mut()
        .unwrap()
        .insert("document_id".to_owned(), json!("secret"));
    assert!(serde_json::from_value::<WorkerDiffRequest>(with_authority).is_err());
}

#[test]
fn response_rejects_missing_side_and_invalid_locator() {
    let mut candidate = response();
    candidate.changes[0].operation = Some(ChangeOperation::Added);
    assert!(candidate.validate().is_err());
    candidate.changes[0].base = None;
    assert!(candidate.validate().is_ok());

    let mut candidate = response();
    candidate.changes[0].operation = Some(ChangeOperation::Removed);
    candidate.changes[0].target = None;
    assert!(candidate.validate().is_ok());
    candidate.changes[0].target = Some(SourceLocator::TextSpan {
        line: 0,
        byte_start: 0,
        byte_end: 0,
    });
    assert!(candidate.validate().is_err());

    let mut candidate = response();
    candidate.changes[0].base = Some(SourceLocator::TextSpan {
        line: 0,
        byte_start: 0,
        byte_end: 4,
    });
    assert!(candidate.validate().is_err());
}

#[test]
fn response_decoder_rejects_unknown_version_and_excess_bytes() {
    let encoded = serde_json::to_vec(&response()).unwrap();
    assert!(decode_worker_response_bounded(&encoded, encoded.len() - 1).is_err());
    assert_eq!(
        decode_worker_response_bounded(&encoded, encoded.len()).unwrap(),
        response()
    );

    let mut value: Value = serde_json::from_slice(&encoded).unwrap();
    value["protocol_version"] = json!("diff-worker-v999");
    let unknown = serde_json::to_vec(&value).unwrap();
    assert!(decode_worker_response_bounded(&unknown, unknown.len()).is_err());
}

#[test]
fn comparison_budget_rejects_one_over_limit() {
    let mut budget = ComparisonBudget::new(2, 1);
    assert!(budget.consume_candidates(2).is_ok());
    assert!(budget.consume_candidates(1).is_err());
    assert!(budget.consume_changes(1).is_ok());
    assert!(budget.consume_changes(1).is_err());
}

#[test]
fn response_binding_requires_exact_input_profile_and_raw_hash() {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: [1; 32],
        base_size_bytes: 8,
        target_raw_sha256: [2; 32],
        target_size_bytes: 10,
    };
    let response = response();
    assert!(response.validate_against(&request).is_ok());
    let mut wrong = response;
    wrong.target_raw_sha256 = [3; 32];
    assert!(wrong.validate_against(&request).is_err());
}
