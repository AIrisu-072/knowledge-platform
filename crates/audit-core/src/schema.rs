//! JSON Schema 2020-12 generation from the catalog.
//!
//! Relation: every envelope the Rust validator accepts is valid under the
//! generated schema. Constraints standard JSON Schema cannot express (UTF-8
//! byte limits, the 32 KiB jsonb text bound, subject/resource and
//! detail/resource equality, correlation and reason-code equality, calendar
//! validity, float versus integer, duplicate keys, the submission path, the
//! principal character set, the int8 range of `int8_text`, the `db_role`
//! actor rule) are enforced only in Rust.

use serde_json::{Map, Value, json};

use crate::catalog::{
    AUDIT_STORE_RESOURCE_ID, AdapterSpec, Catalog, EventSpec, FieldSpec, ReasonPolicy, Requirement,
    ResourceType, Segment, SubjectSpec, VersionRequirement,
};
use crate::envelope::{
    ATTRIBUTES, DATACONTENTTYPE, DATASCHEMA, MAX_REASON_UTF8_BYTES, REASON_TEXT_RETAINED,
    REGISTRATION_KINDS, SCHEMA_VERSION, SPECVERSION,
};
use crate::json::canonicalize;
use crate::kinds::{
    Kind, MAX_CODE_BYTES, MAX_CONTROL_LIST, MAX_DB_ROLE_BYTES, MAX_EVENT_TYPE_BYTES,
    MAX_PRINCIPAL_PART_BYTES, MAX_STRING_BYTES, MAX_UUID_LIST, NIL_UUID,
};

/// Repository path of the generated schema, relative to the workspace root.
pub const SCHEMA_PATH: &str = "spec/telemetry/audit-event.schema.json";
/// `$id` of the generated schema.
pub const SCHEMA_ID: &str = "urn:knowledge-platform:audit:envelope-schema:v1";

const UUID_PATTERN: &str = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const NO_CONTROL_PATTERN: &str = "^[^\\u0000-\\u001f\\u007f-\\u009f]*$";

fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/$defs/{name}") })
}

fn nullable(name: &str) -> Value {
    json!({ "anyOf": [{"type": "null"}, reference(name)] })
}

fn shared_definitions() -> Map<String, Value> {
    let int = |min: i64, max: i64| json!({"type": "integer", "minimum": min, "maximum": max});
    let mut defs = Map::new();
    let mut put = |name: &str, schema: Value| {
        defs.insert(name.to_owned(), schema);
    };
    put(
        "canonical_uuid",
        json!({"type": "string", "pattern": format!("^{UUID_PATTERN}$")}),
    );
    put(
        "uuid",
        json!({"$ref": "#/$defs/canonical_uuid", "not": {"const": NIL_UUID}}),
    );
    put("nullable_uuid", nullable("uuid"));
    put("counter", int(0, i64::MAX));
    put("nullable_counter", nullable("counter"));
    put("positive_counter", int(1, i64::MAX));
    put("boolean", json!({"type": "boolean"}));
    put(
        "digest",
        json!({"type": "array", "minItems": 32, "maxItems": 32, "items": int(0, 255)}),
    );
    put("nullable_digest", nullable("digest"));
    put(
        "principal_part",
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_PRINCIPAL_PART_BYTES,
            "pattern": NO_CONTROL_PATTERN
        }),
    );
    put(
        "principal",
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["identityProvider", "principalId"],
            "properties": {
                "identityProvider": reference("principal_part"),
                "principalId": reference("principal_part")
            }
        }),
    );
    put(
        "legacy_time",
        json!({
            "description": "time 0.3 serde tuple [year, ordinal, hour, minute, second, nanosecond, offset_h, offset_m, offset_s]; not RFC 3339",
            "type": "array",
            "minItems": 9,
            "maxItems": 9,
            "prefixItems": [
                int(-9999, 9999), int(1, 366), int(0, 23), int(0, 59), int(0, 59),
                int(0, 999_999_999), int(-25, 25), int(-59, 59), int(-59, 59)
            ],
            "items": false
        }),
    );
    put(
        "utc_timestamp",
        json!({
            "type": "string",
            "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{6}Z$"
        }),
    );
    put(
        "uuid_list",
        json!({"type": "array", "maxItems": MAX_UUID_LIST, "uniqueItems": true, "items": reference("uuid")}),
    );
    put(
        "hex_digest",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    put("nullable_hex_digest", nullable("hex_digest"));
    put("nullable_utc_timestamp", nullable("utc_timestamp"));
    put("nullable_positive_counter", nullable("positive_counter"));
    put(
        "resource_ref",
        json!({
            "description": "Control events only: a resource id (canonical UUID, nil included, or audit-store)",
            "anyOf": [reference("canonical_uuid"), {"const": AUDIT_STORE_RESOURCE_ID}]
        }),
    );
    put(
        "event_type",
        json!({
            "description": "Control events only: an event type name in the catalog grammar",
            "type": "string",
            "maxLength": MAX_EVENT_TYPE_BYTES,
            "pattern": "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$"
        }),
    );
    put(
        "event_type_list",
        json!({
            "type": "array",
            "minItems": 1,
            "maxItems": MAX_CONTROL_LIST,
            "uniqueItems": true,
            "items": reference("event_type")
        }),
    );
    put(
        "db_role",
        json!({
            "description": "Control events only: a PostgreSQL role name that needs no quoting",
            "type": "string",
            "pattern": format!("^[a-z_][a-z0-9_$]{{0,{}}}$", MAX_DB_ROLE_BYTES - 1)
        }),
    );
    put("nullable_db_role", nullable("db_role"));
    put(
        "principal_ref",
        json!({
            "description": "Control events only: one principal part (rust_only:principal_charset applies)",
            "$ref": "#/$defs/principal_part"
        }),
    );
    put(
        "int8_text",
        json!({
            "description": "Control events only: canonical decimal text of an int8 (the i64 range is checked in Rust)",
            "type": "string",
            "pattern": "^(0|-?[1-9][0-9]{0,18})$"
        }),
    );
    put(
        "code",
        json!({"type": "string", "pattern": format!("^[a-z0-9_]{{1,{MAX_CODE_BYTES}}}$")}),
    );
    put(
        "w3c_trace_id",
        json!({"type": "string", "pattern": "^[0-9a-f]{32}$", "not": {"const": "0".repeat(32)}}),
    );
    put(
        "actor",
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["issuer", "principal_id"],
            "properties": {
                "issuer": reference("principal_part"),
                "principal_id": reference("principal_part")
            }
        }),
    );
    put(
        "reason_summary",
        json!({
            "description": "Caller reason text is never copied; only its presence and UTF-8 length",
            "type": "object",
            "additionalProperties": false,
            "required": ["provided", "utf8_bytes", "text_retained"],
            "properties": {
                "provided": {"const": true},
                "utf8_bytes": int(0, MAX_REASON_UTF8_BYTES),
                "text_retained": {"const": REASON_TEXT_RETAINED}
            }
        }),
    );
    put("extensions", json!({"type": "object", "maxProperties": 0}));
    defs
}

/// `$defs` name of an adapter's provenance schema.
fn provenance_name(adapter: &AdapterSpec) -> String {
    format!("provenance.{}", adapter.source_format)
}

fn provenance_schema(adapter: &AdapterSpec) -> Value {
    let mut properties = Map::new();
    let mut required = vec!["source_format", "adapter_version"];
    properties.insert(
        "source_format".to_owned(),
        json!({"const": adapter.source_format}),
    );
    properties.insert(
        "adapter_version".to_owned(),
        json!({"const": adapter.adapter_version}),
    );
    for (name, requirement, schema) in [
        (
            "source_commitment",
            adapter.commitment,
            reference("hex_digest"),
        ),
        (
            "registration",
            adapter.registration,
            json!({"enum": REGISTRATION_KINDS}),
        ),
    ] {
        match requirement {
            Requirement::Required => {
                properties.insert(name.to_owned(), schema);
                required.push(name);
            }
            Requirement::Optional => {
                properties.insert(name.to_owned(), schema);
            }
            Requirement::Forbidden => {}
        }
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn kind_schema(field: &FieldSpec) -> Value {
    match field.kind {
        Kind::Enum => json!({"enum": field.values}),
        Kind::NullableEnum => {
            let mut values: Vec<Value> = field.values.iter().map(|v| json!(v)).collect();
            values.push(Value::Null);
            json!({"enum": values})
        }
        Kind::EnumList => json!({
            "type": "array",
            "minItems": 1,
            "maxItems": field.values.len(),
            "uniqueItems": true,
            "items": {"enum": field.values}
        }),
        Kind::SourceUrn => json!({"enum": field.values}),
        Kind::SourceList => json!({
            "type": "array",
            "minItems": 1,
            "maxItems": field.values.len().min(MAX_CONTROL_LIST),
            "uniqueItems": true,
            "items": {"enum": field.values}
        }),
        other => reference(other.as_str()),
    }
}

/// Escapes ECMA-262 syntax characters only (identity escapes of other
/// characters are invalid in unicode-mode patterns).
fn regex_escape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    for c in literal.chars() {
        if "^$\\.*+?()[]{}|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn subject_pattern(subject: &SubjectSpec) -> String {
    let mut pattern = String::from("^");
    for segment in &subject.segments {
        match segment {
            Segment::Literal(text) => pattern.push_str(&regex_escape(text)),
            Segment::ResourceId | Segment::ResourceVersionId | Segment::Detail(_) => {
                pattern.push_str(UUID_PATTERN);
            }
        }
    }
    pattern.push('$');
    pattern
}

fn resource_schema(spec: &EventSpec) -> Value {
    let types: Vec<&str> = spec.resources.iter().map(|r| r.as_str()).collect();
    let has_store = spec.resources.contains(&ResourceType::AuditStore);
    let uuid = if spec.nil_resource_allowed {
        reference("canonical_uuid")
    } else {
        reference("uuid")
    };
    let id = match (has_store, spec.resources.len()) {
        (true, 1) => json!({"const": AUDIT_STORE_RESOURCE_ID}),
        (true, _) => json!({"anyOf": [{"const": AUDIT_STORE_RESOURCE_ID}, uuid]}),
        (false, _) => uuid,
    };
    let mut properties = Map::new();
    properties.insert("type".to_owned(), json!({"enum": types}));
    properties.insert("id".to_owned(), id);
    let mut required = vec!["type", "id"];
    match spec.version_required {
        VersionRequirement::Required => {
            properties.insert("version_id".to_owned(), reference("uuid"));
            required.push("version_id");
        }
        VersionRequirement::Optional => {
            properties.insert("version_id".to_owned(), reference("uuid"));
        }
        VersionRequirement::Forbidden => {}
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn correlation_schema(spec: &EventSpec, adapter: &AdapterSpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (member, field) in [
        ("operation_id", &spec.operation_id_field),
        ("publish_operation_id", &spec.publish_operation_id_field),
    ] {
        if let Some(name) = field {
            properties.insert(member.to_owned(), reference("uuid"));
            if spec.required.contains(name) {
                required.push(member);
            }
        }
    }
    properties.insert("source_correlation_id".to_owned(), reference("uuid"));
    if adapter.trace_id {
        properties.insert("trace_id".to_owned(), reference("w3c_trace_id"));
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn details_schema(spec: &EventSpec) -> Value {
    let properties: Map<String, Value> = spec
        .detail_fields()
        .map(|(name, field)| (name.to_owned(), kind_schema(field)))
        .collect();
    let required: Vec<&str> = spec.detail_required().collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn event_schema(spec: &EventSpec, adapter: &AdapterSpec) -> Value {
    let mut properties = Map::new();
    let mut required = vec![
        "schema_version",
        "event_class",
        "action",
        "actor",
        "resource",
        "result",
        "correlation",
        "details",
        "extensions",
        "provenance",
    ];
    let mut put = |name: &str, schema: Value| {
        properties.insert(name.to_owned(), schema);
    };
    put("schema_version", json!({"const": SCHEMA_VERSION}));
    put("event_class", json!({"const": spec.event_class.as_str()}));
    put("action", json!({"const": spec.event_type}));
    put("actor", reference("actor"));
    if spec.service_executor_field.is_some() {
        put("service_executor", reference("actor"));
    }
    put("resource", resource_schema(spec));
    put("result", json!({"enum": spec.results}));
    if let Some(field) = spec
        .reason_code_field
        .as_ref()
        .and_then(|f| spec.fields.get(f))
    {
        put("reason_code", kind_schema(field));
        required.push("reason_code");
    }
    if spec.reason == ReasonPolicy::CallerText {
        put("reason", reference("reason_summary"));
        required.push("reason");
    }
    put("correlation", correlation_schema(spec, adapter));
    put("details", details_schema(spec));
    put("extensions", reference("extensions"));
    put("provenance", reference(&provenance_name(adapter)));
    let subjects: Vec<Value> = spec
        .subjects
        .iter()
        .map(|subject| {
            let mut alternative = json!({
                "properties": {"subject": {"pattern": subject_pattern(subject)}}
            });
            if let Some(only) = subject.resource {
                alternative["properties"]["data"] = json!({
                    "properties": {"resource": {"properties": {"type": {"const": only.as_str()}}}}
                });
            }
            alternative
        })
        .collect();
    json!({
        "properties": {
            "source": {"const": spec.source},
            "data": {
                "type": "object",
                "additionalProperties": false,
                "required": required,
                "properties": properties
            }
        },
        "anyOf": subjects
    })
}

/// Generates the envelope schema (members in byte order).
pub fn generate_json_schema(catalog: &Catalog) -> Value {
    let mut defs = shared_definitions();
    for adapter in catalog.adapters() {
        defs.insert(provenance_name(adapter), provenance_schema(adapter));
    }
    let mut branches = Vec::new();
    let mut types = Vec::new();
    for spec in catalog.events() {
        let Some(adapter) = catalog.adapter(&spec.source) else {
            unreachable!("the loader guarantees an adapter per source");
        };
        let name = format!("event.{}", spec.event_type);
        branches.push(json!({
            "if": {"properties": {"type": {"const": spec.event_type}}, "required": ["type"]},
            "then": reference(&name)
        }));
        types.push(spec.event_type.as_str());
        defs.insert(name, event_schema(spec, adapter));
    }
    let text = json!({"type": "string", "minLength": 1, "maxLength": MAX_STRING_BYTES});
    canonicalize(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": SCHEMA_ID,
        "title": "Knowledge Platform audit envelope (CloudEvents 1.0.2 structured JSON, payload v1)",
        "description": "Generated by audit-core from spec/telemetry/audit-event-catalog.json. Do not edit; regenerate with AUDIT_SCHEMA_BLESS=1 cargo test -p audit-core --test schema_contract. Rust-only constraints are listed in spec/telemetry/README.md.",
        "type": "object",
        "additionalProperties": false,
        "required": ATTRIBUTES,
        "properties": {
            "specversion": {"const": SPECVERSION},
            "id": reference("uuid"),
            "source": text,
            "type": {"enum": types},
            "subject": text,
            "time": reference("utc_timestamp"),
            "datacontenttype": {"const": DATACONTENTTYPE},
            "dataschema": {"const": DATASCHEMA},
            "data": {"type": "object"}
        },
        "allOf": branches,
        "$defs": defs
    }))
}

/// Pretty rendering with a trailing newline, as committed in the repository.
pub fn render_json_schema(catalog: &Catalog) -> String {
    let mut text = serde_json::to_string_pretty(&generate_json_schema(catalog))
        .unwrap_or_else(|_| unreachable!("a JSON value always serializes"));
    text.push('\n');
    text
}
