use std::collections::{HashMap, VecDeque};

use document_application::{
    RepositoryError,
    document_diff::{DiffCache, DiffCacheKey, DiffResult},
};

use crate::PostgresDocumentRepository;

const MAX_ENTRIES: usize = 64;
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
struct CacheEntry {
    bytes: Vec<u8>,
    digest: [u8; 32],
}

#[derive(Debug, Default)]
pub(crate) struct BoundedDiffCache {
    entries: HashMap<DiffCacheKey, CacheEntry>,
    order: VecDeque<DiffCacheKey>,
    total_bytes: usize,
}

impl BoundedDiffCache {
    fn get(&self, key: &DiffCacheKey) -> Option<(&[u8], [u8; 32])> {
        self.entries
            .get(key)
            .map(|entry| (entry.bytes.as_slice(), entry.digest))
    }

    fn put(&mut self, key: DiffCacheKey, bytes: Vec<u8>, digest: [u8; 32]) {
        if bytes.len() > MAX_ENTRY_BYTES {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.total_bytes -= old.bytes.len();
            self.order.retain(|candidate| candidate != &key);
        }
        while self.entries.len() >= MAX_ENTRIES
            || self.total_bytes.saturating_add(bytes.len()) > MAX_TOTAL_BYTES
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest) {
                self.total_bytes -= old.bytes.len();
            }
        }
        self.total_bytes += bytes.len();
        self.order.push_back(key);
        self.entries.insert(key, CacheEntry { bytes, digest });
    }
}

impl DiffCache for PostgresDocumentRepository {
    async fn get(&self, key: &DiffCacheKey) -> Result<Option<DiffResult>, RepositoryError> {
        let entry = {
            let guard = self
                .diff_cache
                .lock()
                .map_err(|_| RepositoryError::Unavailable)?;
            guard
                .get(key)
                .map(|(bytes, digest)| (bytes.to_vec(), digest))
        };
        let Some((bytes, digest)) = entry else {
            return Ok(None);
        };
        let result: DiffResult =
            serde_json::from_slice(&bytes).map_err(|_| RepositoryError::IntegrityViolation)?;
        if result.validate().is_err()
            || result.canonical_bytes() != bytes
            || result.canonical_digest() != digest
            || DiffCacheKey::from_result(&result) != *key
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        Ok(Some(result))
    }

    async fn put(
        &self,
        key: DiffCacheKey,
        result: DiffResult,
        expected_digest: [u8; 32],
    ) -> Result<(), RepositoryError> {
        if result.validate().is_err()
            || DiffCacheKey::from_result(&result) != key
            || result.canonical_digest() != expected_digest
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        let bytes = result.canonical_bytes();
        if bytes.len() > MAX_ENTRY_BYTES {
            return Ok(());
        }
        self.diff_cache
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?
            .put(key, bytes, expected_digest);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn total_byte_budget_never_exceeds_the_cap() {
        let mut cache = BoundedDiffCache::default();
        for n in 0..64_u128 {
            let key = DiffCacheKey::from_result(&document_application::document_diff::DiffResult {
                document_id: document_domain::DocumentId::from_uuid(uuid::Uuid::from_u128(1)),
                base_version_id: document_domain::DocumentVersionId::from_uuid(
                    uuid::Uuid::from_u128(2),
                ),
                target_version_id: document_domain::DocumentVersionId::from_uuid(
                    uuid::Uuid::from_u128(3),
                ),
                base_snapshot_digest: [n as u8; 32],
                target_snapshot_digest: [0; 32],
                profile: document_diff_core::DiffProfileVersion::V0,
                resource_profile: document_diff_core::ResourceProfileVersion::V0,
                verdict: document_diff_core::ContentVerdict::Same,
                coverage: document_diff_core::DiffCoverage::Full,
                changes: vec![],
                unverified_regions: vec![],
                ancillary_changes: vec![],
            });
            cache.put(key, vec![0; 3 * 1024 * 1024], [n as u8; 32]);
        }
        assert!(cache.total_bytes <= MAX_TOTAL_BYTES);
        assert!(cache.entries.len() < 64);
    }
}
