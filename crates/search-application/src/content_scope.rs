//! P1-A02 request-scoped content scope. A trusted adapter builds it from an
//! authenticated request; external callers never choose raw files, locators,
//! profiles or coverage.

use search_core::id::ClaimId;

use crate::SearchError;
use crate::ports::{LexicalFieldScope, LexicalQuery};

/// Upper bound shared with the lexical window contract.
pub const MAX_BODY_QUERY_LIMIT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodySearchSpec {
    /// Executed only as a BodyOnly lexical query over Source-owned Units.
    pub query: LexicalQuery,
    /// Present only when the trusted caller registered an exact-text selector.
    pub exact_text_claim: Option<ClaimId>,
}

impl BodySearchSpec {
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.query.field_scope != LexicalFieldScope::BodyOnly
            || self.query.text.trim().is_empty()
            || self.query.limit == 0
            || self.query.limit > MAX_BODY_QUERY_LIMIT
        {
            return Err(SearchError::InvalidRequest(
                "body search requires a BodyOnly query with nonempty text and a limit of 1..=256"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiscoveryScope {
    #[default]
    Normal,
    BodyRequired(BodySearchSpec),
}

impl DiscoveryScope {
    pub fn body(&self) -> Option<&BodySearchSpec> {
        match self {
            Self::Normal => None,
            Self::BodyRequired(spec) => Some(spec),
        }
    }
}
