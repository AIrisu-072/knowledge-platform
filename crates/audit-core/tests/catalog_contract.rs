//! Catalog self-consistency and loader validation.

mod common;

use std::collections::BTreeSet;

use audit_core::catalog::{
    AUDIT_RELAY_SOURCE, AUDIT_STORE_SOURCE, CatalogError, DOCUMENT_SOURCE, ReasonPolicy,
    VersionRequirement,
};
use audit_core::envelope::LEGACY_SOURCE_FORMAT;
use audit_core::{Catalog, EventClass, LEGACY_ADAPTER_VERSION, Origin, Requirement};
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

/// Design §4.5 control events. `checkpoint` is recorded as
/// `audit.integrity.verified` with `trigger: "checkpoint"` (explicit reuse);
/// planned moves are `audit.recovery.epoch_started` with
/// `classification: "planned_move"` and an empty lost range.
const STORE_CONTROL_TYPES: [&str; 11] = [
    "audit.access.intent_opened",
    "audit.access.denied",
    "audit.access.closed",
    "audit.access_policy.changed",
    "audit.retention.policy_changed",
    "audit.retention.expired",
    "audit.retention.expire_refused",
    "audit.body.purged",
    "audit.integrity.verified",
    "audit.integrity.conflict_detected",
    "audit.recovery.epoch_started",
];
const RELAY_CONTROL_TYPES: [&str; 3] = [
    "audit.delivery.replay_requested",
    "audit.reconciliation.completed",
    "audit.integrity.source_mismatch_detected",
];

fn types_of(origin: Origin) -> BTreeSet<&'static str> {
    Catalog::embedded()
        .events()
        .iter()
        .filter(|e| e.origin == origin)
        .map(|e| e.event_type.as_str())
        .collect()
}

#[test]
fn embedded_catalog_lists_exactly_the_document_producer_types() {
    let catalog = Catalog::embedded();
    assert_eq!(catalog.version(), 1);
    assert_eq!(
        types_of(Origin::Relay),
        DOCUMENT_TYPES.into_iter().collect()
    );
    for spec in catalog
        .events()
        .iter()
        .filter(|e| e.origin == Origin::Relay)
    {
        assert_eq!(spec.source, audit_core::catalog::DOCUMENT_SOURCE);
        assert!(catalog.get(&spec.event_type).is_some());
    }
    assert_eq!(
        catalog.events().len(),
        DOCUMENT_TYPES.len() + STORE_CONTROL_TYPES.len() + RELAY_CONTROL_TYPES.len()
    );
}

#[test]
fn control_types_are_registered_with_their_origin_source_and_resource() {
    let catalog = Catalog::embedded();
    assert_eq!(
        types_of(Origin::Store),
        STORE_CONTROL_TYPES.into_iter().collect()
    );
    assert_eq!(
        types_of(Origin::RelayControl),
        RELAY_CONTROL_TYPES.into_iter().collect()
    );
    for spec in catalog
        .events()
        .iter()
        .filter(|e| e.origin != Origin::Relay)
    {
        let source = match spec.origin {
            Origin::Store => audit_core::catalog::AUDIT_STORE_SOURCE,
            _ => audit_core::catalog::AUDIT_RELAY_SOURCE,
        };
        assert_eq!(spec.source, source, "{}", spec.event_type);
        assert_eq!(
            spec.resources,
            vec![audit_core::ResourceType::AuditStore],
            "{}",
            spec.event_type
        );
        assert_eq!(spec.version_required, VersionRequirement::Forbidden);
        assert_eq!(spec.reason, ReasonPolicy::Absent);
        assert_eq!(spec.subjects.len(), 1);
        assert_eq!(spec.subjects[0].template, "audit-store");
        assert!(
            spec.required.iter().any(|f| f == "session_role"),
            "{}: control events record session_user",
            spec.event_type
        );
    }
}

#[test]
fn control_only_kinds_are_refused_on_relay_entries() {
    for kind in [
        "identifier",
        "nullable_identifier",
        "identifier_list",
        "code",
    ] {
        let result = edited(|e| e["fields"]["name"] = json!({"kind": kind}));
        assert!(result.is_err(), "{kind} must be control-only");
    }
}

#[test]
fn event_classes_follow_the_design_assignment() {
    let catalog = Catalog::embedded();
    for spec in catalog.events() {
        let expected = match spec.event_type.as_str() {
            "access_policy.changed" | "audit.access_policy.changed" => EventClass::AccessPolicy,
            "authorization.denied"
            | "audit.access.denied"
            | "audit.integrity.conflict_detected"
            | "audit.integrity.source_mismatch_detected" => EventClass::Security,
            "document.version.read_confirmed"
            | "document.file.access_granted"
            | "document.diff.result_access_granted"
            | "document.revision_comparison.result_access_granted"
            | "audit.access.intent_opened"
            | "audit.access.closed" => EventClass::DataAccess,
            "audit.retention.policy_changed" => EventClass::Configuration,
            "audit.retention.expired"
            | "audit.retention.expire_refused"
            | "audit.body.purged"
            | "audit.delivery.replay_requested" => EventClass::PrivilegedOperation,
            "audit.integrity.verified"
            | "audit.recovery.epoch_started"
            | "audit.reconciliation.completed" => EventClass::SystemAudit,
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
    for spec in Catalog::embedded()
        .events()
        .iter()
        .filter(|e| e.origin == Origin::Relay)
    {
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

/// An adapter for every (source, origin) the entries use.
fn adapters_for(entries: &[Value]) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    entries
        .iter()
        .filter_map(|entry| {
            let source = entry["source"].as_str()?.to_owned();
            let origin = entry["origin"].as_str()?.to_owned();
            seen.insert(source.clone()).then(|| {
                let relay = origin == "relay";
                json!({
                    "source": source,
                    "origin": origin,
                    "source_format": format!("format-{}", seen.len()),
                    "adapter_version": 1,
                    "commitment": if relay { "required" } else { "forbidden" },
                    "registration": if relay { "required" } else { "forbidden" },
                    "trace_id": false
                })
            })
        })
        .collect()
}

fn load(entries: Vec<Value>) -> Result<Catalog, CatalogError> {
    let adapters = adapters_for(&entries);
    load_with(adapters, entries)
}

fn load_with(adapters: Vec<Value>, entries: Vec<Value>) -> Result<Catalog, CatalogError> {
    Catalog::from_json(&json!({"version": 1, "adapters": adapters, "events": entries}).to_string())
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
        Catalog::from_json(&json!({"version": 2, "adapters": [], "events": []}).to_string()),
        Err(CatalogError::Version)
    );
    assert!(matches!(
        Catalog::from_json(r#"{"version":1,"version":1,"adapters":[],"events":[]}"#),
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
fn every_source_has_one_adapter_matching_the_legacy_projection() {
    let catalog = Catalog::embedded();
    assert_eq!(catalog.adapters().len(), 3);
    let document = catalog.adapter(DOCUMENT_SOURCE).expect("document adapter");
    assert_eq!(document.origin, Origin::Relay);
    assert_eq!(document.source_format, LEGACY_SOURCE_FORMAT);
    assert_eq!(
        document.adapter_version, LEGACY_ADAPTER_VERSION,
        "bump the catalog adapter_version together with LEGACY_ADAPTER_VERSION"
    );
    assert_eq!(document.commitment, Requirement::Required);
    assert_eq!(document.registration, Requirement::Required);
    assert!(
        !document.trace_id,
        "trace_id is reserved on the legacy path"
    );
    for (source, origin, format) in [
        (AUDIT_STORE_SOURCE, Origin::Store, "audit-store-control-v1"),
        (
            AUDIT_RELAY_SOURCE,
            Origin::RelayControl,
            "audit-relay-control-v1",
        ),
    ] {
        let adapter = catalog.adapter(source).expect("control adapter");
        assert_eq!(adapter.origin, origin);
        assert_eq!(adapter.source_format, format);
        assert_eq!(adapter.adapter_version, 1);
        assert_eq!(adapter.commitment, Requirement::Forbidden);
        assert_eq!(adapter.registration, Requirement::Forbidden);
        assert!(!adapter.trace_id);
    }
    for spec in catalog.events() {
        assert_eq!(
            catalog.adapter(&spec.source).map(|a| a.origin),
            Some(spec.origin),
            "{}",
            spec.event_type
        );
    }
}

#[test]
fn registered_types_are_the_relay_types_with_their_adapter_version() {
    let registered = Catalog::embedded().registered_types();
    assert_eq!(registered.len(), DOCUMENT_TYPES.len());
    for (source, event_type, version) in &registered {
        assert_eq!(*source, DOCUMENT_SOURCE);
        assert_eq!(*version, LEGACY_ADAPTER_VERSION);
        assert!(DOCUMENT_TYPES.contains(event_type), "{event_type}");
    }
    assert!(
        registered.iter().all(|(_, t, _)| !t.starts_with("audit.")),
        "control types are never ingested"
    );
}

#[test]
fn loader_rejects_inconsistent_adapters() {
    let relay = || {
        json!({
            "source": "urn:knowledge-platform:document-platform",
            "origin": "relay",
            "source_format": "document-audit-outbox-v0",
            "adapter_version": 1,
            "commitment": "required",
            "registration": "required",
            "trace_id": false
        })
    };
    load_with(vec![relay()], vec![minimal_entry()]).expect("baseline loads");
    let store_entry = json!({
        "type": "audit.thing.done",
        "source": "urn:knowledge-platform:audit-store",
        "origin": "store",
        "event_class": "SYSTEM_AUDIT",
        "resources": ["AuditStore"],
        "version_required": false,
        "results": ["success"],
        "subjects": ["audit-store"],
        "fields": {"session_role": {"kind": "identifier"}},
        "required": ["session_role"],
        "reason": "absent"
    });
    let store = |edit: &dyn Fn(&mut Value)| {
        let mut adapter = json!({
            "source": "urn:knowledge-platform:audit-store",
            "origin": "store",
            "source_format": "audit-store-control-v1",
            "adapter_version": 1,
            "commitment": "forbidden",
            "registration": "forbidden",
            "trace_id": false
        });
        edit(&mut adapter);
        adapter
    };
    load_with(
        vec![relay(), store(&|_| {})],
        vec![minimal_entry(), store_entry.clone()],
    )
    .expect("store adapter loads");
    let edited_relay = |edit: &dyn Fn(&mut Value)| {
        let mut adapter = relay();
        edit(&mut adapter);
        vec![adapter]
    };
    let bad: Vec<(&str, Vec<Value>, Vec<Value>)> = vec![
        ("no adapters", vec![], vec![minimal_entry()]),
        (
            "duplicate source",
            vec![relay(), relay()],
            vec![minimal_entry()],
        ),
        (
            "unused adapter",
            vec![relay(), store(&|_| {})],
            vec![minimal_entry()],
        ),
        (
            "event without adapter",
            vec![relay()],
            vec![minimal_entry(), store_entry.clone()],
        ),
        (
            "adapter version zero",
            edited_relay(&|a| a["adapter_version"] = json!(0)),
            vec![minimal_entry()],
        ),
        (
            "source format charset",
            edited_relay(&|a| a["source_format"] = json!("Document Outbox")),
            vec![minimal_entry()],
        ),
        (
            "unknown requirement",
            edited_relay(&|a| a["commitment"] = json!("sometimes")),
            vec![minimal_entry()],
        ),
        (
            "unknown adapter member",
            edited_relay(&|a| a["extra"] = json!(true)),
            vec![minimal_entry()],
        ),
        (
            "relay adapter with control source",
            edited_relay(&|a| a["source"] = json!("urn:knowledge-platform:audit-relay")),
            vec![minimal_entry()],
        ),
        (
            "origin differs from the event",
            edited_relay(&|a| a["origin"] = json!("store")),
            vec![minimal_entry()],
        ),
        (
            "control adapter with commitment",
            vec![relay(), store(&|a| a["commitment"] = json!("optional"))],
            vec![minimal_entry(), store_entry.clone()],
        ),
        (
            "control adapter with trace id",
            vec![relay(), store(&|a| a["trace_id"] = json!(true))],
            vec![minimal_entry(), store_entry.clone()],
        ),
        (
            "duplicate source format",
            vec![
                relay(),
                store(&|a| a["source_format"] = json!("document-audit-outbox-v0")),
            ],
            vec![minimal_entry(), store_entry.clone()],
        ),
    ];
    for (label, adapters, entries) in bad {
        assert!(
            load_with(adapters, entries).is_err(),
            "{label} must be refused"
        );
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
