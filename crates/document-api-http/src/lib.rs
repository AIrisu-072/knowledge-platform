//! Document HTTP transport boundary. Infrastructure is assembled outside this crate.

pub mod create;
pub mod error;
pub mod identity;
pub mod limits;
pub mod management;
mod multipart;
pub mod read;
mod read_state;
pub mod router;
pub mod trace;
pub mod validation;
pub mod versioning;
