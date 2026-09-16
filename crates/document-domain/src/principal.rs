use serde::{Deserialize, Serialize};

use crate::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalRef {
    identity_provider: String,
    principal_id: String,
}

impl PrincipalRef {
    pub fn new(
        identity_provider: impl Into<String>,
        principal_id: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let identity_provider = identity_provider.into();
        let principal_id = principal_id.into();
        let identity_provider = identity_provider.trim();
        let principal_id = principal_id.trim();

        if identity_provider.is_empty() {
            return Err(DomainError::BlankIdentityProvider);
        }
        if principal_id.is_empty() {
            return Err(DomainError::BlankPrincipalId);
        }

        Ok(Self {
            identity_provider: identity_provider.to_owned(),
            principal_id: principal_id.to_owned(),
        })
    }

    pub fn identity_provider(&self) -> &str {
        &self.identity_provider
    }

    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }
}
