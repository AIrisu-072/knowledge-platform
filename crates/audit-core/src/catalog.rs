use crate::ValidationError;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalog {
    pub version: u32,
    pub events: Vec<EventRule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventRule {
    pub r#type: String,
    pub source: String,
    pub category: String,
    pub resources: Vec<String>,
    pub result: String,
    pub requires_version: bool,
    pub deferred: bool,
    pub fields: BTreeMap<String, FieldRule>,
    pub required: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldRule {
    pub kind: String,
    #[serde(default)]
    pub values: Vec<Value>,
}

pub(crate) fn catalog() -> Result<&'static Catalog, ValidationError> {
    static CATALOG: OnceLock<Result<Catalog, ValidationError>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog: Catalog = serde_json::from_str(include_str!(
                "../../../spec/telemetry/audit-event-catalog.json"
            ))
            .map_err(|_| ValidationError::InvalidCatalog)?;
            let mut unique = BTreeSet::new();
            if catalog.version != 1
                || catalog.events.iter().any(|event| {
                    !unique.insert(&event.r#type)
                        || event
                            .required
                            .iter()
                            .any(|key| !event.fields.contains_key(key))
                })
            {
                return Err(ValidationError::InvalidCatalog);
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(|e| *e)
}
pub(crate) fn rule(kind: &str) -> Result<&'static EventRule, ValidationError> {
    catalog()?
        .events
        .iter()
        .find(|rule| rule.r#type == kind)
        .ok_or(ValidationError::UnsupportedEvent)
}
