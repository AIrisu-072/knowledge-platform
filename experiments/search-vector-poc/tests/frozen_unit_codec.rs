use search_vector_poc::corpus::{Corpus, SEED};
use search_vector_poc::validate::{encode_text_locator, normalized_text, sha256_hex, unit_id};
use uuid::Uuid;

#[test]
fn frozen_nine_field_vectors_and_text_digest_match_spec() {
    let mut unit = Corpus::synthetic(32, SEED).unwrap().records[0].unit.clone();
    unit.source_id = Uuid::from_u128(1);
    unit.resource_id = Uuid::from_u128(2);
    unit.source_native_version = Uuid::from_u128(3).to_string();
    unit.source_native_part_id = Uuid::from_u128(4).to_string();
    unit.logical_path = "primary".into();
    unit.part_ordinal = 0;
    unit.unit_ordinal = 0;
    assert_eq!(
        encode_text_locator(0, 1)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "6e61746976652d6c6f6361746f723a763100050000000000000001"
    );
    assert_eq!(
        unit_id(&unit).unwrap(),
        "ku1:c6b9bfcc8a9d53ee19966146ccfce5a8b2f6f792f7cab53d4a9154377e867ca1"
    );
    unit.source_native_part_id = Uuid::from_u128(5).to_string();
    unit.logical_path = "attachment".into();
    unit.part_ordinal = 1;
    assert_eq!(
        unit_id(&unit).unwrap(),
        "ku1:3b01330d059d71802ec8b3bc216ff9739b3765843892fe4b8fa9bdfa987b115e"
    );
    assert_eq!(
        sha256_hex(normalized_text("東京\r\n").as_bytes()),
        "866bff0df548a00eaad416ca1fc987f20d94dae000fee8abd356dfb29bd15934"
    );
}

#[test]
fn source_binding_cannot_be_rewritten_even_with_recomputed_unit_id() {
    let mut corpus = Corpus::synthetic(32, SEED).unwrap();
    corpus.records[0].unit.source_id = Uuid::from_u128(999);
    corpus.records[0].unit.unit_id = unit_id(&corpus.records[0].unit).unwrap();
    assert!(corpus.validate().is_err());
}

#[test]
fn text_digest_and_raw_binding_tamper_are_rejected() {
    let mut corpus = Corpus::synthetic(32, SEED).unwrap();
    corpus.records[0].unit.text.push('改');
    assert!(corpus.validate().is_err());
    let mut corpus = Corpus::synthetic(32, SEED).unwrap();
    corpus.records[0].unit.raw_size_bytes += 1;
    assert!(corpus.validate().is_err());
    let mut corpus = Corpus::synthetic(32, SEED).unwrap();
    corpus.records[0].unit.generation_id = Uuid::from_u128(999);
    assert!(corpus.validate().is_err());
}

#[test]
fn distinct_parts_of_one_version_require_source_owned_bindings() {
    let corpus = Corpus::synthetic(32, SEED).unwrap();
    let record = &corpus.records[0];
    assert_eq!(record.additional_units.len(), 1);
    let attachment = &record.additional_units[0];
    assert_eq!(attachment.resource_id, record.unit.resource_id);
    assert_eq!(
        attachment.source_native_version,
        record.unit.source_native_version
    );
    assert_ne!(
        attachment.source_native_part_id,
        record.unit.source_native_part_id
    );
    assert_ne!(
        attachment.authoritative_representation_ref,
        record.unit.authoritative_representation_ref
    );
    assert_ne!(attachment.raw_sha256, record.unit.raw_sha256);
    assert_ne!(attachment.line_end, record.unit.line_end);
    assert_ne!(
        encode_text_locator(attachment.line_start, attachment.line_end).unwrap(),
        encode_text_locator(record.unit.line_start, record.unit.line_end).unwrap()
    );
    assert_ne!(attachment.unit_id, record.unit.unit_id);

    let mut changed = Corpus::synthetic(32, SEED).unwrap();
    changed.records[0].additional_units[0].source_native_part_id =
        changed.records[0].unit.source_native_part_id.clone();
    changed.records[0].additional_units[0].unit_id =
        unit_id(&changed.records[0].additional_units[0]).unwrap();
    assert!(
        changed.validate().is_err(),
        "Source Part must remain authoritative after UnitId recomputation"
    );

    let mut changed = Corpus::synthetic(32, SEED).unwrap();
    changed.records[0].additional_units[0].authoritative_representation_ref = "forged".into();
    assert!(changed.validate().is_err());

    let mut changed = Corpus::synthetic(32, SEED).unwrap();
    changed.records[0].additional_units[0].raw_bytes.push(b'!');
    changed.records[0].additional_units[0].raw_sha256 =
        sha256_hex(&changed.records[0].additional_units[0].raw_bytes);
    changed.records[0].additional_units[0].raw_size_bytes += 1;
    assert!(
        changed.validate().is_err(),
        "recomputed raw fields cannot rewrite Source-owned Part"
    );

    let mut changed = Corpus::synthetic(32, SEED).unwrap();
    changed.records[0].additional_units[0].line_end = 1;
    changed.records[0].additional_units[0].unit_id =
        unit_id(&changed.records[0].additional_units[0]).unwrap();
    assert!(
        changed.validate().is_err(),
        "recomputed UnitId cannot rewrite Source locator"
    );
}
