//! Document HTTP transport boundary. Infrastructure is assembled outside this crate.

pub mod error;
pub mod identity;
pub mod management;
pub mod read;
mod read_state;
pub mod router;
pub mod trace;
pub mod validation;
