//! Fixed-origin HTTP adapter for one registered remote Source.
//!
//! The only production dependency direction is `search-source-http` →
//! `search-application` → `search-core`. The transport reaches exactly the
//! operator-registered HTTPS origin and paths; provider data is decoded as
//! untrusted input and returned to the application only through its checked
//! observation adapter.

#[cfg(all(feature = "synthetic-loopback-test-only", not(debug_assertions)))]
compile_error!("synthetic-loopback-test-only must never be enabled in a release build");

pub mod adapter;
pub mod protocol;
pub mod transport;
