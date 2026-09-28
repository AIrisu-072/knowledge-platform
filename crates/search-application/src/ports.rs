//! Backend-neutral, asynchronous ports for Source-owned reads and derived Search state.

use std::future::Future;
use std::pin::Pin;

use search_core::assertion::Assertion;
use search_core::binding::RepresentationBinding;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::fact::FactSet;
use search_core::graph::GraphTraversalPlan;
use search_core::id::{ResourceId, SourceId};
use search_core::predicate::TruthValue;
use search_core::source::DiscoverableSource;

use crate::error::SearchError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SearchError>> + Send + 'a>>;

pub trait SourceRegistryPort: Send + Sync {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>>;
    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>>;
}

pub trait AssertionStorePort: Send + Sync {
    fn assertions_for<'a>(
        &'a self,
        resource_ref: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>>;
}

pub trait ConceptRegistryPort: Send + Sync {
    fn same_concept<'a>(&'a self, left: &'a str, right: &'a str) -> BoxFuture<'a, TruthValue>;
    fn is_a<'a>(&'a self, child: &'a str, parent: &'a str) -> BoxFuture<'a, TruthValue>;
}

pub trait DirectoryRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        source_id: SourceId,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait StructuredRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        source_id: SourceId,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait LexicalRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        source_id: SourceId,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait VectorRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        source_id: SourceId,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait HyperGraphRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait ProbePort: Send + Sync {
    fn probe<'a>(&'a self, candidate: &'a FederatedCandidate) -> BoxFuture<'a, FactSet>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Denied,
    Unknown,
}

pub trait CurrentAccessEvaluatorPort: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializationReceipt {
    pub resource_ref: ResourceId,
    pub content_digest: Option<String>,
    pub locator: Option<String>,
}

pub trait MaterializerPort: Send + Sync {
    fn materialize<'a>(
        &'a self,
        binding: &'a RepresentationBinding,
        access_context: &'a str,
    ) -> BoxFuture<'a, MaterializationReceipt>;
}
