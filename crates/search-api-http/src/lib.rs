//! Search HTTP adapter: the four v0 routes over the Search application.
//!
//! Axum/Tower and serde stay in this crate. `problem` is the only writer of
//! Search Problem responses, `dto` the only public JSON mapping, and every
//! route shares one verified actor, one visible catalog and the same core
//! services. Nothing from a request body or header becomes a principal,
//! tenant, grant or invocation kind.

#![forbid(unsafe_code)]

pub mod auth;
pub mod dto;
pub mod limits;
pub mod problem;
pub mod router;
pub mod send;

pub use auth::{
    AuthConfigurationError, CredentialError, SearchAuthChallengePort, SearchAuthSchemeBinding,
    SearchCredentialVerifierPort, StaticBearerChallenge, ValidatedChallenge,
};
pub use router::{
    ApiFuture, SearchApiBackend, SearchOperation, SearchRouterConfig, StartupError,
    build_search_router,
};
pub use send::{CloseReason, ConnectionLeases, SendLease, SendObserver, ServeOptions, serve};
