use crate::capability::Capability;
use crate::evidence::{self, Evidence, Outcome};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use thiserror::Error;

pub trait Provider {
    fn id(&self) -> &'static str;
    fn run(&self, capability: &Capability) -> Result<Evidence, ProviderError>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("capability {0} has no command")]
    EmptyCommand(String),
    #[error("provider I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("evidence error: {0}")]
    Evidence(#[from] evidence::EvidenceError),
}

pub struct CommandProvider {
    root: PathBuf,
    commit: String,
}

impl CommandProvider {
    pub fn new(root: impl Into<PathBuf>, commit: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            commit: commit.into(),
        }
    }
}

impl Provider for CommandProvider {
    fn id(&self) -> &'static str {
        "command"
    }

    fn run(&self, capability: &Capability) -> Result<Evidence, ProviderError> {
        let (program, args) = capability
            .command
            .split_first()
            .ok_or_else(|| ProviderError::EmptyCommand(capability.id.clone()))?;
        let started = Instant::now();
        let output = Command::new(program)
            .args(args)
            .current_dir(&self.root)
            .output()?;
        let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);

        let dir = self.root.join("target/assurance/evidence");
        fs::create_dir_all(&dir)?;
        let artifact = dir.join(format!("{}.artifact.txt", capability.id));
        let mut bytes = format!("status={:?}\n--- stdout ---\n", output.status).into_bytes();
        bytes.extend_from_slice(&output.stdout);
        bytes.extend_from_slice(b"\n--- stderr ---\n");
        bytes.extend_from_slice(&output.stderr);
        fs::write(&artifact, bytes)?;

        let evidence = Evidence {
            capability_id: capability.id.clone(),
            outcome: if output.status.success() {
                Outcome::Pass
            } else {
                Outcome::Fail
            },
            commit: self.commit.clone(),
            duration_ms,
            artifact_path: Some(relative(&self.root, &artifact)),
        };
        evidence::write(&self.root, &evidence)?;
        Ok(evidence)
    }
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}
