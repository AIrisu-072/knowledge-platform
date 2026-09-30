//! Document HTTP transport boundary. Infrastructure is assembled outside this crate.

pub mod create;
pub mod diff;
pub mod error;
pub mod file_download;
pub mod identity;
pub mod limits;
pub mod management;
mod multipart;
pub mod publication;
pub mod read;
mod read_state;
pub mod router;
pub mod timeout;
pub mod trace;
pub mod validation;
pub mod versioning;
