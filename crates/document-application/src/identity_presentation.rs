use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentityKind {
    Principal,
    Group,
    Role,
}

impl IdentityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Principal => "principal",
            Self::Group => "group",
            Self::Role => "role",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityRef {
    pub provider: String,
    pub kind: IdentityKind,
    pub subject_id: String,
}

impl IdentityRef {
    pub fn from_principal(principal: &PrincipalRef) -> Self {
        Self {
            provider: principal.identity_provider().to_owned(),
            kind: IdentityKind::Principal,
            subject_id: principal.principal_id().to_owned(),
        }
    }

    pub fn from_policy_subject(subject: &PolicySubject) -> Self {
        Self {
            provider: subject.identity_provider().to_owned(),
            kind: match subject.kind() {
                PolicySubjectKind::Principal => IdentityKind::Principal,
                PolicySubjectKind::Group => IdentityKind::Group,
                PolicySubjectKind::Role => IdentityKind::Role,
            },
            subject_id: subject.subject_id().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityPresentationResolution {
    Resolved,
    NotFound,
    Unavailable,
}

impl IdentityPresentationResolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::NotFound => "notFound",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityPresentation {
    pub reference: IdentityRef,
    pub display_name: Option<String>,
    pub secondary_text: Option<String>,
    pub resolution: IdentityPresentationResolution,
}

impl IdentityPresentation {
    pub fn unavailable(reference: IdentityRef) -> Self {
        Self {
            reference,
            display_name: None,
            secondary_text: None,
            resolution: IdentityPresentationResolution::Unavailable,
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum IdentityPresentationResolutionError {
    #[error("identity presentation provider is temporarily unavailable")]
    Unavailable,
}

pub trait IdentityPresentationResolver: Send + Sync {
    fn resolve_batch<'a>(
        &'a self,
        refs: &'a [IdentityRef],
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>,
                > + Send
                + 'a,
        >,
    >;
}

pub struct IdentityPresentationService;

impl IdentityPresentationService {
    /// Resolve a page's identities in one batch and fail soft to stable machine refs.
    pub async fn resolve_batch<R: IdentityPresentationResolver + ?Sized>(
        resolver: &R,
        refs: &[IdentityRef],
    ) -> Vec<IdentityPresentation> {
        let mut unique_refs = Vec::with_capacity(refs.len());
        let mut seen = HashSet::with_capacity(refs.len());
        for identity_ref in refs {
            if seen.insert(identity_ref.clone()) {
                unique_refs.push(identity_ref.clone());
            }
        }
        if unique_refs.is_empty() {
            return Vec::new();
        }

        let requested = unique_refs.iter().cloned().collect::<HashSet<_>>();
        let candidates = resolver
            .resolve_batch(&unique_refs)
            .await
            .unwrap_or_default();
        let mut presentations = HashMap::with_capacity(unique_refs.len());
        for mut candidate in candidates {
            if !requested.contains(&candidate.reference) {
                continue;
            }
            if candidate.resolution != IdentityPresentationResolution::Resolved {
                candidate.display_name = None;
                candidate.secondary_text = None;
            }
            presentations
                .entry(candidate.reference.clone())
                .or_insert(candidate);
        }

        refs.iter()
            .map(|identity_ref| {
                presentations
                    .get(identity_ref)
                    .cloned()
                    .unwrap_or_else(|| IdentityPresentation::unavailable(identity_ref.clone()))
            })
            .collect()
    }
}
