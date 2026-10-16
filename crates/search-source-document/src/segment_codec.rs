//! `v2` storage of one Unit segment (SD-T11 5).
//!
//! Every Unit of a Part repeats the Part's version, Part reference and
//! provenance, and a Unit's text digest follows from its text: at 10,000
//! documents these took about two thirds of the stored segments. A segment
//! therefore stores each distinct (version, Part, provenance) context once,
//! each Unit by the index of its context, and no text digest; decoding
//! recomputes the digests and shares one allocation per context. The segment
//! digest is computed from the decoded entry and does not depend on this
//! encoding.

use std::sync::Arc;

use search_core::knowledge_unit::{
    ArchiveProfilePlan, ContentPartRef, ExtractionProfileId, FormatId, KnowledgeUnit,
    NativeLocator, RawBinding, ResourceVersionRef, UnitId, UnitKind, UnitProvenance, text_sha256,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use serde::{Deserialize, Serialize};

use crate::body_manifest::BodyItemEntry;
use crate::extraction::BodyBuildError;

/// Version tag of a stored Unit segment.
pub const SEGMENT_DTO_VERSION: &str = "v2";

#[derive(Serialize)]
struct EnvelopeRef<'a> {
    dto_version: &'static str,
    body: SegmentRef<'a>,
}

#[derive(Serialize)]
struct SegmentRef<'a> {
    version: &'a ResourceVersionRef,
    part: &'a ContentPartRef,
    authoritative_representation_ref: &'a str,
    raw: &'a RawBinding,
    detected_format: &'a Option<FormatId>,
    profile: &'a Option<ExtractionProfileId>,
    parser_build_id: &'a str,
    archive_plan: &'a Option<ArchiveProfilePlan>,
    operation: &'a ItemOperationState,
    coverage: &'a Option<BodyCoverage>,
    contexts: Vec<ContextRef<'a>>,
    units: Vec<UnitRef<'a>>,
}

#[derive(Serialize)]
struct ContextRef<'a> {
    version: &'a ResourceVersionRef,
    part: &'a ContentPartRef,
    provenance: &'a UnitProvenance,
}

#[derive(Serialize)]
struct UnitRef<'a> {
    unit_id: &'a UnitId,
    context: u32,
    parent_unit_id: &'a Option<UnitId>,
    ordinal: u32,
    kind: &'a UnitKind,
    text: &'a str,
    locator: &'a NativeLocator,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    dto_version: String,
    body: Segment,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    version: ResourceVersionRef,
    part: ContentPartRef,
    authoritative_representation_ref: String,
    raw: RawBinding,
    detected_format: Option<FormatId>,
    profile: Option<ExtractionProfileId>,
    parser_build_id: String,
    archive_plan: Option<ArchiveProfilePlan>,
    operation: ItemOperationState,
    coverage: Option<BodyCoverage>,
    contexts: Vec<Context>,
    units: Vec<Unit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    version: ResourceVersionRef,
    part: ContentPartRef,
    provenance: UnitProvenance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unit {
    unit_id: UnitId,
    context: u32,
    parent_unit_id: Option<UnitId>,
    ordinal: u32,
    kind: UnitKind,
    text: String,
    locator: NativeLocator,
}

fn failed(message: &'static str) -> BodyBuildError {
    BodyBuildError::Integrity(message)
}

/// The stored text of `entry` as a `v2` segment.
pub fn encode_segment(entry: &BodyItemEntry) -> Result<String, BodyBuildError> {
    if entry.stored.is_some() {
        return Err(failed("segment of a stored item"));
    }
    let mut contexts: Vec<ContextRef<'_>> = Vec::new();
    let mut units = Vec::with_capacity(entry.units.len());
    for unit in &entry.units {
        let found = contexts.iter().position(|context| {
            *context.version == *unit.version
                && *context.part == *unit.part
                && *context.provenance == *unit.provenance
        });
        let context = match found {
            Some(index) => index,
            None => {
                contexts.push(ContextRef {
                    version: &unit.version,
                    part: &unit.part,
                    provenance: &unit.provenance,
                });
                contexts.len() - 1
            }
        };
        units.push(UnitRef {
            unit_id: &unit.unit_id,
            context: u32::try_from(context).map_err(|_| failed("segment context count"))?,
            parent_unit_id: &unit.parent_unit_id,
            ordinal: unit.ordinal,
            kind: &unit.kind,
            text: &unit.text,
            locator: &unit.locator,
        });
    }
    serde_json::to_string(&EnvelopeRef {
        dto_version: SEGMENT_DTO_VERSION,
        body: SegmentRef {
            version: &entry.version,
            part: &entry.part,
            authoritative_representation_ref: &entry.authoritative_representation_ref,
            raw: &entry.raw,
            detected_format: &entry.detected_format,
            profile: &entry.profile,
            parser_build_id: &entry.parser_build_id,
            archive_plan: &entry.archive_plan,
            operation: &entry.operation,
            coverage: &entry.coverage,
            contexts,
            units,
        },
    })
    .map_err(|_| failed("segment encoding"))
}

/// The entry stored as `text`; each Unit's text digest is recomputed and the
/// Units of one context share its allocations.
pub fn decode_segment(text: &str) -> Result<BodyItemEntry, BodyBuildError> {
    let envelope: Envelope = serde_json::from_str(text).map_err(|_| failed("segment decoding"))?;
    if envelope.dto_version != SEGMENT_DTO_VERSION {
        return Err(failed("segment version"));
    }
    let segment = envelope.body;
    let contexts: Vec<_> = segment
        .contexts
        .into_iter()
        .map(|context| {
            (
                Arc::new(context.version),
                Arc::new(context.part),
                Arc::new(context.provenance),
            )
        })
        .collect();
    let units = segment
        .units
        .into_iter()
        .map(|unit| {
            let index = usize::try_from(unit.context).map_err(|_| failed("segment context"))?;
            let (version, part, provenance) =
                contexts.get(index).ok_or(failed("segment context"))?;
            Ok(KnowledgeUnit {
                unit_id: unit.unit_id,
                version: version.clone(),
                part: part.clone(),
                parent_unit_id: unit.parent_unit_id,
                ordinal: unit.ordinal,
                kind: unit.kind,
                text_sha256: text_sha256(&unit.text),
                text: unit.text,
                locator: unit.locator,
                provenance: provenance.clone(),
            })
        })
        .collect::<Result<Vec<_>, BodyBuildError>>()?;
    Ok(BodyItemEntry {
        version: segment.version,
        part: segment.part,
        authoritative_representation_ref: segment.authoritative_representation_ref,
        raw: segment.raw,
        detected_format: segment.detected_format,
        profile: segment.profile,
        parser_build_id: segment.parser_build_id,
        archive_plan: segment.archive_plan,
        operation: segment.operation,
        coverage: segment.coverage,
        units,
        stored: None,
    })
}

#[cfg(test)]
mod tests {
    use search_core::id::{ResourceId, SourceId};
    use search_core::knowledge_unit::{
        ContentPartRef, ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator, RawBinding,
        ResourceVersionRef, UnitId, UnitKind, UnitProvenance, text_sha256,
    };
    use search_extraction_core::{BodyCoverage, ItemOperationState};
    use uuid::Uuid;

    use super::*;

    fn profile() -> ExtractionProfileId {
        ExtractionProfileId::parse(&format!("sha256:{}", "a".repeat(64))).unwrap()
    }

    fn version() -> ResourceVersionRef {
        ResourceVersionRef {
            source_id: SourceId::from_uuid(Uuid::from_u128(1)),
            resource_id: ResourceId::from_uuid(Uuid::from_u128(10)),
            source_native_version: "v1".into(),
        }
    }

    fn part() -> ContentPartRef {
        ContentPartRef {
            source_native_part_id: "part".into(),
            logical_path: "doc/primary".into(),
            ordinal: 0,
        }
    }

    fn provenance(inner: Option<FormatId>) -> UnitProvenance {
        UnitProvenance {
            source_snapshot: String::new(),
            authoritative_representation_ref: "representation".into(),
            raw: RawBinding {
                sha256: [1; 32],
                size_bytes: 8,
                media_type: "text/plain".into(),
            },
            detected_format: FormatId::Text,
            archive_inner_format: inner,
            profile: profile(),
            parser_build_id: "build".into(),
        }
    }

    fn unit(ordinal: u32, text: &str, inner: Option<FormatId>) -> KnowledgeUnit {
        let locator = NativeLocator::Text {
            line_start: ordinal,
            line_end: ordinal + 1,
        };
        KnowledgeUnit {
            unit_id: UnitId::derive(&version(), &part(), &profile(), &locator, ordinal).unwrap(),
            version: version().into(),
            part: part().into(),
            parent_unit_id: None,
            ordinal,
            kind: UnitKind::PlainText,
            text: text.into(),
            locator,
            text_sha256: text_sha256(text),
            provenance: provenance(inner).into(),
        }
    }

    fn entry(units: Vec<KnowledgeUnit>) -> BodyItemEntry {
        BodyItemEntry {
            version: version(),
            part: part(),
            authoritative_representation_ref: "representation".into(),
            raw: provenance(None).raw,
            detected_format: Some(FormatId::Text),
            profile: Some(profile()),
            parser_build_id: "build".into(),
            archive_plan: None,
            operation: ItemOperationState::Completed,
            coverage: Some(BodyCoverage::Supported),
            units,
            stored: None,
        }
    }

    #[test]
    fn a_segment_round_trips_with_each_context_stored_once() {
        let original = entry(vec![
            unit(0, "東京", None),
            unit(1, "同文。", None),
            unit(2, "大阪", Some(FormatId::Text)),
        ]);
        let text = encode_segment(&original).unwrap();
        let stored: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(stored["dto_version"], SEGMENT_DTO_VERSION);
        assert_eq!(stored["body"]["contexts"].as_array().unwrap().len(), 2);
        assert!(stored["body"]["units"][0].get("text_sha256").is_none());

        let decoded = decode_segment(&text).unwrap();
        assert_eq!(decoded, original);
        assert!(std::sync::Arc::ptr_eq(
            &decoded.units[0].provenance,
            &decoded.units[1].provenance
        ));
    }

    #[test]
    fn a_segment_with_another_version_or_a_missing_context_is_rejected() {
        let text = encode_segment(&entry(vec![unit(0, "東京", None)])).unwrap();
        let mut stored: serde_json::Value = serde_json::from_str(&text).unwrap();
        stored["dto_version"] = "v1".into();
        assert!(decode_segment(&stored.to_string()).is_err());

        let mut stored: serde_json::Value = serde_json::from_str(&text).unwrap();
        stored["body"]["units"][0]["context"] = 1.into();
        assert!(decode_segment(&stored.to_string()).is_err());
    }
}
