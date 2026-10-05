//! P4-10: verified remote evidence and canonical upstream lineage.
//!
//! A provider's evidence role, origin label, summary flag and citation text
//! are an `UntrustedEvidenceHint`, never evidence. Only the host-wired
//! provenance lookup of the fixed Source, matched against the pinned
//! Resource version and digest and the server-owned lineage registration,
//! produces a `VerifiedProvenance` (private constructor). Directness comes
//! from that lookup; the upstream origin is a registered canonical lineage
//! group, so two provider labels of one upstream count once and an unknown
//! lineage folds into the provider's single group. `Authoritative` needs an
//! explicit predicate grant in the registration and a matching version.

use std::collections::BTreeMap;
use std::fmt;

use search_core::assertion::AssertionOrigin;
use search_core::evidence::EvidenceRole;
use search_core::id::ResourceId;
use search_core::projection::ProjectionGenerationKey;

use crate::SearchError;
use crate::ports::{BoxFuture, ResolvedAssertionEvidence};
use crate::remote::PinnedRemoteTarget;
use crate::remote_registration::RemoteSourceRegistration;
use crate::scoped::AuthorizedSourceScope;

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

/// Provider-declared evidence metadata. Not evidence, not persistable and
/// never copied into audit or telemetry.
#[derive(Clone, PartialEq, Eq)]
pub struct UntrustedEvidenceHint {
    evidence_ref: String,
    role_label: Option<String>,
    origin_label: Option<String>,
    quoted: bool,
}

impl fmt::Debug for UntrustedEvidenceHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UntrustedEvidenceHint(<untrusted>)")
    }
}

impl UntrustedEvidenceHint {
    pub fn new(
        evidence_ref: impl Into<String>,
        role_label: Option<String>,
        origin_label: Option<String>,
        quoted: bool,
    ) -> Result<Self, SearchError> {
        let evidence_ref = evidence_ref.into();
        if !bounded(&evidence_ref, 512)
            || role_label.as_deref().is_some_and(|v| !bounded(v, 64))
            || origin_label.as_deref().is_some_and(|v| !bounded(v, 256))
        {
            return Err(SearchError::InvalidRequest(
                "remote evidence hint is out of bounds".into(),
            ));
        }
        Ok(Self {
            evidence_ref,
            role_label,
            origin_label,
            quoted,
        })
    }

    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

/// Server-owned canonical lineage groups of one remote registration. The
/// registration's own lineage is the default group for anything unproven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredLineage {
    default_group: String,
    groups: BTreeMap<String, String>,
}

impl RegisteredLineage {
    /// `groups` maps a verified lineage label to its opaque canonical group.
    pub fn new(
        registration: &RemoteSourceRegistration,
        groups: Vec<(String, String)>,
    ) -> Result<Self, SearchError> {
        let mut map = BTreeMap::new();
        for (label, group) in groups {
            if !bounded(&label, 256) || !bounded(&group, 256) || map.insert(label, group).is_some()
            {
                return Err(SearchError::InvalidRequest(
                    "registered lineage is invalid".into(),
                ));
            }
        }
        Ok(Self {
            default_group: registration.canonical_upstream_lineage().into(),
            groups: map,
        })
    }

    fn group(&self, verified_label: &str) -> &str {
        self.groups
            .get(verified_label)
            .map(String::as_str)
            .unwrap_or(&self.default_group)
    }
}

/// What the fixed Source's own provenance protocol verified for one
/// evidence reference of one pinned Resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSourceProvenance {
    /// The Source itself authored the cited evidence (not quoted or derived).
    pub direct: bool,
    pub summary: bool,
    pub version: Option<String>,
    pub digest: Option<String>,
    /// The lineage label as verified by the Source, not the provider hint.
    pub lineage_label: String,
    pub predicate: String,
    pub citation_chain: Vec<String>,
}

/// Host-wired provenance lookup of the fixed Source. Returning `None` keeps
/// the evidence unverified and the Claim unknown.
pub trait RemoteProvenanceLookupPort: Send + Sync {
    fn lookup<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        target: &'a PinnedRemoteTarget,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedSourceProvenance>>;
}

/// Private constructor: only `verify_provenance` creates it.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedProvenance {
    evidence_ref: String,
    role: EvidenceRole,
    is_summary: bool,
    upstream_origin: String,
    citation_chain: Vec<String>,
    content_digest: Option<String>,
    authoritative: bool,
}

impl fmt::Debug for VerifiedProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedProvenance(<verified>)")
    }
}

impl VerifiedProvenance {
    pub const fn role(&self) -> EvidenceRole {
        self.role
    }
    pub const fn is_summary(&self) -> bool {
        self.is_summary
    }
    pub fn upstream_origin(&self) -> &str {
        &self.upstream_origin
    }
    /// `Authoritative` only with a registered predicate grant and a direct,
    /// version-matched record; otherwise the assertion is only observed.
    pub const fn assertion_origin(&self) -> AssertionOrigin {
        if self.authoritative {
            AssertionOrigin::Authoritative
        } else {
            AssertionOrigin::Observed
        }
    }
}

pub async fn verify_provenance(
    registration: &RemoteSourceRegistration,
    lineage: &RegisteredLineage,
    scope: &AuthorizedSourceScope,
    target: &PinnedRemoteTarget,
    hint: &UntrustedEvidenceHint,
    lookup: &dyn RemoteProvenanceLookupPort,
) -> Result<Option<VerifiedProvenance>, SearchError> {
    if scope.source_id() != registration.source_id()
        || scope.registration_revision() != registration.registration_revision()
        || scope.visibility_revision() != registration.visibility_revision()
        || target.identity().source_scope() != scope
    {
        return Ok(None);
    }
    let Some(record) = lookup.lookup(scope, target, hint.evidence_ref()).await? else {
        return Ok(None);
    };
    // The record must describe exactly the pinned Resource version.
    if record.version.as_deref() != target.version() || record.digest.as_deref() != target.digest()
    {
        return Ok(None);
    }
    let direct = record.direct && !record.summary && !hint.quoted;
    // Provider role labels never elevate; a direct record may only say it
    // corroborates or contradicts instead of being the primary source.
    let role = if !direct {
        EvidenceRole::Contextual
    } else {
        match hint.role_label.as_deref() {
            Some("corroborating") => EvidenceRole::Corroborating,
            Some("contradicting") => EvidenceRole::Contradicting,
            _ => EvidenceRole::Primary,
        }
    };
    let authoritative = direct
        && record.version.is_some()
        && registration
            .authority_predicates()
            .contains(&record.predicate);
    Ok(Some(VerifiedProvenance {
        evidence_ref: hint.evidence_ref().into(),
        role,
        is_summary: !direct,
        upstream_origin: lineage.group(&record.lineage_label).into(),
        citation_chain: record.citation_chain,
        content_digest: record.digest,
        authoritative,
    }))
}

/// The only route from remote provenance into Claim evidence.
pub fn resolved_evidence(
    key: ProjectionGenerationKey,
    resource: ResourceId,
    verified: &VerifiedProvenance,
) -> ResolvedAssertionEvidence {
    ResolvedAssertionEvidence {
        generation: key,
        source_id: key.source_id,
        resource_id: resource,
        evidence_ref: verified.evidence_ref.clone(),
        upstream_origin: verified.upstream_origin.clone(),
        role: verified.role,
        citation_chain: verified.citation_chain.clone(),
        content_digest: verified.content_digest.clone(),
        is_summary: verified.is_summary,
    }
}
