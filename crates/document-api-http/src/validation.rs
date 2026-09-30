use std::collections::HashMap;

use jsonschema::Validator;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    Compile(String),
    UnknownSchema,
    ActorClaimInPayload,
    Invalid,
}

pub struct SchemaRegistry {
    validators: HashMap<String, Validator>,
}

impl SchemaRegistry {
    pub fn compile<K: AsRef<str>>(
        schemas: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<Self, SchemaError> {
        let mut validators = HashMap::new();
        for (name, schema) in schemas {
            let validator = jsonschema::draft202012::new(&schema)
                .map_err(|error| SchemaError::Compile(error.to_string()))?;
            validators.insert(name.as_ref().to_owned(), validator);
        }
        Ok(Self { validators })
    }

    pub fn validate_request(&self, name: &str, input: &Value) -> Result<(), SchemaError> {
        if has_top_level_actor_claim(input) {
            return Err(SchemaError::ActorClaimInPayload);
        }
        self.validate(name, input)
    }

    pub fn validate_response(&self, name: &str, output: &Value) -> Result<(), SchemaError> {
        self.validate(name, output)
    }

    fn validate(&self, name: &str, value: &Value) -> Result<(), SchemaError> {
        let validator = self
            .validators
            .get(name)
            .ok_or(SchemaError::UnknownSchema)?;
        if validator.is_valid(value) {
            Ok(())
        } else {
            Err(SchemaError::Invalid)
        }
    }
}

fn has_top_level_actor_claim(value: &Value) -> bool {
    const FORBIDDEN: &[&str] = &[
        "actor",
        "principal",
        "principalId",
        "principal_id",
        "identityProvider",
        "identity_provider",
        "group",
        "groups",
        "role",
        "roles",
        "invocationKind",
        "invocation_kind",
        "serviceExecutor",
        "service_executor",
        "delegation",
    ];
    value
        .as_object()
        .is_some_and(|object| FORBIDDEN.iter().any(|field| object.contains_key(*field)))
}
