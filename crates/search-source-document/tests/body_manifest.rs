//! P1-B01 canonical body manifest, coverage artifact and bundle receipt.
use std::sync::Arc;

#[path = "support/body.rs"]
mod body_support;

use body_support::*;
use document_domain::{FileId, StorageKey};
use search_core::id::ProjectionGenerationId;
use search_core::knowledge_unit::{ContentPartRef, RawBinding};
use search_core::projection::ProjectionGenerationKey;
use search_extraction_core::{
    BodyCoverage, CoverageReason, ItemOperationState, PermanentFailureCode, RetryableFailureCode,
};
use search_source_document::{
    ArtifactReceipt, AuthoritativeItemBinding, BodyBuildError, BodyItemEntry, BodyUnitManifest,
    DocumentBodyExtractor, DocumentOutboxSnapshot, compute_bundle_receipt, coverage_receipt,
    segment_digest, unit_manifest_receipt, unit_manifest_receipt_from_segments, validate_manifest,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn binding(
    raw: &[u8],
    media_type: &str,
    ordinal: u32,
    path: &str,
    seed: u128,
) -> AuthoritativeItemBinding {
    let content_item_id = Uuid::from_u128(seed);
    AuthoritativeItemBinding {
        content_item_id,
        part: ContentPartRef {
            source_native_part_id: content_item_id.to_string(),
            logical_path: path.into(),
            ordinal,
        },
        representation_id: Uuid::from_u128(seed + 1),
        file_id: FileId::from_uuid(Uuid::from_u128(seed + 2)),
        raw: RawBinding {
            sha256: Sha256::digest(raw).into(),
            size_bytes: raw.len() as u64,
            media_type: media_type.into(),
        },
        storage_key: StorageKey::new(KEY).unwrap(),
    }
}

fn key(generation: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
    }
}

/// Two Live parts extracted through the real readers and host verification.
async fn fixture(first_text: &str) -> (DocumentOutboxSnapshot, BodyUnitManifest) {
    let items = [
        (
            format!("{first_text}\n").into_bytes(),
            "text/plain",
            0,
            "本文/a",
            100,
        ),
        (
            "同文。,x\n".as_bytes().to_vec(),
            "text/csv",
            1,
            "本文/b",
            200,
        ),
    ];
    let mut snapshot_record = record();
    let mut entries = Vec::new();
    for (raw, media, ordinal, path, seed) in items {
        let item = binding(&raw, media, ordinal, path, seed);
        snapshot_record.authoritative_items.push(item.clone());
        let body = DocumentBodyExtractor::new(
            source_id(),
            SequencedStorage::with(vec![raw.clone()]),
            InProcessExtractor::new(Mode::Honest),
            registry(),
        );
        let result = body.extract_item(&snapshot_record, &item).await.unwrap();
        entries.push(BodyItemEntry::from_extracted(
            source_id(),
            &snapshot_record,
            &item,
            body.registry().parser_build_id(),
            result,
        ));
    }
    let snapshot = DocumentOutboxSnapshot {
        source_snapshot: snapshot_record.snapshot.source_snapshot.clone(),
        live: vec![snapshot_record],
        historical: Vec::new(),
    };
    let manifest = BodyUnitManifest {
        key: key(7),
        source_snapshot: snapshot.source_snapshot.clone(),
        entries,
    };
    (snapshot, manifest)
}

fn other_receipt(key: ProjectionGenerationKey, byte: u8) -> ArtifactReceipt {
    ArtifactReceipt {
        key,
        digest: [byte; 32],
        count: 1,
    }
}

type EntryMutation = Box<dyn Fn(&mut BodyItemEntry)>;

const PROJECTION: &str = "sha256:0101010101010101010101010101010101010101010101010101010101010101";

#[tokio::test]
async fn manifest_covers_every_live_item_once_in_canonical_order() {
    let (snapshot, manifest) = fixture("東京").await;
    let artifact = validate_manifest(&manifest, &snapshot).unwrap();
    assert_eq!(
        artifact
            .items
            .iter()
            .map(|item| (item.part.ordinal, item.unit_count))
            .collect::<Vec<_>>(),
        vec![(0, 1), (1, 2)]
    );

    let mut missing = manifest.clone();
    missing.entries.pop();
    assert_eq!(
        validate_manifest(&missing, &snapshot),
        Err(BodyBuildError::Integrity("manifest item set"))
    );
    let mut reordered = manifest.clone();
    reordered.entries.swap(0, 1);
    assert_eq!(
        validate_manifest(&reordered, &snapshot),
        Err(BodyBuildError::Integrity("manifest order"))
    );
    let mut wrong_raw = manifest.clone();
    wrong_raw.entries[0].raw.size_bytes += 1;
    assert_eq!(
        validate_manifest(&wrong_raw, &snapshot),
        Err(BodyBuildError::Integrity("manifest item binding"))
    );
    let mut stale = manifest.clone();
    stale.source_snapshot = "older-snapshot".into();
    assert_eq!(
        validate_manifest(&stale, &snapshot),
        Err(BodyBuildError::Integrity("manifest source snapshot"))
    );
}

#[tokio::test]
async fn only_publication_safe_states_with_matching_witnesses_validate() {
    let (snapshot, manifest) = fixture("東京").await;
    let cases: Vec<(&str, EntryMutation)> = vec![
        (
            "retryable item is not publishable",
            Box::new(|entry| {
                entry.operation = ItemOperationState::Retryable {
                    code: RetryableFailureCode::Timeout,
                };
                entry.coverage = None;
                entry.units.clear();
            }),
        ),
        (
            "failed item output",
            Box::new(|entry| {
                entry.operation = ItemOperationState::FailedPermanent {
                    code: PermanentFailureCode::CorruptDocument,
                };
                entry.coverage = None;
            }),
        ),
        (
            "unsupported item units",
            Box::new(|entry| {
                entry.coverage = Some(BodyCoverage::Unsupported {
                    reason: CoverageReason::Encrypted,
                });
            }),
        ),
        (
            "partial witness",
            Box::new(|entry| {
                entry.coverage = Some(BodyCoverage::Partial {
                    reasons: vec![CoverageReason::UnsupportedStructure],
                });
                entry.units.clear();
            }),
        ),
        (
            "unit authority",
            Box::new(|entry| entry.units[0].text.push('!')),
        ),
    ];
    for (reason, mutate) in cases {
        let mut changed = manifest.clone();
        mutate(&mut changed.entries[0]);
        assert_eq!(
            validate_manifest(&changed, &snapshot),
            Err(BodyBuildError::Integrity(reason)),
            "{reason}"
        );
    }
    // A permanent failure with no output is a valid published outcome.
    let mut failed = manifest.clone();
    failed.entries[0].operation = ItemOperationState::FailedPermanent {
        code: PermanentFailureCode::CorruptDocument,
    };
    failed.entries[0].coverage = None;
    failed.entries[0].units.clear();
    assert!(validate_manifest(&failed, &snapshot).is_ok());
}

#[tokio::test]
async fn body_only_change_moves_composite_digest_but_not_projection_digest() {
    let (snapshot_a, manifest_a) = fixture("東京").await;
    let (snapshot_b, manifest_b) = fixture("大阪").await;
    let coverage_a = validate_manifest(&manifest_a, &snapshot_a).unwrap();
    let coverage_b = validate_manifest(&manifest_b, &snapshot_b).unwrap();
    let lexical = other_receipt(key(7), 5);
    let graph = other_receipt(key(7), 6);
    let receipt_a = compute_bundle_receipt(
        key(7),
        &manifest_a.source_snapshot,
        PROJECTION,
        &manifest_a,
        &coverage_a,
        lexical,
        graph,
    )
    .unwrap();
    let receipt_b = compute_bundle_receipt(
        key(7),
        &manifest_b.source_snapshot,
        PROJECTION,
        &manifest_b,
        &coverage_b,
        lexical,
        graph,
    )
    .unwrap();
    assert_eq!(receipt_a.projection_digest, receipt_b.projection_digest);
    assert_ne!(
        receipt_a.unit_manifest.digest,
        receipt_b.unit_manifest.digest
    );
    assert_ne!(receipt_a.composite_digest, receipt_b.composite_digest);
    assert_eq!(receipt_a.unit_manifest.count, 3);
    assert_eq!(receipt_a.body_coverage.count, 2);
    assert_eq!(receipt_a.lexical_schema_version, "schema-2");
}

#[tokio::test]
async fn rebuild_equivalence_ignores_generation_and_snapshot_identity() {
    let (_, manifest) = fixture("東京").await;
    let mut rebuilt = manifest.clone();
    rebuilt.key = key(8);
    rebuilt.source_snapshot = "document-snapshot-rebuilt".into();
    for entry in &mut rebuilt.entries {
        for unit in &mut entry.units {
            Arc::make_mut(&mut unit.provenance).source_snapshot =
                "document-snapshot-rebuilt".into();
        }
    }
    let first = unit_manifest_receipt(&manifest).unwrap();
    let second = unit_manifest_receipt(&rebuilt).unwrap();
    assert_eq!(first.digest, second.digest);
    assert_ne!(first.key, second.key);
}

#[tokio::test]
async fn segment_digests_reproduce_the_manifest_receipt_and_isolate_one_item() {
    let (_, manifest) = fixture("東京").await;
    let segments: Vec<([u8; 32], u64)> = manifest
        .entries
        .iter()
        .map(|entry| (segment_digest(entry).unwrap(), entry.units.len() as u64))
        .collect();
    assert_eq!(
        unit_manifest_receipt(&manifest).unwrap(),
        unit_manifest_receipt_from_segments(manifest.key, &segments).unwrap()
    );

    let mut changed = manifest.clone();
    let unit = &mut changed.entries[0].units[0];
    unit.text.push('。');
    unit.text_sha256 = Sha256::digest(unit.text.as_bytes()).into();
    assert_ne!(segment_digest(&changed.entries[0]).unwrap(), segments[0].0);
    for (entry, (digest, _)) in changed.entries.iter().zip(&segments).skip(1) {
        assert_eq!(&segment_digest(entry).unwrap(), digest);
    }
    assert_ne!(
        unit_manifest_receipt(&changed).unwrap().digest,
        unit_manifest_receipt(&manifest).unwrap().digest
    );

    let mut rebuilt = manifest.clone();
    rebuilt.entries[1].parser_build_id = "sha256:other-build".into();
    assert_ne!(segment_digest(&rebuilt.entries[1]).unwrap(), segments[1].0);
}

#[tokio::test]
async fn mismatched_artifact_key_or_unvalidated_coverage_is_rejected() {
    let (snapshot, manifest) = fixture("東京").await;
    let coverage = validate_manifest(&manifest, &snapshot).unwrap();
    assert_eq!(
        compute_bundle_receipt(
            key(7),
            &manifest.source_snapshot,
            PROJECTION,
            &manifest,
            &coverage,
            other_receipt(key(9), 5),
            other_receipt(key(7), 6),
        ),
        Err(BodyBuildError::Integrity("bundle key"))
    );
    let mut short = coverage.clone();
    short.items.pop();
    assert_eq!(
        compute_bundle_receipt(
            key(7),
            &manifest.source_snapshot,
            PROJECTION,
            &manifest,
            &short,
            other_receipt(key(7), 5),
            other_receipt(key(7), 6),
        ),
        Err(BodyBuildError::Integrity("coverage item count"))
    );
    assert_eq!(coverage_receipt(&coverage).unwrap().count, 2);
    assert_eq!(
        compute_bundle_receipt(
            key(7),
            &manifest.source_snapshot,
            "sha256:not-hex",
            &manifest,
            &coverage,
            other_receipt(key(7), 5),
            other_receipt(key(7), 6),
        ),
        Err(BodyBuildError::Integrity("projection digest"))
    );
}
