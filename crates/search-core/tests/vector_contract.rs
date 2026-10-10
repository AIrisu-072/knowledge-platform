use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::knowledge_unit::{
    ContentPartRef, DocxStep, ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator,
    RawBinding, ResourceVersionRef, UnitId, UnitKind, UnitProvenance, VectorAuthorityInput,
    text_sha256,
};
use search_core::projection::ProjectionGenerationKey;
use search_core::source::RetentionMode;
use search_core::vector::{
    BoundEmbedding, EmbeddingModelSpec, QueryEmbedding, RankedVectorHit, VectorActivationPolicy,
    VectorEntryRef, VectorIndexDescriptor, VectorManifestInput, VectorManifestUnit, VectorMetric,
    VectorNormalization, VectorPrecision, VectorProjectionManifest, VectorStorageKind,
    VectorUnitCoverage, check_segment,
};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

fn digest(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn spec() -> EmbeddingModelSpec {
    EmbeddingModelSpec {
        model_name: "synthetic/multilingual-small".into(),
        model_revision: "commit-abc123".into(),
        weights_sha256: [1; 32],
        tokenizer_revision: "commit-def456".into(),
        tokenizer_files_sha256: [2; 32],
        tokenizer_config_sha256: [3; 32],
        unicode_preprocessing: "NFC".into(),
        input_preprocessing: "unit-text-v1".into(),
        query_template: "query: {text}".into(),
        passage_template: "passage: {text}".into(),
        pooling: "masked-mean".into(),
        attention_masking: "exclude-padding".into(),
        max_tokens: 512,
        chunking: "one-unit-no-neighbor".into(),
        truncation: "right-at-512".into(),
        dimension: 3,
        precision: VectorPrecision::F32,
        normalization: VectorNormalization::UnitL2,
        metric: VectorMetric::Dot,
        runtime_family: "rust-cpu".into(),
        runtime_build: "build-789".into(),
        native_binary_sha256: Some([4; 32]),
        deterministic_config_sha256: [5; 32],
    }
}

fn unit(part_ordinal: u32) -> KnowledgeUnit {
    let version = ResourceVersionRef {
        source_id: SourceId::from_uuid(id(1)),
        resource_id: ResourceId::from_uuid(id(2)),
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: format!("part-{part_ordinal}"),
        logical_path: format!("part-{part_ordinal}.txt"),
        ordinal: part_ordinal,
    };
    let locator = NativeLocator::Text {
        line_start: 0,
        line_end: 1,
    };
    let profile = ExtractionProfileId::parse(&digest(9)).unwrap();
    let text = "東京の予算".to_string();
    KnowledgeUnit {
        unit_id: UnitId::derive(&version, &part, &profile, &locator, 0).unwrap(),
        version: version.into(),
        part: part.into(),
        parent_unit_id: None,
        ordinal: 0,
        kind: UnitKind::PlainText,
        text_sha256: text_sha256(&text),
        text,
        locator,
        provenance: UnitProvenance {
            source_snapshot: "snapshot-1".into(),
            authoritative_representation_ref: "representation-1".into(),
            raw: RawBinding {
                sha256: [7; 32],
                size_bytes: 1024,
                media_type: "text/plain".into(),
            },
            detected_format: FormatId::Text,
            archive_inner_format: None,
            profile,
            parser_build_id: "reader-1".into(),
        }
        .into(),
    }
}

fn later_unit_in_same_part(first: &KnowledgeUnit) -> KnowledgeUnit {
    let mut later = first.clone();
    later.ordinal = 1;
    later.locator = NativeLocator::Text {
        line_start: 1,
        line_end: 2,
    };
    later.unit_id = UnitId::derive(
        &later.version,
        &later.part,
        &later.provenance.profile,
        &later.locator,
        later.ordinal,
    )
    .unwrap();
    later
}

fn authority(unit: &KnowledgeUnit) -> VectorAuthorityInput {
    VectorAuthorityInput {
        generation: ProjectionGenerationKey {
            source_id: unit.version.source_id,
            generation_id: ProjectionGenerationId::from_uuid(id(3)),
        },
        version: (*unit.version).clone(),
        part: (*unit.part).clone(),
        authoritative_representation_ref: unit.provenance.authoritative_representation_ref.clone(),
        raw: unit.provenance.raw.clone(),
        profile: unit.provenance.profile.clone(),
        authority_scope_key: "tenant-service-owner".into(),
        retention_lease_id: "lease-1".into(),
        lifetime_scope_id: String::new(),
        retention_mode: RetentionMode::PersistentResource,
        lease_expires_at: None,
    }
}

fn input(units: &[KnowledgeUnit]) -> VectorManifestInput {
    VectorManifestInput {
        bundle_key: authority(&units[0]).generation,
        body_receipt_digest: digest(10),
        source_snapshot: "snapshot-1".into(),
        authority_scope_key: "tenant-service-owner".into(),
        retention_lease_id: "lease-1".into(),
        lease_expires_at: None,
        source_declares_persistent_embedding_permission: false,
        units: units
            .iter()
            .map(|unit| VectorManifestUnit {
                unit: unit.clone(),
                authority: authority(unit),
                coverage: VectorUnitCoverage::Complete,
            })
            .collect(),
        nonindexed_retention_unit_ids: Vec::new(),
        lexical_analyzer_revision: "lexical-v1".into(),
        graph_schema_revision: "graph-v1".into(),
    }
}

fn entries(spec: &EmbeddingModelSpec, units: &[KnowledgeUnit]) -> Vec<VectorEntryRef> {
    units
        .iter()
        .map(|unit| {
            BoundEmbedding::new(spec, unit, &authority(unit), vec![1.0, 0.0, 0.0])
                .unwrap()
                .entry_ref()
        })
        .collect()
}

fn index() -> VectorIndexDescriptor {
    VectorIndexDescriptor {
        engine: "exact".into(),
        engine_build: "build-1".into(),
        parameters_digest: digest(11),
        index_receipt_digest: digest(12),
        index_digest: digest(13),
    }
}

fn stage(
    spec: &EmbeddingModelSpec,
    input: &VectorManifestInput,
    entries: &[VectorEntryRef],
) -> VectorProjectionManifest {
    VectorProjectionManifest::stage(
        spec,
        input,
        index(),
        entries,
        VectorStorageKind::Volatile,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap()
}

#[test]
fn model_id_changes_for_every_semantic_input() {
    let base = spec();
    let id = base.validate_and_id().unwrap();
    assert!(id.as_str().starts_with("sha256:"));
    assert_eq!(id.as_str().len(), 71);
    let mut changed = Vec::new();
    macro_rules! mutate {
        ($field:ident, $value:expr) => {{
            let mut candidate = base.clone();
            candidate.$field = $value;
            changed.push(candidate);
        }};
    }
    mutate!(model_name, "synthetic/other".into());
    mutate!(model_revision, "commit-next".into());
    mutate!(weights_sha256, [21; 32]);
    mutate!(tokenizer_revision, "commit-next".into());
    mutate!(tokenizer_files_sha256, [22; 32]);
    mutate!(tokenizer_config_sha256, [23; 32]);
    mutate!(unicode_preprocessing, "NFKC".into());
    mutate!(input_preprocessing, "unit-text-v2".into());
    mutate!(query_template, "Q: {text}".into());
    mutate!(passage_template, "P: {text}".into());
    mutate!(pooling, "cls".into());
    mutate!(attention_masking, "include-padding".into());
    mutate!(max_tokens, 128);
    mutate!(chunking, "overlapping-subunits".into());
    mutate!(truncation, "left-at-512".into());
    mutate!(dimension, 4);
    mutate!(precision, VectorPrecision::F16);
    mutate!(normalization, VectorNormalization::None);
    mutate!(metric, VectorMetric::Cosine);
    mutate!(runtime_family, "other-rust-cpu".into());
    mutate!(runtime_build, "build-790".into());
    mutate!(native_binary_sha256, Some([24; 32]));
    mutate!(deterministic_config_sha256, [25; 32]);
    for candidate in changed {
        assert_ne!(candidate.validate_and_id().unwrap(), id);
    }
    let mut invalid = base;
    invalid.model_revision = "main".into();
    assert!(invalid.validate_and_id().is_err());
}

#[test]
fn wrong_dimension_nan_inf_zero_norm_rejected() {
    let spec = spec();
    let unit = unit(0);
    let pinned = authority(&unit);
    for values in [
        vec![1.0, 0.0],
        vec![f32::NAN, 0.0, 0.0],
        vec![f32::INFINITY, 0.0, 0.0],
        vec![0.0, 0.0, 0.0],
        vec![2.0, 0.0, 0.0],
    ] {
        assert!(BoundEmbedding::new(&spec, &unit, &pinned, values.clone()).is_err());
        assert!(QueryEmbedding::new(&spec, values).is_err());
    }
    assert!(BoundEmbedding::new(&spec, &unit, &pinned, vec![1.0, 0.0, 0.0]).is_ok());
}

#[test]
fn one_unit_two_parts_and_wrong_parent_rejected() {
    let spec = spec();
    let units = [unit(0), unit(1)];
    assert_ne!(units[0].unit_id, units[1].unit_id);
    assert_eq!(units[0].version.resource_id, units[1].version.resource_id);
    let input = input(&units);
    let entries = entries(&spec, &units);
    assert_eq!(
        stage(&spec, &input, &entries)
            .validate_against(&input, &entries)
            .unwrap()
            .indexed_count,
        2
    );

    let hit = RankedVectorHit {
        hit: entries[0].hit.clone(),
        model_id: entries[0].model_id.clone(),
        rank: 1,
        raw_similarity: 0.8,
    };
    assert!(
        hit.validate(&spec, &units[0], &authority(&units[0]))
            .is_ok()
    );
    let mut wrong = hit.clone();
    wrong.hit.version.resource_id = ResourceId::from_uuid(id(99));
    assert!(
        wrong
            .validate(&spec, &units[0], &authority(&units[0]))
            .is_err()
    );
    let mut wrong = hit.clone();
    wrong.hit.part = (*units[1].part).clone();
    assert!(
        wrong
            .validate(&spec, &units[0], &authority(&units[0]))
            .is_err()
    );
    let mut wrong = hit.clone();
    wrong.rank = 0;
    assert!(
        wrong
            .validate(&spec, &units[0], &authority(&units[0]))
            .is_err()
    );
    let mut wrong = hit;
    wrong.raw_similarity = f32::NAN;
    assert!(
        wrong
            .validate(&spec, &units[0], &authority(&units[0]))
            .is_err()
    );

    let mut tampered_unit = units[0].clone();
    tampered_unit.locator = NativeLocator::Text {
        line_start: 1,
        line_end: 2,
    };
    assert!(
        BoundEmbedding::new(
            &spec,
            &tampered_unit,
            &authority(&tampered_unit),
            vec![1.0, 0.0, 0.0]
        )
        .is_err()
    );
}

#[test]
fn manifest_missing_extra_duplicate_or_stale_entry_rejected() {
    let spec = spec();
    let units = [unit(0), unit(1)];
    let input = input(&units);
    let entries = entries(&spec, &units);
    let manifest = stage(&spec, &input, &entries);
    assert!(manifest.validate_against(&input, &entries).is_ok());
    assert!(manifest.validate_against(&input, &entries[..1]).is_err());
    let mut duplicate = entries.clone();
    duplicate.push(entries[0].clone());
    assert!(manifest.validate_against(&input, &duplicate).is_err());
    let mut extra = entries.clone();
    extra[1].hit.unit_id = UnitId::derive(
        &units[1].version,
        &units[1].part,
        &units[1].provenance.profile,
        &NativeLocator::Text {
            line_start: 2,
            line_end: 3,
        },
        0,
    )
    .unwrap();
    assert!(manifest.validate_against(&input, &extra).is_err());
    let mut stale = entries.clone();
    stale[0].hit.generation.generation_id = ProjectionGenerationId::from_uuid(id(98));
    assert!(manifest.validate_against(&input, &stale).is_err());
    let mut stale = entries.clone();
    stale[0].hit.raw.sha256[0] ^= 1;
    assert!(manifest.validate_against(&input, &stale).is_err());
    let mut stale = entries.clone();
    stale[0].hit.authoritative_representation_ref = "old-representation".into();
    assert!(manifest.validate_against(&input, &stale).is_err());
    let mut wrong_model = entries.clone();
    wrong_model[0].model_id = {
        let mut changed = spec.clone();
        changed.model_revision = "commit-next".into();
        changed.validate_and_id().unwrap()
    };
    assert!(manifest.validate_against(&input, &wrong_model).is_err());
    let mut corrupted = manifest;
    corrupted.index.index_digest = digest(99);
    assert!(corrupted.validate_against(&input, &entries).is_err());
}

#[test]
fn cache_key_scope_lease_lifetime_isolation() {
    let spec = spec();
    let unit = unit(0);
    let original = authority(&unit);
    let first = BoundEmbedding::new(&spec, &unit, &original, vec![1.0, 0.0, 0.0]).unwrap();
    for changed in [
        {
            let mut x = original.clone();
            x.authority_scope_key = "other-scope".into();
            x
        },
        {
            let mut x = original.clone();
            x.retention_lease_id = "other-lease".into();
            x
        },
        {
            let mut x = original.clone();
            x.lifetime_scope_id = "other-session".into();
            x
        },
    ] {
        assert!(first.validate_binding(&spec, &unit, &changed).is_err());
    }
    let mut wrong_key = first.entry_ref();
    wrong_key.cache_key.authority_scope_key = "other-scope".into();
    let units = [unit];
    let input = input(&units);
    let entries = entries(&spec, &units);
    let manifest = stage(&spec, &input, &entries);
    assert!(manifest.validate_against(&input, &[wrong_key]).is_err());
}

#[test]
fn partial_validated_units_only() {
    let spec = spec();
    let units = [unit(0), unit(1)];
    let mut input = input(&units);
    input.units[0].coverage = VectorUnitCoverage::PartialValidated;
    input.units[1].authority.retention_mode = RetentionMode::PersistentDiscoveryMetadata;
    input.nonindexed_retention_unit_ids = vec![units[1].unit_id];
    let entries = entries(&spec, &units[..1]);
    let manifest = stage(&spec, &input, &entries);
    let receipt = manifest.validate_against(&input, &entries).unwrap();
    assert_eq!(receipt.indexed_count, 1);
    assert_eq!(receipt.nonindexed_retention_count, 1);
    let mut undeclared = input.clone();
    undeclared.nonindexed_retention_unit_ids.clear();
    assert!(manifest.validate_against(&undeclared, &entries).is_err());
    let mut nonindexed_and_indexed = input.clone();
    nonindexed_and_indexed.nonindexed_retention_unit_ids = vec![units[0].unit_id];
    assert!(
        manifest
            .validate_against(&nonindexed_and_indexed, &entries)
            .is_err()
    );

    let mut ephemeral = input;
    ephemeral.units[0].authority.retention_mode = RetentionMode::NoRetention;
    ephemeral.units[0].authority.lifetime_scope_id = "request-1".into();
    let mut persistent = manifest;
    persistent.storage = VectorStorageKind::Persistent;
    assert!(persistent.validate_against(&ephemeral, &entries).is_err());
}

#[test]
fn immutable_unit_fields_cannot_reuse_a_manifest_seal() {
    let spec = spec();
    {
        let first = unit(0);
        let mut later = later_unit_in_same_part(&first);
        later.parent_unit_id = Some(first.unit_id);
        let foreign_part = unit(1);
        let units = [first, later, foreign_part];
        let input = input(&units);
        let entries = entries(&spec, &units);
        let manifest = stage(&spec, &input, &entries);

        let mut changed = input.clone();
        changed.units[1].unit.parent_unit_id = None;
        assert_ne!(
            manifest.unit_bindings_digest,
            stage(&spec, &changed, &entries).unit_bindings_digest
        );
        assert!(manifest.validate_against(&changed, &entries).is_err());

        let mut changed = input.clone();
        Arc::make_mut(&mut changed.units[1].unit.provenance).parser_build_id = "reader-2".into();
        assert_ne!(
            manifest.unit_bindings_digest,
            stage(&spec, &changed, &entries).unit_bindings_digest
        );
        assert!(manifest.validate_against(&changed, &entries).is_err());

        let mut changed = input.clone();
        changed.units[1].unit.parent_unit_id = Some(units[2].unit_id);
        assert!(manifest.validate_against(&changed, &entries).is_err());
        let mut changed = input.clone();
        changed.units[1].unit.kind = UnitKind::Paragraph;
        assert!(manifest.validate_against(&changed, &entries).is_err());
        let mut changed = input.clone();
        Arc::make_mut(&mut changed.units[1].unit.provenance).detected_format = FormatId::Csv;
        assert!(manifest.validate_against(&changed, &entries).is_err());
        let mut changed = input;
        Arc::make_mut(&mut changed.units[1].unit.provenance).archive_inner_format =
            Some(FormatId::Text);
        assert!(manifest.validate_against(&changed, &entries).is_err());
    }

    let mut docx = unit(2);
    docx.locator = NativeLocator::Docx {
        steps: vec![DocxStep::BodyBlock(0)],
    };
    docx.kind = UnitKind::Paragraph;
    Arc::make_mut(&mut docx.provenance).detected_format = FormatId::Docx;
    docx.unit_id = UnitId::derive(
        &docx.version,
        &docx.part,
        &docx.provenance.profile,
        &docx.locator,
        docx.ordinal,
    )
    .unwrap();
    let docx_entries = entries(&spec, &[docx.clone()]);
    let paragraph_input = input(&[docx]);
    let paragraph_manifest = stage(&spec, &paragraph_input, &docx_entries);
    let mut heading_input = paragraph_input;
    heading_input.units[0].unit.kind = UnitKind::Heading;
    let heading_manifest = stage(&spec, &heading_input, &docx_entries);
    assert_ne!(
        paragraph_manifest.unit_bindings_digest,
        heading_manifest.unit_bindings_digest
    );

    let mut spreadsheet = unit(3);
    spreadsheet.locator = NativeLocator::Spreadsheet {
        sheet_ordinal: 0,
        row: 0,
        col: 0,
    };
    spreadsheet.kind = UnitKind::SpreadsheetCell;
    Arc::make_mut(&mut spreadsheet.provenance).detected_format = FormatId::Xlsx;
    spreadsheet.unit_id = UnitId::derive(
        &spreadsheet.version,
        &spreadsheet.part,
        &spreadsheet.provenance.profile,
        &spreadsheet.locator,
        spreadsheet.ordinal,
    )
    .unwrap();
    let sheet_entries = entries(&spec, &[spreadsheet.clone()]);
    let xlsx_input = input(&[spreadsheet]);
    let xlsx_manifest = stage(&spec, &xlsx_input, &sheet_entries);
    let mut xlsm_input = xlsx_input;
    Arc::make_mut(&mut xlsm_input.units[0].unit.provenance).detected_format = FormatId::Xlsm;
    let xlsm_manifest = stage(&spec, &xlsm_input, &sheet_entries);
    assert_ne!(
        xlsx_manifest.unit_bindings_digest,
        xlsm_manifest.unit_bindings_digest
    );
}

#[test]
fn stage_rejects_foreign_future_or_missing_parent_and_wrong_kind_or_format() {
    let spec = spec();
    let first = unit(0);
    let later = later_unit_in_same_part(&first);
    let foreign_part = unit(1);
    let units = [first, later, foreign_part];
    let indexed_entries = entries(&spec, &units);
    let valid = input(&units);

    let mut wrong = valid.clone();
    wrong.units[1].unit.parent_unit_id = Some(units[2].unit_id);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = valid.clone();
    wrong.units[0].unit.parent_unit_id = Some(units[1].unit_id);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = valid.clone();
    wrong.units[1].unit.parent_unit_id = Some(
        UnitId::derive(
            &units[1].version,
            &units[1].part,
            &units[1].provenance.profile,
            &NativeLocator::Text {
                line_start: 9,
                line_end: 10,
            },
            9,
        )
        .unwrap(),
    );
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = valid.clone();
    wrong.units[0].unit.kind = UnitKind::Paragraph;
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = valid.clone();
    Arc::make_mut(&mut wrong.units[0].unit.provenance).detected_format = FormatId::Csv;
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = valid;
    Arc::make_mut(&mut wrong.units[0].unit.provenance).archive_inner_format = Some(FormatId::Text);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &indexed_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );

    let mut archive = unit(2);
    archive.locator = NativeLocator::Archive {
        members: vec!["leaf.txt".into()],
        inner: Box::new(NativeLocator::Text {
            line_start: 0,
            line_end: 1,
        }),
    };
    Arc::make_mut(&mut archive.provenance).detected_format = FormatId::Zip;
    Arc::make_mut(&mut archive.provenance).archive_inner_format = Some(FormatId::Text);
    archive.unit_id = UnitId::derive(
        &archive.version,
        &archive.part,
        &archive.provenance.profile,
        &archive.locator,
        archive.ordinal,
    )
    .unwrap();
    let archive_entries = entries(&spec, &[archive.clone()]);
    let archive_input = input(&[archive]);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &archive_input,
            index(),
            &archive_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_ok()
    );
    let mut wrong = archive_input.clone();
    Arc::make_mut(&mut wrong.units[0].unit.provenance).archive_inner_format = Some(FormatId::Csv);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &archive_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
    let mut wrong = archive_input;
    Arc::make_mut(&mut wrong.units[0].unit.provenance).archive_inner_format = None;
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &wrong,
            index(),
            &archive_entries,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
}

#[test]
fn retention_prohibition_is_derived_not_caller_declared() {
    let spec = spec();
    let eligible = unit(0);
    let mut persistent = input(std::slice::from_ref(&eligible));
    persistent.source_declares_persistent_embedding_permission = true;
    persistent.nonindexed_retention_unit_ids = vec![eligible.unit_id];
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &persistent,
            index(),
            &[],
            VectorStorageKind::Persistent,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );

    let mut metadata = input(std::slice::from_ref(&eligible));
    metadata.units[0].authority.retention_mode = RetentionMode::PersistentDiscoveryMetadata;
    metadata.nonindexed_retention_unit_ids = vec![eligible.unit_id];
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &metadata,
            index(),
            &[],
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_ok()
    );
    metadata.nonindexed_retention_unit_ids.clear();
    let indexed = entries(&spec, &[eligible]);
    assert!(
        VectorProjectionManifest::stage(
            &spec,
            &metadata,
            index(),
            &indexed,
            VectorStorageKind::Volatile,
            OffsetDateTime::UNIX_EPOCH
        )
        .is_err()
    );
}

#[test]
fn retention_storage_matrix_records_only_forbidden_units() {
    let spec = spec();
    let units = [unit(0), unit(1), unit(2), unit(3), unit(4)];
    let mut input = input(&units);
    let expiry = OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1);
    input.lease_expires_at = Some(expiry);
    for item in &mut input.units {
        item.authority.lease_expires_at = Some(expiry);
    }
    input.units[1].authority.retention_mode = RetentionMode::PersistentDiscoveryMetadata;
    input.units[2].authority.retention_mode = RetentionMode::CacheWithExpiry;
    input.units[3].authority.retention_mode = RetentionMode::SessionOnly;
    input.units[3].authority.lifetime_scope_id = "session-1".into();
    input.units[4].authority.retention_mode = RetentionMode::NoRetention;
    input.units[4].authority.lifetime_scope_id = "request-1".into();
    let entry = |input: &VectorManifestInput, index: usize| {
        let item = &input.units[index];
        BoundEmbedding::new(&spec, &item.unit, &item.authority, vec![1.0, 0.0, 0.0])
            .unwrap()
            .entry_ref()
    };

    input.nonindexed_retention_unit_ids = vec![units[1].unit_id];
    let volatile_entries = [
        entry(&input, 0),
        entry(&input, 2),
        entry(&input, 3),
        entry(&input, 4),
    ];
    let manifest = VectorProjectionManifest::stage(
        &spec,
        &input,
        index(),
        &volatile_entries,
        VectorStorageKind::Volatile,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(manifest.indexed_unit_count, 4);
    assert_eq!(
        manifest.nonindexed_retention_unit_ids,
        vec![units[1].unit_id]
    );

    input.source_declares_persistent_embedding_permission = true;
    input.nonindexed_retention_unit_ids = units[1..].iter().map(|unit| unit.unit_id).collect();
    let persistent_entries = [entry(&input, 0)];
    let manifest = VectorProjectionManifest::stage(
        &spec,
        &input,
        index(),
        &persistent_entries,
        VectorStorageKind::Persistent,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(manifest.indexed_unit_count, 1);
    assert_eq!(manifest.nonindexed_retention_unit_ids.len(), 4);

    input.source_declares_persistent_embedding_permission = false;
    input.nonindexed_retention_unit_ids = units.iter().map(|unit| unit.unit_id).collect();
    let manifest = VectorProjectionManifest::stage(
        &spec,
        &input,
        index(),
        &[],
        VectorStorageKind::Persistent,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(manifest.indexed_unit_count, 0);
    assert_eq!(manifest.nonindexed_retention_unit_ids.len(), 5);
}

#[test]
fn disabled_default() {
    assert_eq!(
        VectorActivationPolicy::default(),
        VectorActivationPolicy::Disabled
    );
}

#[test]
fn segment_checks_compose_to_the_full_manifest_in_any_generation() {
    let spec = spec();
    let model = spec.validate_and_id().unwrap();
    let first = unit(0);
    let units = vec![first.clone(), later_unit_in_same_part(&first), unit(1)];
    let input = input(&units);
    let entries = entries(&spec, &units);
    let full = stage(&spec, &input, &entries);
    assert_eq!(full.schema_version, 2);
    let header = input.header();
    let volatile = VectorStorageKind::Volatile;
    let checks = vec![
        check_segment(&model, &header, volatile, &input.units[..2], &entries[..2]).unwrap(),
        check_segment(&model, &header, volatile, &input.units[2..], &entries[2..]).unwrap(),
    ];
    let by_segments = VectorProjectionManifest::stage_segments(
        &spec,
        &header,
        index(),
        &checks,
        volatile,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(by_segments, full);
    assert!(full.validate_segments(&header, &checks).is_ok());

    // A segment's check names no generation: the same Units bound to another
    // generation check the same, while the manifest binds its own.
    let other = ProjectionGenerationKey {
        source_id: first.version.source_id,
        generation_id: ProjectionGenerationId::from_uuid(id(4)),
    };
    let mut moved = input.clone();
    moved.bundle_key = other;
    for item in &mut moved.units {
        item.authority.generation = other;
    }
    let moved_entries: Vec<VectorEntryRef> = moved
        .units
        .iter()
        .map(|item| {
            BoundEmbedding::new(&spec, &item.unit, &item.authority, vec![1.0, 0.0, 0.0])
                .unwrap()
                .entry_ref()
        })
        .collect();
    let moved_check = check_segment(
        &model,
        &moved.header(),
        volatile,
        &moved.units[2..],
        &moved_entries[2..],
    )
    .unwrap();
    assert_eq!(moved_check, checks[1]);
    assert!(full.validate_segments(&moved.header(), &checks).is_err());

    // One segment spans one Part, and the segment order is sealed.
    assert!(check_segment(&model, &header, volatile, &input.units, &entries).is_err());
    let reordered = vec![checks[1].clone(), checks[0].clone()];
    assert!(full.validate_segments(&header, &reordered).is_err());
    // An entry of another segment is rejected.
    assert!(check_segment(&model, &header, volatile, &input.units[..2], &entries[1..]).is_err());
}
