use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub capability_id: String,
    pub outcome: Outcome,
    pub commit: String,
    pub duration_ms: u64,
    pub artifact_path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("evidence I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid evidence JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn write(root: &Path, evidence: &Evidence) -> Result<PathBuf, EvidenceError> {
    let dir = root.join("target/assurance/evidence");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", evidence.capability_id));
    fs::write(&path, serde_json::to_vec_pretty(evidence)?)?;
    Ok(path)
}

pub fn read_all(root: &Path) -> Result<Vec<Evidence>, EvidenceError> {
    let dir = root.join("target/assurance/evidence");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut output = Vec::new();
    for path in paths {
        output.push(serde_json::from_slice(&fs::read(path)?)?);
    }
    Ok(output)
}
