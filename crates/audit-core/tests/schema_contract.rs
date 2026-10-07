//! Generated JSON Schema: byte-for-byte reproducibility and the relation
//! "everything Rust accepts, the schema accepts". Rust-only rejections carry a
//! `rust_only:<category>` label in the shared rejection table.

mod common;

use audit_core::schema::{SCHEMA_PATH, render_json_schema};
use audit_core::{Catalog, generate_json_schema};
use common::*;
use serde_json::Value;

fn schema_file() -> String {
    format!("{}/../../{SCHEMA_PATH}", env!("CARGO_MANIFEST_DIR"))
}

fn validator() -> jsonschema::Validator {
    jsonschema::draft202012::new(&generate_json_schema(Catalog::embedded()))
        .expect("generated schema compiles as draft 2020-12")
}

#[test]
fn schema_is_reproducible() {
    let generated = render_json_schema(Catalog::embedded());
    assert!(generated.ends_with("}\n"));
    if std::env::var("AUDIT_SCHEMA_BLESS").as_deref() == Ok("1") {
        std::fs::write(schema_file(), &generated).expect("bless schema");
    }
    let committed = std::fs::read_to_string(schema_file())
        .expect("committed schema exists; run with AUDIT_SCHEMA_BLESS=1 to create it");
    assert!(
        committed == generated,
        "spec/telemetry/audit-event.schema.json is stale; regenerate with AUDIT_SCHEMA_BLESS=1"
    );
    let parsed: Value = serde_json::from_str(&committed).expect("committed schema is JSON");
    assert_eq!(parsed, generate_json_schema(Catalog::embedded()));
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(
        render_json_schema(Catalog::embedded()),
        render_json_schema(Catalog::embedded())
    );
}

#[test]
fn schema_shares_definitions_instead_of_repeating_them() {
    let schema = generate_json_schema(Catalog::embedded());
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let defs = schema["$defs"].as_object().expect("$defs");
    for name in [
        "uuid",
        "actor",
        "reason_summary",
        "provenance_relay",
        "legacy_time",
        "digest",
    ] {
        assert!(defs.contains_key(name), "missing $defs/{name}");
    }
    let rendered = render_json_schema(Catalog::embedded());
    let anchored_uuid = "\"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$\"";
    assert_eq!(
        rendered.matches(anchored_uuid).count(),
        1,
        "uuid defined once"
    );
    assert!(rendered.matches("\"$ref\": \"#/$defs/uuid\"").count() > 40);
    assert!(rendered.matches("\"$ref\": \"#/$defs/actor\"").count() >= 2);
}

#[test]
fn everything_rust_accepts_the_schema_accepts() {
    let validator = validator();
    for fixture in accepted_fixtures() {
        let envelope = audit_core::project(&fixture.row).expect("projects");
        let errors: Vec<String> = validator
            .iter_errors(envelope.as_value())
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{}: schema rejected an accepted envelope: {errors:?}",
            fixture.name
        );
    }
}

#[test]
fn rust_only_rejections_are_labeled_and_all_others_fail_the_schema() {
    let validator = validator();
    for case in envelope_rejections() {
        let schema_accepts = match &case.input {
            Input::Value(value) => validator.is_valid(value),
            Input::Text(text) => {
                serde_json::from_str::<Value>(text).is_ok_and(|v| validator.is_valid(&v))
            }
        };
        match case.rust_only {
            Some(category) => assert!(
                schema_accepts,
                "{}: labeled rust_only:{category} but the schema rejects it",
                case.name
            ),
            None => assert!(
                !schema_accepts,
                "{}: the schema accepts it, label it rust_only:<category>",
                case.name
            ),
        }
    }
}
