use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata(Map<String, Value>);

impl Metadata {
    pub fn from_map(value: Map<String, Value>) -> Self {
        Self(value)
    }

    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }
}
