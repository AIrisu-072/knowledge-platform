//! Progressive, runtime-only content access is distinct from Source retention.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaterializationState {
    ReferenceOnly,
    Metadata,
    Probed,
    Fragment,
    FullContent,
}

impl MaterializationState {
    /// A policy may skip stages, but a later result cannot erase an earlier stage.
    pub const fn can_advance_to(self, next: Self) -> bool {
        (next as u8) >= (self as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterializationPolicy {
    InlineFull,
    DiscriminativeFirst,
    TargetedFragment,
    ReferenceOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeExecutionLocation {
    Local,
    Provider,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeQueryMode {
    FacetExact,
    FreeText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeReturnType {
    Facts,
    CandidateReferences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeCompletenessSemantics {
    CompleteForQuery,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeOutcome {
    Found,
    NotFoundByProbe,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderContentPermission {
    ReferenceOnly,
    Metadata,
    Fragment,
    FullContent,
}

impl ProviderContentPermission {
    pub const fn permits(self, state: MaterializationState) -> bool {
        match (self, state) {
            (_, MaterializationState::ReferenceOnly) => true,
            (
                Self::Metadata | Self::Fragment | Self::FullContent,
                MaterializationState::Metadata,
            ) => true,
            (Self::Fragment | Self::FullContent, MaterializationState::Fragment) => true,
            (Self::FullContent, MaterializationState::FullContent) => true,
            // Probe execution has its own capability and permission contract.
            (_, MaterializationState::Probed) => false,
            _ => false,
        }
    }
}
