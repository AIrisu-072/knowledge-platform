//! Shared synthetic Units for P7 runtime tests.
#![allow(dead_code)]

use std::collections::BTreeMap;

use search_application::search_core::id::{ResourceId, SourceId};
use search_application::search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    FormatSettings, KnowledgeUnit, NativeLocator, RawBinding, ResourceVersionRef, UnitId, UnitKind,
    UnitProvenance, text_sha256,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use search_source_document::BodyItemEntry;
use uuid::Uuid;

pub const SNAPSHOT: &str = "synthetic-snapshot-v1";

/// The parent Version Resource of every synthetic entry.
pub fn parent() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(7_402))
}

pub fn profile() -> ExtractionProfileId {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 16);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1024);
    limits.insert(BudgetKey::WorkerOutputBytes, 65_536);
    ExtractionProfileId::for_definition(&ExtractionProfileDefinitionV1 {
        format: FormatId::Text,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [1; 32],
        native_binary_sha256: None,
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: FormatSettings::Text {
            charset: "utf-8".into(),
        },
        limits,
    })
    .unwrap()
}

/// One Supported text item of one Version with the given line Units.
pub fn entry(source: SourceId, lines: &[&str]) -> BodyItemEntry {
    let version_id = Uuid::from_u128(7_402);
    let version = ResourceVersionRef {
        source_id: source,
        resource_id: ResourceId::from_uuid(version_id),
        source_native_version: version_id.to_string(),
    };
    let part = ContentPartRef {
        source_native_part_id: Uuid::from_u128(7_403).to_string(),
        logical_path: "本文/primary".into(),
        ordinal: 0,
    };
    let raw = RawBinding {
        sha256: [9; 32],
        size_bytes: 64,
        media_type: "text/plain".into(),
    };
    let units = lines
        .iter()
        .enumerate()
        .map(|(ordinal, text)| {
            let ordinal = u32::try_from(ordinal).unwrap();
            let locator = NativeLocator::Text {
                line_start: ordinal,
                line_end: ordinal + 1,
            };
            KnowledgeUnit {
                unit_id: UnitId::derive(&version, &part, &profile(), &locator, ordinal).unwrap(),
                version: version.clone(),
                part: part.clone(),
                parent_unit_id: None,
                ordinal,
                kind: UnitKind::PlainText,
                text: (*text).into(),
                locator,
                text_sha256: text_sha256(text),
                provenance: UnitProvenance {
                    source_snapshot: SNAPSHOT.into(),
                    authoritative_representation_ref: Uuid::from_u128(7_404).to_string(),
                    raw: raw.clone(),
                    detected_format: FormatId::Text,
                    archive_inner_format: None,
                    profile: profile(),
                    parser_build_id: "search-extraction-worker-test".into(),
                },
            }
        })
        .collect();
    BodyItemEntry {
        version,
        part,
        authoritative_representation_ref: Uuid::from_u128(7_404).to_string(),
        raw,
        detected_format: Some(FormatId::Text),
        profile: Some(profile()),
        parser_build_id: "search-extraction-worker-test".into(),
        archive_plan: None,
        operation: ItemOperationState::Completed,
        coverage: Some(BodyCoverage::Supported),
        units,
    }
}
