//! Generated JSON Schema: byte-for-byte reproducibility and the relation
//! "everything Rust accepts, the schema accepts". Rust-only rejections carry a
//! `rust_only:<category>` label in the shared rejection table.

mod common;

use audit_core::schema::{SCHEMA_PATH, render_json_schema};
use audit_core::{Catalog, Origin, generate_json_schema, validate_envelope};
use common::*;
use serde_json::{Value, json};

fn schema_file() -> String {
    format!("{}/../../{SCHEMA_PATH}", env!("CARGO_MANIFEST_DIR"))
}

fn validator() -> jsonschema::Validator {
    jsonschema::draft202012::new(&generate_json_schema(Catalog::embedded()))
        .expect("generated schema compiles as draft 2020-12")
}

/// `AUDIT_SCHEMA_BLESS=1` rewrites the committed schema and then fails, so a
/// blessed run can never pass silently; it is refused when `CI` is set.
#[test]
fn schema_is_reproducible() {
    let generated = render_json_schema(Catalog::embedded());
    assert!(generated.ends_with("}\n"));
    if std::env::var("AUDIT_SCHEMA_BLESS").as_deref() == Ok("1") {
        assert!(
            std::env::var_os("CI").is_none(),
            "refusing to bless spec/telemetry/audit-event.schema.json in CI"
        );
        std::fs::write(schema_file(), &generated).expect("bless schema");
        panic!("blessed; rerun without AUDIT_SCHEMA_BLESS");
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
        "provenance.document-audit-outbox-v0",
        "provenance.audit-store-control-v1",
        "provenance.audit-relay-control-v1",
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
fn every_control_envelope_rust_accepts_the_schema_accepts() {
    let validator = validator();
    for (name, origin, value) in control_envelopes() {
        audit_core::validate_envelope(&value, origin).unwrap_or_else(|r| panic!("{name}: {r}"));
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{name}: schema rejected: {errors:?}");
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

/// Values a single member is replaced with.
fn replacements() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(false),
        json!(0),
        json!(1),
        json!(-1),
        json!(i64::MAX),
        json!(256),
        json!(1.5),
        json!(""),
        json!("x"),
        json!(" x "),
        json!("x\u{202e}"),
        json!("a".repeat(513)),
        json!(NIL),
        json!(OTHER_DOC),
        json!("ab".repeat(32)),
        json!(OCCURRED),
        json!([]),
        json!([1]),
        json!({}),
    ]
}

/// JSON pointers of every member and array element.
fn pointers(value: &Value, prefix: &str, out: &mut Vec<String>) {
    let escape = |key: &str| key.replace('~', "~0").replace('/', "~1");
    match value {
        Value::Object(map) => {
            for (key, member) in map {
                let pointer = format!("{prefix}/{}", escape(key));
                out.push(pointer.clone());
                pointers(member, &pointer, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let pointer = format!("{prefix}/{index}");
                out.push(pointer.clone());
                pointers(item, &pointer, out);
            }
        }
        _ => {}
    }
}

fn mutate(base: &Value, pointer: &str, replacement: Option<&Value>) -> Value {
    let mut value = base.clone();
    let (parent, key) = pointer.rsplit_once('/').expect("pointer");
    let key = key.replace("~1", "/").replace("~0", "~");
    match value.pointer_mut(parent).expect("parent") {
        Value::Object(map) => match replacement {
            Some(new) => {
                map.insert(key, new.clone());
            }
            None => {
                map.remove(&key);
            }
        },
        Value::Array(items) => {
            let index: usize = key.parse().expect("index");
            match replacement {
                Some(new) => items[index] = new.clone(),
                None => {
                    items.remove(index);
                }
            }
        }
        _ => unreachable!("parents are containers"),
    }
    value
}

/// Systematic single-member mutations (removal, replacement with each of
/// [`replacements`], one added unknown member per object) of every accepted
/// envelope: whenever Rust still accepts a mutant, the schema must too.
#[test]
fn mutants_rust_accepts_are_accepted_by_the_schema() {
    let validator = validator();
    let mut bases: Vec<(String, Origin, Value)> = accepted_fixtures()
        .into_iter()
        .map(|fixture| {
            let value = audit_core::project(&fixture.row)
                .expect("projects")
                .into_value();
            (fixture.name.to_owned(), Origin::Relay, value)
        })
        .collect();
    bases.extend(control_envelopes());
    let replacements = replacements();
    let (mut mutants, mut accepted) = (0_usize, 0_usize);
    for (name, origin, base) in &bases {
        let mut paths = Vec::new();
        pointers(base, "", &mut paths);
        let mut candidates: Vec<Value> = Vec::new();
        for pointer in &paths {
            candidates.push(mutate(base, pointer, None));
            for replacement in &replacements {
                candidates.push(mutate(base, pointer, Some(replacement)));
            }
        }
        let mut objects = vec![String::new()];
        objects.extend(
            paths
                .iter()
                .filter(|p| base.pointer(p).is_some_and(Value::is_object))
                .cloned(),
        );
        for object in objects {
            candidates.push(mutate(
                base,
                &format!("{object}/zz_unknown"),
                Some(&json!(1)),
            ));
        }
        for mutant in candidates {
            mutants += 1;
            if validate_envelope(&mutant, *origin).is_ok() {
                accepted += 1;
                let errors: Vec<String> = validator
                    .iter_errors(&mutant)
                    .map(|e| format!("{e} at {}", e.instance_path()))
                    .collect();
                assert!(
                    errors.is_empty(),
                    "{name}: Rust accepts a mutant the schema rejects: {errors:?}\n{mutant}"
                );
            }
        }
    }
    assert!(mutants > 20_000, "mutation space shrank: {mutants}");
    assert!(
        accepted > 100,
        "too few accepted mutants to be meaningful: {accepted}"
    );
}
