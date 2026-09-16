use std::path::{Path, PathBuf};

use document_application::{
    ContentReader, FileStorage, StorageError, StorageObjectInfo, StorageObjectKind,
    StoreFileRequest, StoredFile,
};
use document_domain::{ContentHash, FileId, FileSize, StorageKey};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::error::{
    map_finalize_error, map_open_error, map_sync_error, map_unavailable_error, map_write_error,
};
use crate::ops::{FsFailurePoint, StorageOps};

#[derive(Debug, Clone)]
pub struct FileSystemStorage {
    root: PathBuf,
    ops: StorageOps,
}

impl FileSystemStorage {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            ops: StorageOps::normal(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_failure_point_for_test(
        root: impl AsRef<Path>,
        failure_point: FsFailurePoint,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            ops: StorageOps::fail_at(failure_point),
        }
    }

    fn staging_relative_key(file_id: FileId) -> String {
        format!("staging/{}.part", file_id.as_uuid())
    }

    fn final_relative_key(file_id: FileId) -> String {
        let uuid = file_id.as_uuid();
        let simple = uuid.simple().to_string();
        format!("objects/{}/{}", &simple[..2], uuid)
    }

    fn absolute_path(&self, relative_key: &str) -> PathBuf {
        self.root.join(relative_key)
    }

    async fn ensure_parent_directories(
        &self,
        staging_path: &Path,
        final_path: &Path,
    ) -> Result<(), StorageError> {
        let staging_parent = staging_path
            .parent()
            .ok_or_else(|| StorageError::Internal("staging path has no parent".to_owned()))?;
        let final_parent = final_path
            .parent()
            .ok_or_else(|| StorageError::Internal("final path has no parent".to_owned()))?;

        fs::create_dir_all(staging_parent)
            .await
            .map_err(map_unavailable_error)?;
        fs::create_dir_all(final_parent)
            .await
            .map_err(map_unavailable_error)?;
        Ok(())
    }

    async fn collect_staging_objects(
        &self,
        objects: &mut Vec<StorageObjectInfo>,
    ) -> Result<(), StorageError> {
        let Some(mut entries) = read_dir_if_exists(self.root.join("staging")).await? else {
            return Ok(());
        };

        while let Some(entry) = entries.next_entry().await.map_err(map_unavailable_error)? {
            if !entry
                .file_type()
                .await
                .map_err(map_unavailable_error)?
                .is_file()
            {
                continue;
            }

            let name = entry.file_name().to_string_lossy().into_owned();
            let relative_key = format!("staging/{name}");
            let file_id = name.strip_suffix(".part").and_then(parse_file_id);
            let kind = if file_id.is_some() {
                StorageObjectKind::Staging
            } else {
                StorageObjectKind::Unknown
            };
            objects.push(object_info(entry.path(), relative_key, kind, file_id).await?);
        }

        Ok(())
    }

    async fn collect_final_objects(
        &self,
        objects: &mut Vec<StorageObjectInfo>,
    ) -> Result<(), StorageError> {
        let Some(mut prefix_entries) = read_dir_if_exists(self.root.join("objects")).await? else {
            return Ok(());
        };

        while let Some(prefix_entry) = prefix_entries
            .next_entry()
            .await
            .map_err(map_unavailable_error)?
        {
            let file_type = prefix_entry
                .file_type()
                .await
                .map_err(map_unavailable_error)?;
            let prefix = prefix_entry.file_name().to_string_lossy().into_owned();

            if file_type.is_file() {
                let relative_key = format!("objects/{prefix}");
                objects.push(
                    object_info(
                        prefix_entry.path(),
                        relative_key,
                        StorageObjectKind::Unknown,
                        parse_file_id(&prefix),
                    )
                    .await?,
                );
                continue;
            }

            if !file_type.is_dir() {
                continue;
            }

            let mut entries = fs::read_dir(prefix_entry.path())
                .await
                .map_err(map_unavailable_error)?;
            while let Some(entry) = entries.next_entry().await.map_err(map_unavailable_error)? {
                if !entry
                    .file_type()
                    .await
                    .map_err(map_unavailable_error)?
                    .is_file()
                {
                    continue;
                }

                let name = entry.file_name().to_string_lossy().into_owned();
                let relative_key = format!("objects/{prefix}/{name}");
                let file_id = parse_file_id(&name);
                let kind = if file_id
                    .map(|id| {
                        let simple = id.as_uuid().simple().to_string();
                        prefix == &simple[..2]
                    })
                    .unwrap_or(false)
                {
                    StorageObjectKind::Final
                } else {
                    StorageObjectKind::Unknown
                };
                objects.push(object_info(entry.path(), relative_key, kind, file_id).await?);
            }
        }

        Ok(())
    }
}

impl FileStorage for FileSystemStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let (file_id, mut content, media_type) = request.into_parts();
        let staging_relative_key = Self::staging_relative_key(file_id);
        let final_relative_key = Self::final_relative_key(file_id);
        let staging_path = self.absolute_path(&staging_relative_key);
        let final_path = self.absolute_path(&final_relative_key);

        self.ensure_parent_directories(&staging_path, &final_path)
            .await?;
        self.ops.check(FsFailurePoint::Write)?;

        let mut staging_file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging_path)
            .await
            .map_err(map_write_error)?;

        let mut hasher = Sha256::new();
        let mut size_bytes: u64 = 0;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = content
                .read(&mut buffer)
                .await
                .map_err(|_| StorageError::WriteFailed)?;
            if read == 0 {
                break;
            }

            staging_file
                .write_all(&buffer[..read])
                .await
                .map_err(map_write_error)?;
            hasher.update(&buffer[..read]);
            size_bytes = size_bytes
                .checked_add(read as u64)
                .ok_or_else(|| StorageError::Internal("file size overflow".to_owned()))?;
        }

        self.ops.check(FsFailurePoint::SyncFile)?;
        staging_file.sync_all().await.map_err(map_sync_error)?;
        drop(staging_file);

        if fs::try_exists(&final_path)
            .await
            .map_err(map_finalize_error)?
        {
            return Err(StorageError::FinalizeFailed);
        }

        self.ops.check(FsFailurePoint::Rename)?;
        fs::rename(&staging_path, &final_path)
            .await
            .map_err(map_finalize_error)?;

        self.ops.check(FsFailurePoint::SyncDirectory)?;
        let final_parent = final_path
            .parent()
            .ok_or_else(|| StorageError::Internal("final path has no parent".to_owned()))?;
        sync_directory(final_parent)?;

        let digest: [u8; 32] = hasher.finalize().into();
        let content_hash = ContentHash::from_slice(&digest)
            .map_err(|error| StorageError::Internal(error.to_string()))?;
        let size_bytes = i64::try_from(size_bytes)
            .map_err(|_| StorageError::Internal("file size exceeds i64".to_owned()))?;
        let size_bytes =
            FileSize::new(size_bytes).map_err(|error| StorageError::Internal(error.to_string()))?;
        let storage_key = StorageKey::new(final_relative_key)
            .map_err(|error| StorageError::Internal(error.to_string()))?;

        Ok(StoredFile::new(
            storage_key,
            content_hash,
            size_bytes,
            media_type,
        ))
    }

    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        let file = fs::File::open(self.absolute_path(key.as_str()))
            .await
            .map_err(map_open_error)?;
        Ok(Box::pin(file))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        let mut objects = Vec::new();
        self.collect_staging_objects(&mut objects).await?;
        self.collect_final_objects(&mut objects).await?;
        Ok(objects)
    }
}

async fn read_dir_if_exists(path: PathBuf) -> Result<Option<fs::ReadDir>, StorageError> {
    match fs::read_dir(path).await {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(map_unavailable_error(error)),
    }
}

async fn object_info(
    path: PathBuf,
    relative_key: String,
    kind: StorageObjectKind,
    file_id: Option<FileId>,
) -> Result<StorageObjectInfo, StorageError> {
    let metadata = fs::metadata(path).await.map_err(map_unavailable_error)?;
    let modified = metadata.modified().map_err(map_unavailable_error)?;
    Ok(StorageObjectInfo::new(
        relative_key,
        kind,
        file_id,
        OffsetDateTime::from(modified),
    ))
}

fn parse_file_id(value: &str) -> Option<FileId> {
    Uuid::parse_str(value).ok().map(FileId::from_uuid)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), StorageError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(map_sync_error)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}
