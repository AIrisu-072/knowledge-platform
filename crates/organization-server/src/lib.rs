#![forbid(unsafe_code)]
//! Synthetic Organization composition reuses the unchanged Document authority.
mod config;
mod identity;
pub use config::OrganizationConfig;
pub use identity::{OrganizationProfile, SyntheticIdentityAdapter};
mod composition;
pub use composition::compose_routes;
mod bootstrap;
pub use bootstrap::{bootstrap_document_policy, organization_root_grants, verify_shared_document};
mod document_evidence;
pub use document_evidence::{DocumentAgentSource, DocumentEvidenceSource};
mod synthetic_agent;
pub use synthetic_agent::OwnedAgentDispatcher;
