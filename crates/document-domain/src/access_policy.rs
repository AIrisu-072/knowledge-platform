use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    ReadHistory,
    Write,
    Publish,
    Administer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySubjectKind {
    Principal,
    Group,
    Role,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PolicySubject {
    kind: PolicySubjectKind,
    identity_provider: String,
    subject_id: String,
}

impl PolicySubject {
    pub fn new(
        kind: PolicySubjectKind,
        identity_provider: impl Into<String>,
        subject_id: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let identity_provider = identity_provider.into();
        let subject_id = subject_id.into();
        let identity_provider = identity_provider.trim();
        let subject_id = subject_id.trim();
        if identity_provider.is_empty()
            || subject_id.is_empty()
            || identity_provider.chars().any(char::is_control)
            || subject_id.chars().any(char::is_control)
        {
            return Err(DomainError::InvalidPolicySubject);
        }
        Ok(Self {
            kind,
            identity_provider: identity_provider.to_owned(),
            subject_id: subject_id.to_owned(),
        })
    }

    pub const fn kind(&self) -> PolicySubjectKind {
        self.kind
    }

    pub fn identity_provider(&self) -> &str {
        &self.identity_provider
    }

    pub fn subject_id(&self) -> &str {
        &self.subject_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyGrant {
    subject: PolicySubject,
    actions: BTreeSet<Action>,
}

impl PolicyGrant {
    pub fn new(
        subject: PolicySubject,
        actions: impl IntoIterator<Item = Action>,
    ) -> Result<Self, DomainError> {
        let mut unique = BTreeSet::new();
        for action in actions {
            if !unique.insert(action) {
                return Err(DomainError::InvalidPolicyGrant);
            }
        }
        if unique.is_empty() {
            return Err(DomainError::InvalidPolicyGrant);
        }
        Ok(Self {
            subject,
            actions: unique,
        })
    }

    pub const fn subject(&self) -> &PolicySubject {
        &self.subject
    }

    pub const fn actions(&self) -> &BTreeSet<Action> {
        &self.actions
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyMode {
    Inherit,
    Explicit(Vec<PolicyGrant>),
}

impl PolicyMode {
    pub fn validate_explicit(grants: &[PolicyGrant]) -> Result<(), DomainError> {
        if grants.is_empty() {
            return Err(DomainError::EmptyExplicitPolicy);
        }
        let mut subjects = BTreeSet::new();
        for grant in grants {
            if grant.actions.is_empty() {
                return Err(DomainError::InvalidPolicyGrant);
            }
            if !subjects.insert(&grant.subject) {
                return Err(DomainError::DuplicatePolicySubject);
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::Inherit => Ok(()),
            Self::Explicit(grants) => Self::validate_explicit(grants),
        }
    }
}

pub fn nearest_explicit_policy<'a>(
    path_from_resource: impl IntoIterator<Item = &'a PolicyMode>,
) -> Option<&'a PolicyMode> {
    path_from_resource
        .into_iter()
        .find(|mode| matches!(mode, PolicyMode::Explicit(_)))
}

pub fn evaluate_policy(
    subjects: &[PolicySubject],
    effective: &PolicyMode,
    required: &[Action],
) -> bool {
    let PolicyMode::Explicit(grants) = effective else {
        return false;
    };
    if grants.is_empty() || required.is_empty() {
        return false;
    }
    required.iter().all(|action| {
        grants
            .iter()
            .any(|grant| subjects.contains(&grant.subject) && grant.actions.contains(action))
    })
}
