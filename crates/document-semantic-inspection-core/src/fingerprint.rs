use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FingerprintAlgorithm {
    #[serde(rename = "sha256")]
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticFingerprint {
    algorithm: FingerprintAlgorithm,
    digest: [u8; 32],
}

impl SemanticFingerprint {
    pub fn sha256_from_slice(value: &[u8]) -> Result<Self, CoreError> {
        let digest: [u8; 32] = value
            .try_into()
            .map_err(|_| CoreError::InvalidSha256Length {
                observed: value.len(),
            })?;
        Ok(Self {
            algorithm: FingerprintAlgorithm::Sha256,
            digest,
        })
    }

    pub fn sha256(projection: &[u8]) -> Self {
        let digest: [u8; 32] = Sha256::digest(projection).into();
        Self {
            algorithm: FingerprintAlgorithm::Sha256,
            digest,
        }
    }

    pub const fn algorithm(&self) -> FingerprintAlgorithm {
        self.algorithm
    }

    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}
