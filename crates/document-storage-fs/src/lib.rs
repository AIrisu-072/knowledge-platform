#![forbid(unsafe_code)]

//! Local filesystem adapter for immutable document binaries.

mod error;
pub(crate) mod ops;
mod storage;

pub use storage::FileSystemStorage;

#[cfg(test)]
mod tests;
