use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub controls: Vec<String>,
    pub provider: String,
    pub mechanism: String,
    pub oracle: String,
    pub cost: String,
    pub command: Vec<String>,
    pub scope_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[allow(dead_code)]
    version: u32,
    capability: Vec<Capability>,
}

#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("capability I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid capability manifest: {0}")]
    Toml(#[from] toml::de::Error),
}

pub fn load(root: &Path) -> Result<Vec<Capability>, CapabilityError> {
    let dir = root.join("spec/assurance/capabilities");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut output = Vec::new();
    for path in paths {
        let manifest: Manifest = toml::from_str(&fs::read_to_string(path)?)?;
        output.extend(manifest.capability);
    }
    Ok(output)
}
