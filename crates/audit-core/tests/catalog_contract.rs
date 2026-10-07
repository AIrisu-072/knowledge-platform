//! Catalog self-consistency and loader validation.

mod common;

use std::collections::BTreeSet;

use audit_core::catalog::{CatalogError, ReasonPolicy, VersionRequirement};
use audit_core::{Catalog, EventClass, Origin};
use common::accepted_fixtures;
use serde_json::{Value, json};

const DOCUMENT_TYPES: [&str; 21] = [
    "document.created",
    "document.version.created",
    "document.version.updated",
    "document.version.rebased",
    "document.version.published",
    "document.version.publication.scheduled",
    "document.version.publication.cancelled",
    "document.version.publication.terminal",
    "document.version.withdrawn",
    "document.publication.ended",
    "document.metadata.changed",
    "document.moved",
    "folder.created",
    "folder.renamed",
    "folder.moved",
    "access_policy.changed",
    "document.version.read_confirmed",
    "document.file.access_granted",
    "document.diff.result_access_granted",
    "document.revision_comparison.result_access_granted",
    "authorization.denied",
];

#[test]
fn embedded_catalog_lists_exactly_the_document_producer_types() {
    let catalog = Catalog::embedded();
    assert_eq!(catalog.version(), 1);
    let types: BTreeSet<&str> = catalog
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert_eq!(types, DOCUMENT_TYPES.into_iter().collect());
    for spec in catalog.events() {
        assert_eq!(spec.origin, Origin::Relay, "{}", spec.event_type);
        assert_eq!(spec.source, audit_core::catalog::DOCUMENT_SOURCE);
        assert!(catalog.get(&spec.event_type).is_some());
    }
    assert!(catalog.get("audit.access.denied").is_none());
}

#[test]
fn event_classes_follow_the_design_assignment() {
    let catalog = Catalog::embedded();
    for spec in catalog.events() {
        let expected = match spec.event_type.as_str() {
            "access_policy.changed" => EventClass::AccessPolicy,
            "authorization.denied" => EventClass::Security,
            "document.version.read_confirmed"
            | "document.file.access_granted"
            | "document.diff.result_access_granted"
            | "document.revision_comparison.result_access_granted" => EventClass::DataAccess,
            _ => EventClass::ContentLifecycle,
        };
        assert_eq!(spec.event_class, expected, "{}", spec.event_type);
    }
}

#[test]
fn reason_bearing_types_are_exactly_the_seven_caller_text_events() {
    let caller_text: BTreeSet<&str> = Catalog::embedded()
        .events()
        .iter()
        .filter(|e| e.reason == ReasonPolicy::CallerText)
        .map(|e| e.event_type.as_str())
        .collect();
    let expected: BTreeSet<&str> = [
        "document.version.withdrawn",
        "document.publication.ended",
        "document.metadata.changed",
        "document.moved",
        "folder.created",
        "folder.renamed",
        "folder.moved",
    ]
    .into_iter()
    .collect();
    assert_eq!(caller_text, expected);
}

#[test]
fn correlation_mapping_follows_design_4_2() {
    let catalog = Catalog::embedded();
    let op = |t: &str| catalog.get(t).expect(t).operation_id_field.clone();
    let publish = |t: &str| catalog.get(t).expect(t).publish_operation_id_field.clone();
    for t in [
        "document.version.published",
        "document.version.publication.scheduled",
        "document.version.publication.terminal",
    ] {
        assert_eq!(op(t).as_deref(), Some("publishOperationId"), "{t}");
        assert_eq!(publish(t).as_deref(), Some("publishOperationId"), "{t}");
    }
    let cancelled = "document.version.publication.cancelled";
    assert_eq!(
        op(cancelled),
        None,
        "cancel has no own operation id in the payload"
    );
    assert_eq!(publish(cancelled).as_deref(), Some("publishOperationId"));
    assert_eq!(
        op("document.publication.ended").as_deref(),
        Some("operationId")
    );
    for t in [
        "document.metadata.changed",
        "document.moved",
        "folder.created",
        "folder.renamed",
        "folder.moved",
        "access_policy.changed",
    ] {
        assert_eq!(op(t).as_deref(), Some("operation_id"), "{t}");
        assert_eq!(publish(t), None, "{t}");
    }
    let denied = catalog.get("authorization.denied").expect("denied");
    assert!(denied.nil_resource_allowed);
    assert_eq!(denied.reason_code_field.as_deref(), Some("reason_code"));
    assert_eq!(
        catalog
            .events()
            .iter()
            .filter(|e| e.nil_resource_allowed)
            .count(),
        1
    );
    assert_eq!(
        catalog
            .get("document.version.created")
            .expect("created")
            .version_required,
        VersionRequirement::Required
    );
}

#[test]
fn every_catalog_type_has_an_accepted_fixture() {
    let covered: BTreeSet<String> = accepted_fixtures()
        .into_iter()
        .map(|fixture| {
            audit_core::project(&fixture.row).unwrap_or_else(|r| panic!("{}: {r}", fixture.name));
            fixture.row.event_type
        })
        .collect();
    for spec in Catalog::embedded().events() {
        assert!(
            covered.contains(&spec.event_type),
            "{} has no fixture",
            spec.event_type
        );
        for subject in &spec.subjects {
            let used = accepted_fixtures().into_iter().any(|f| {
                f.row.event_type == spec.event_type
                    && audit_core::catalog::ResourceType::parse(&f.row.resource_type).is_some_and(
                        |r| {
                            subject.render(
                                r,
                                &f.row.resource_id,
                                f.row.resource_version_id.as_deref(),
                                f.row
                                    .data
                                    .as_ref()
                                    .and_then(Value::as_object)
                                    .expect("object"),
                            ) == Some(f.row.subject.clone())
                        },
                    )
            });
            assert!(
                used,
                "{}: subject {} has no fixture",
                spec.event_type, subject.template
            );
        }
        for resource in &spec.resources {
            assert!(
                accepted_fixtures()
                    .iter()
                    .any(|f| f.row.event_type == spec.event_type
                        && f.row.resource_type == resource.as_str()),
                "{}: resource {} has no fixture",
                spec.event_type,
                resource.as_str()
            );
        }
    }
}

type Edit = Box<dyn FnOnce(&mut Value)>;

fn minimal_entry() -> Value {
    json!({
        "type": "example.thing.done",
        "source": "urn:knowledge-platform:document-platform",
        "origin": "relay",
        "event_class": "CONTENT_LIFECYCLE",
        "resources": ["Document"],
        "version_required": "optional",
        "results": ["success"],
        "subjects": ["document/{resource.id}"],
        "fields": {"documentId": {"kind": "uuid"}},
        "required": ["documentId"],
        "reason": "absent"
    })
}

fn load(entries: Vec<Value>) -> Result<Catalog, CatalogError> {
    Catalog::from_json(&json!({"version": 1, "events": entries}).to_string())
}

fn edited(edit: impl FnOnce(&mut Value)) -> Result<Catalog, CatalogError> {
    let mut entry = minimal_entry();
    edit(&mut entry);
    load(vec![entry])
}

#[test]
fn loader_accepts_minimal_and_reserved_control_entries() {
    let catalog = load(vec![minimal_entry()]).expect("minimal entry");
    assert_eq!(
        catalog
            .get("example.thing.done")
            .expect("entry")
            .version_required,
        VersionRequirement::Optional
    );
    let control = json!({
        "type": "audit.integrity.verified",
        "source": "urn:knowledge-platform:audit-store",
        "origin": "store",
        "event_class": "SYSTEM_AUDIT",
        "resources": ["AuditStore"],
        "version_required": false,
        "results": ["success", "failure"],
        "subjects": ["audit-store/integrity"],
        "fields": {
            "from_seq": {"kind": "positive_counter"},
            "checked": {"kind": "counter"},
            "head_chain": {"kind": "hex_digest"},
            "event_ids": {"kind": "uuid_list"},
            "verified_at": {"kind": "utc_timestamp"},
            "outcome": {"kind": "enum", "values": ["ok", "violations"]},
            "lag": {"kind": "nullable_counter"}
        },
        "required": ["from_seq", "checked", "head_chain", "outcome"],
        "reason": "absent"
    });
    let relay_control = json!({
        "type": "audit.delivery.replay_requested",
        "source": "urn:knowledge-platform:audit-relay",
        "origin": "relay_control",
        "event_class": "PRIVILEGED_OPERATION",
        "resources": ["AuditStore"],
        "version_required": false,
        "results": ["success"],
        "subjects": ["audit-relay/replay/{details.event_id}"],
        "fields": {"event_id": {"kind": "uuid"}},
        "required": ["event_id"],
        "reason": "absent"
    });
    let catalog =
        load(vec![minimal_entry(), control, relay_control]).expect("control entries load");
    assert_eq!(
        catalog
            .get("audit.integrity.verified")
            .expect("store")
            .origin,
        Origin::Store
    );
    assert_eq!(
        catalog
            .get("audit.delivery.replay_requested")
            .expect("relay")
            .origin,
        Origin::RelayControl
    );
}

#[test]
fn loader_rejects_inconsistent_catalogs() {
    assert_eq!(
        Catalog::from_json(&json!({"version": 2, "events": []}).to_string()),
        Err(CatalogError::Version)
    );
    assert!(matches!(
        Catalog::from_json(r#"{"version":1,"version":1,"events":[]}"#),
        Err(CatalogError::Json("duplicate_key"))
    ));
    assert!(matches!(
        load(vec![minimal_entry(), minimal_entry()]),
        Err(CatalogError::Entry { .. })
    ));
    let bad: Vec<(&str, Edit)> = vec![
        (
            "required not in fields",
            Box::new(|e| e["required"] = json!(["missing"])),
        ),
        (
            "unknown kind",
            Box::new(|e| e["fields"]["documentId"] = json!({"kind": "text"})),
        ),
        (
            "enum without values",
            Box::new(|e| e["fields"]["x"] = json!({"kind": "enum"})),
        ),
        (
            "values on uuid",
            Box::new(|e| e["fields"]["documentId"]["values"] = json!(["a"])),
        ),
        ("empty subjects", Box::new(|e| e["subjects"] = json!([]))),
        (
            "unknown placeholder",
            Box::new(|e| e["subjects"] = json!(["document/{resource.name}"])),
        ),
        (
            "optional detail placeholder",
            Box::new(|e| {
                e["fields"]["other"] = json!({"kind": "uuid"});
                e["subjects"] = json!(["document/{details.other}"]);
            }),
        ),
        (
            "version placeholder without versions",
            Box::new(|e| {
                e["version_required"] = json!(false);
                e["subjects"] = json!(["document/{resource.id}/version/{resource.version_id}"]);
            }),
        ),
        (
            "unterminated placeholder",
            Box::new(|e| e["subjects"] = json!(["document/{resource.id"])),
        ),
        (
            "control type on relay",
            Box::new(|e| e["type"] = json!("audit.thing.done")),
        ),
        (
            "control source on relay",
            Box::new(|e| e["source"] = json!("urn:knowledge-platform:audit-store")),
        ),
        (
            "store origin with document source",
            Box::new(|e| e["origin"] = json!("store")),
        ),
        (
            "audit store resource on relay",
            Box::new(|e| e["resources"] = json!(["Document", "AuditStore"])),
        ),
        ("empty results", Box::new(|e| e["results"] = json!([]))),
        (
            "reason field",
            Box::new(|e| e["fields"]["reason"] = json!({"kind": "uuid"})),
        ),
        (
            "binding to optional version",
            Box::new(|e| {
                e["bindings"] = json!([{"field": "documentId", "equals": "resource.version_id"}]);
            }),
        ),
        (
            "binding unknown field",
            Box::new(|e| e["bindings"] = json!([{"field": "x", "equals": "resource.id"}])),
        ),
        (
            "reason code not enum",
            Box::new(|e| e["reason_code_field"] = json!("documentId")),
        ),
        (
            "service executor not principal",
            Box::new(|e| e["service_executor_field"] = json!("documentId")),
        ),
        (
            "operation id not uuid",
            Box::new(|e| {
                e["fields"]["n"] = json!({"kind": "counter"});
                e["operation_id_field"] = json!("n");
            }),
        ),
        ("unknown member", Box::new(|e| e["deferred"] = json!(true))),
        (
            "bad version_required",
            Box::new(|e| e["version_required"] = json!("sometimes")),
        ),
        (
            "subject resource outside resources",
            Box::new(|e| {
                e["subjects"] = json!([{"template": "folder/{resource.id}", "resource": "Folder"}]);
            }),
        ),
        (
            "uppercase literal",
            Box::new(|e| e["subjects"] = json!(["Document/{resource.id}"])),
        ),
    ];
    for (label, edit) in bad {
        assert!(edited(edit).is_err(), "{label} must be refused");
    }
}

#[test]
fn embedded_text_is_the_spec_file() {
    let on_disk = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/telemetry/audit-event-catalog.json"
    ))
    .expect("catalog file");
    assert_eq!(Catalog::embedded_text(), on_disk);
}
