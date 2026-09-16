mod checks;
mod config;
mod report;

use std::io;
use thiserror::Error;

pub use checks::check_repository;
pub use config::Config;
pub use report::{Finding, Report};

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid architecture config: {0}")]
    Toml(#[from] toml::de::Error),
}
