//! P1-S02 host-side extraction: raw binding before/after the worker, trusted
//! profile registry, native locator round trip and Source-owned Unit identity.

#[path = "support/body.rs"]
mod body_support;

use body_support::*;
use search_core::knowledge_unit::{BudgetKey, FormatId, NativeLocator, UnitId, UnitKind};
use search_extraction_core::{
    BodyCoverage, CoverageReason, ItemOperationState, PermanentFailureCode, RetryableFailureCode,
};
use search_source_document::{BodyBuildError, BodyProfileRegistry};
use uuid::Uuid;

#[tokio::test]
async fn text_item_builds_verified_units_without_source_identity_in_worker() {
    let raw = "東京\n同文。\n".as_bytes().to_vec();
    let binding = item(&raw, "text/plain");
    let body = extractor(vec![raw.clone()], Mode::Honest);
    let result = body.extract_item(&record(), &binding).await.unwrap();
    assert_eq!(result.operation, ItemOperationState::Completed);
    assert_eq!(result.coverage, Some(BodyCoverage::Supported));
    assert_eq!(result.detected_format, Some(FormatId::Text));
    let texts: Vec<_> = result.units.iter().map(|unit| unit.text.as_str()).collect();
    assert_eq!(texts, vec!["東京", "同文。"]);
    let unit = &result.units[1];
    assert_eq!(unit.kind, UnitKind::PlainText);
    assert_eq!(
        unit.version.resource_id,
        search_core::id::ResourceId::from_uuid(Uuid::from_u128(20))
    );
    assert_eq!(
        unit.unit_id,
        UnitId::derive(
            &unit.version,
            &unit.part,
            result.profile.as_ref().unwrap(),
            &NativeLocator::Text {
                line_start: 1,
                line_end: 2
            },
            1
        )
        .unwrap()
    );
    assert_eq!(
        unit.provenance.authoritative_representation_ref,
        Uuid::from_u128(41).to_string()
    );
    // Two worker runs (extract + resolve); requests carry raw binding, profile and
    // budgets only — never the storage key or Source identity.
    let requests = body_requests(&body);
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request.expected_raw, binding.raw);
        let wire = search_extraction_core::encode_request(&request).unwrap();
        assert!(
            !wire
                .windows(KEY.len())
                .any(|window| window == KEY.as_bytes())
        );
        let source = source_id().as_uuid().to_string();
        assert!(
            !wire
                .windows(source.len())
                .any(|window| window == source.as_bytes())
        );
    }
}

#[tokio::test]
async fn raw_mismatch_before_or_after_worker_is_an_integrity_incident() {
    let raw = b"original\n".to_vec();
    let binding = item(&raw, "text/plain");
    // Wrong bytes on the first read: the worker never runs.
    let before = extractor(vec![b"tampered\n".to_vec()], Mode::Honest);
    assert_eq!(
        before.extract_item(&record(), &binding).await,
        Err(BodyBuildError::Integrity("raw binding"))
    );
    assert_eq!(before.extractor().calls(), 0);
    // Raw swapped while the worker ran.
    let after = extractor(vec![raw.clone(), b"swapped!\n".to_vec()], Mode::Honest);
    assert_eq!(
        after.extract_item(&record(), &binding).await,
        Err(BodyBuildError::Integrity("raw binding"))
    );
}

#[tokio::test]
async fn tampered_locator_resolution_is_rejected() {
    let raw = b"alpha\nbeta\n".to_vec();
    let binding = item(&raw, "text/plain");
    let body = extractor(vec![raw], Mode::TamperResolve);
    assert_eq!(
        body.extract_item(&record(), &binding).await,
        Err(BodyBuildError::Integrity("native locator round trip"))
    );
}

#[tokio::test]
async fn permanent_and_retryable_worker_outcomes_keep_their_class() {
    let raw = b"alpha\n".to_vec();
    let binding = item(&raw, "text/plain");
    let failed = extractor(vec![raw.clone()], Mode::Permanent)
        .extract_item(&record(), &binding)
        .await
        .unwrap();
    assert_eq!(
        failed.operation,
        ItemOperationState::FailedPermanent {
            code: PermanentFailureCode::CorruptDocument
        }
    );
    assert_eq!((failed.coverage, failed.units.len()), (None, 0));
    assert_eq!(
        extractor(vec![raw], Mode::Retryable)
            .extract_item(&record(), &binding)
            .await,
        Err(BodyBuildError::Retryable(RetryableFailureCode::Timeout))
    );
}

#[tokio::test]
async fn format_spoof_and_unknown_media_are_unsupported_without_worker() {
    let raw = b"plain text pretending to be a package".to_vec();
    for media in [
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/msword",
        "application/x-unknown",
    ] {
        let body = extractor(vec![raw.clone()], Mode::Honest);
        let result = body
            .extract_item(&record(), &item(&raw, media))
            .await
            .unwrap();
        assert_eq!(
            result.coverage,
            Some(BodyCoverage::Unsupported {
                reason: CoverageReason::UnsupportedFormat
            }),
            "{media}"
        );
        assert!(result.units.is_empty());
        assert_eq!(body.extractor().calls(), 0);
    }
}

#[tokio::test]
async fn zip_plan_comes_from_the_trusted_registry() {
    let raw = zip(&[
        ("a.txt", "東京\n".as_bytes()),
        ("b.csv", "同文。\n".as_bytes()),
    ]);
    let binding = item(&raw, "application/zip");
    let result = extractor(vec![raw.clone()], Mode::Honest)
        .extract_item(&record(), &binding)
        .await
        .unwrap();
    assert_eq!(result.coverage, Some(BodyCoverage::Supported));
    assert_eq!(
        result
            .units
            .iter()
            .map(|unit| (unit.text.as_str(), unit.provenance.archive_inner_format))
            .collect::<Vec<_>>(),
        vec![
            ("東京", Some(FormatId::Text)),
            ("同文。", Some(FormatId::Csv))
        ]
    );
    // Nested containers are never planned by the host.
    let nested = zip(&[("inner.zip", &zip(&[("a.txt", b"x\n")]))]);
    let result = extractor(vec![nested.clone()], Mode::Honest)
        .extract_item(&record(), &item(&nested, "application/zip"))
        .await
        .unwrap();
    assert_eq!(
        result.coverage,
        Some(BodyCoverage::Unsupported {
            reason: CoverageReason::UnsupportedStructure
        })
    );
}

#[tokio::test]
async fn zip_directory_entries_are_not_leaves() {
    let raw = zip(&[
        ("docs/", b""),
        ("docs/a.txt", "東京\n".as_bytes()),
        ("docs/sub/", b""),
        ("docs/sub/b.csv", "同文。\n".as_bytes()),
    ]);
    let result = extractor(vec![raw.clone()], Mode::Honest)
        .extract_item(&record(), &item(&raw, "application/zip"))
        .await
        .unwrap();
    assert_eq!(result.coverage, Some(BodyCoverage::Supported));
    assert_eq!(
        result
            .units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<Vec<_>>(),
        vec!["東京", "同文。"]
    );
    // A directory entry with an unsafe path still refuses the archive.
    let unsafe_directory = zip(&[("../", b""), ("a.txt", b"x\n")]);
    let result = extractor(vec![unsafe_directory.clone()], Mode::Honest)
        .extract_item(&record(), &item(&unsafe_directory, "application/zip"))
        .await
        .unwrap();
    assert_eq!(
        result.coverage,
        Some(BodyCoverage::Unsupported {
            reason: CoverageReason::UnsupportedStructure
        })
    );
}

#[tokio::test]
async fn oversized_declared_item_is_resource_limited_before_any_read() {
    // No stored image: any read would fail as unavailable.
    let body = extractor(vec![], Mode::Honest);
    for (raw, media_type) in [
        (&b"small"[..], "text/plain"),
        (&b"PK"[..], "application/zip"),
    ] {
        let mut binding = item(raw, media_type);
        binding.raw.size_bytes = u64::MAX / 2;
        let result = body.extract_item(&record(), &binding).await.unwrap();
        assert_eq!(result.operation, ItemOperationState::Completed);
        assert_eq!(
            result.coverage,
            Some(BodyCoverage::Unsupported {
                reason: CoverageReason::ResourceLimit
            })
        );
        assert!(result.units.is_empty());
    }
    assert!(body_requests(&body).is_empty());
}

#[tokio::test]
async fn unknown_media_type_is_unsupported_before_any_read() {
    // No stored image: any read would fail as unavailable.
    let body = extractor(vec![], Mode::Honest);
    let mut binding = item(b"\x00\x00\x00\x18ftypmp42", "video/mp4");
    binding.raw.size_bytes = u64::MAX / 2;
    let result = body.extract_item(&record(), &binding).await.unwrap();
    assert_eq!(result.operation, ItemOperationState::Completed);
    assert_eq!(
        result.coverage,
        Some(BodyCoverage::Unsupported {
            reason: CoverageReason::UnsupportedFormat
        })
    );
    assert!(body_requests(&body).is_empty());
}

#[test]
fn zip_definition_without_host_budgets_is_rejected_at_startup() {
    for key in [BudgetKey::InputBytes, BudgetKey::ZipEntries] {
        let mut zip = definition(FormatId::Zip);
        zip.limits.remove(&key);
        assert!(matches!(
            BodyProfileRegistry::new("build-1", vec![zip]),
            Err(BodyBuildError::Configuration(_))
        ));
    }
}
