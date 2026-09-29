use std::io::Cursor;

use document_diff_core::{
    DiffCoverage, DiffProfileVersion, FormatId, ResourceProfileVersion, UnverifiedReason,
    WorkerDiffRequest, WorkerProtocolVersion,
};
use document_diff_worker::run_worker_shell;
use sha2::{Digest, Sha256};

fn request(base: &[u8], target: &[u8]) -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    }
}

#[test]
fn two_inputs_are_bound_independently_and_unsupported_format_is_unverified() {
    let base = b"old";
    let target = b"new";
    let request = request(base, target);
    let encoded = serde_json::to_vec(&request).unwrap();
    let response = run_worker_shell(&encoded, Cursor::new(base), Cursor::new(target)).unwrap();
    response.validate_against(&request).unwrap();
    assert_eq!(response.coverage, DiffCoverage::None);
    assert_eq!(
        response.unverified_regions[0].reason,
        UnverifiedReason::UnsupportedSemanticConstruct
    );
    assert!(response.unverified_regions[0].base.is_some());
    assert!(response.unverified_regions[0].target.is_some());
}

#[test]
fn wrong_size_or_hash_on_either_side_fails_closed() {
    let base = b"old";
    let target = b"new";
    let mut wrong_size = request(base, target);
    wrong_size.base_size_bytes += 1;
    assert!(
        run_worker_shell(
            &serde_json::to_vec(&wrong_size).unwrap(),
            Cursor::new(base),
            Cursor::new(target)
        )
        .is_err()
    );
    let mut wrong_hash = request(base, target);
    wrong_hash.target_raw_sha256 = [0; 32];
    assert!(
        run_worker_shell(
            &serde_json::to_vec(&wrong_hash).unwrap(),
            Cursor::new(base),
            Cursor::new(target)
        )
        .is_err()
    );
}

#[test]
fn oversized_or_unexpected_request_fields_are_rejected() {
    let request = request(b"a", b"b");
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["principal"] = serde_json::json!("synthetic-secret");
    assert!(
        run_worker_shell(
            &serde_json::to_vec(&encoded).unwrap(),
            Cursor::new(b"a"),
            Cursor::new(b"b")
        )
        .is_err()
    );
    assert!(
        run_worker_shell(
            &vec![b'x'; 64 * 1024 + 1],
            Cursor::new(b"a"),
            Cursor::new(b"b")
        )
        .is_err()
    );
}
