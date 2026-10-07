//! The Work-owned shared artifact store (Domain §7): the selected
//! `FileSystemStorage` adapter reused over a namespace of its own. Nothing here
//! is a Document, an ACL or a path the frontend, an Agent or native code sees.
use document_application::{FileStorage, StorageError, StoreFileRequest};
use document_domain::{FileId, MediaType, StorageKey};
use document_storage_fs::FileSystemStorage;
use std::{future::Future, path::Path, time::Duration};
use tokio::io::AsyncReadExt;
use uuid::Uuid;
use work_application::{StoredGeneration, WorkArtifactStore, WorkFuture, content_identity};
use work_domain::{FileGeneration, MAX_FILE_BYTES, WorkError};

/// Below the configured storage root; Document's own listing never sees it.
pub const WORK_ARTIFACT_NAMESPACE: &str = "work-artifacts";
/// A hung volume is unavailable, never an indefinitely pending request.
const STORE_BUDGET: Duration = Duration::from_secs(5);
async fn bounded<T>(work: impl Future<Output = Result<T, WorkError>>) -> Result<T, WorkError> {
    tokio::time::timeout(STORE_BUDGET, work)
        .await
        .map_err(|_| WorkError::WorkArtifactUnavailable)?
}

pub struct FileSystemWorkArtifactStore {
    storage: FileSystemStorage,
}
impl FileSystemWorkArtifactStore {
    pub fn new(storage_root: &Path) -> Self {
        Self {
            storage: FileSystemStorage::new(storage_root.join(WORK_ARTIFACT_NAMESPACE)),
        }
    }
    fn key(id: Uuid) -> Result<StorageKey, WorkError> {
        let simple = id.simple().to_string();
        StorageKey::new(format!("objects/{}/{id}", &simple[..2]))
            .map_err(|_| WorkError::IntegrityViolation)
    }
    /// Bounded read of one generation; absent is `None`, never an empty file.
    async fn stored(&self, id: Uuid) -> Result<Option<Vec<u8>>, WorkError> {
        let mut reader = match self.storage.open(&Self::key(id)?).await {
            Ok(reader) => reader,
            Err(StorageError::NotFound) => return Ok(None),
            Err(_) => return Err(WorkError::WorkArtifactUnavailable),
        };
        let mut bytes = Vec::new();
        (&mut reader)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| WorkError::WorkArtifactUnavailable)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(WorkError::WorkArtifactUnavailable);
        }
        Ok(Some(bytes))
    }
    /// A generation is immutable: the same bytes replay, other bytes conflict.
    async fn existing(&self, id: Uuid, bytes: &[u8]) -> Result<Option<()>, WorkError> {
        match self.stored(id).await? {
            Some(existing) if existing == bytes => Ok(Some(())),
            Some(_) => Err(WorkError::OperationConflict),
            None => Ok(None),
        }
    }
    async fn put_bounded(&self, id: Uuid, bytes: Vec<u8>) -> Result<StoredGeneration, WorkError> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(WorkError::ValidationFailed);
        }
        let identity = content_identity(&bytes);
        if self.existing(id, &bytes).await?.is_some() {
            return Ok(identity);
        }
        let media = MediaType::new("application/octet-stream")
            .map_err(|_| WorkError::IntegrityViolation)?;
        let stored = match self
            .storage
            .put_immutable(StoreFileRequest::new(
                FileId::from_uuid(id),
                Box::pin(std::io::Cursor::new(bytes.clone())),
                media,
            ))
            .await
        {
            Ok(stored) => stored,
            // Only other bytes finalized under this generation are a conflict;
            // any other I/O failure is unavailability.
            Err(_) => {
                return match self.existing(id, &bytes).await {
                    Ok(Some(())) => Ok(identity),
                    Err(WorkError::OperationConflict) => Err(WorkError::OperationConflict),
                    _ => Err(WorkError::WorkArtifactUnavailable),
                };
            }
        };
        let hash: String = stored
            .content_hash()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if hash != identity.sha256 || stored.size_bytes().get() as u64 != identity.size_bytes {
            return Err(WorkError::WorkArtifactUnavailable);
        }
        Ok(identity)
    }
    async fn read_bounded(&self, generation: FileGeneration) -> Result<Vec<u8>, WorkError> {
        let bytes = self
            .stored(generation.id)
            .await?
            .ok_or(WorkError::WorkArtifactUnavailable)?;
        let identity = content_identity(&bytes);
        if identity.size_bytes != generation.size_bytes || identity.sha256 != generation.sha256 {
            return Err(WorkError::WorkArtifactUnavailable);
        }
        Ok(bytes)
    }
}
impl WorkArtifactStore for FileSystemWorkArtifactStore {
    fn put(&self, generation_id: Uuid, bytes: Vec<u8>) -> WorkFuture<'_, StoredGeneration> {
        Box::pin(bounded(self.put_bounded(generation_id, bytes)))
    }
    fn read(&self, generation: FileGeneration) -> WorkFuture<'_, Vec<u8>> {
        Box::pin(bounded(self.read_bounded(generation)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn generation(id: Uuid, identity: &StoredGeneration) -> FileGeneration {
        FileGeneration {
            id,
            size_bytes: identity.size_bytes,
            sha256: identity.sha256.clone(),
            stored_at: "2026-10-07T00:00:00Z".into(),
            provider_id: work_domain::WORK_ARTIFACT_PROVIDER_ID.into(),
        }
    }
    #[tokio::test]
    async fn generations_are_immutable_verified_and_kept_apart_from_document_storage() {
        let root = tempfile::tempdir().unwrap();
        let store = FileSystemWorkArtifactStore::new(root.path());
        let id = Uuid::now_v7();
        let bytes = "【合成】作業ファイル".as_bytes().to_vec();
        let identity = store.put(id, bytes.clone()).await.unwrap();
        assert_eq!(identity, content_identity(&bytes));
        // Same bytes replay; other bytes for the same generation never overwrite.
        assert_eq!(store.put(id, bytes.clone()).await.unwrap(), identity);
        assert_eq!(
            store.put(id, b"other".to_vec()).await,
            Err(WorkError::OperationConflict)
        );
        assert_eq!(store.read(generation(id, &identity)).await.unwrap(), bytes);
        // Only the Work namespace holds the object; Document's own tree is untouched.
        assert!(root.path().join(WORK_ARTIFACT_NAMESPACE).is_dir());
        assert!(!root.path().join("objects").exists());
        assert!(!root.path().join("staging").exists());
        // A mismatched record, a missing generation and altered bytes disclose nothing.
        let mut wrong = generation(id, &identity);
        wrong.size_bytes += 1;
        assert_eq!(
            store.read(wrong).await,
            Err(WorkError::WorkArtifactUnavailable)
        );
        assert_eq!(
            store.read(generation(Uuid::now_v7(), &identity)).await,
            Err(WorkError::WorkArtifactUnavailable)
        );
        let simple = id.simple().to_string();
        let path = root
            .path()
            .join(WORK_ARTIFACT_NAMESPACE)
            .join("objects")
            .join(&simple[..2])
            .join(id.to_string());
        std::fs::write(&path, b"tampered").unwrap();
        assert_eq!(
            store.read(generation(id, &identity)).await,
            Err(WorkError::WorkArtifactUnavailable)
        );
        assert_eq!(
            store.verify(generation(id, &identity)).await,
            Err(WorkError::WorkArtifactUnavailable)
        );
        // Bounds are enforced before any write.
        assert_eq!(
            store.put(Uuid::now_v7(), vec![]).await,
            Err(WorkError::ValidationFailed)
        );
        assert_eq!(
            store
                .put(Uuid::now_v7(), vec![0; MAX_FILE_BYTES as usize + 1])
                .await,
            Err(WorkError::ValidationFailed)
        );
        // An I/O failure at the generation's location is unavailability, never a
        // claim that other bytes were stored under the same operation.
        let blocked = Uuid::now_v7();
        let simple = blocked.simple().to_string();
        std::fs::create_dir_all(
            root.path()
                .join(WORK_ARTIFACT_NAMESPACE)
                .join("objects")
                .join(&simple[..2])
                .join(blocked.to_string()),
        )
        .unwrap();
        assert_eq!(
            store.put(blocked, b"synthetic".to_vec()).await,
            Err(WorkError::WorkArtifactUnavailable)
        );
        let full = vec![7; MAX_FILE_BYTES as usize];
        let full_id = Uuid::now_v7();
        let full_identity = store.put(full_id, full.clone()).await.unwrap();
        assert_eq!(
            store
                .read(generation(full_id, &full_identity))
                .await
                .unwrap()
                .len(),
            full.len()
        );
    }
}
