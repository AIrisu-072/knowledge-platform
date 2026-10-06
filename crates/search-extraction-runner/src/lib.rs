//! Trusted Search extraction host boundary.

#![forbid(unsafe_code)]

mod executor;

pub use executor::{SearchExtractionRunner, SearchRunnerConfig, executable_sha256};
