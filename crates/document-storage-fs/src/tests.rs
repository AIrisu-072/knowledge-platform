use std::io::Cursor;

use document_application::{
    FileStorage, StorageError, StorageObjectKind, StoreFileRequest,
};
use document_domain::{FileId, MediaType};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    ops::FsFailurePoint,
    FileSystemStorage,
};

const CONTENT: &[u8] = b"authoritative-content";

fn file_id() -> FileId {
    FileId::from_uuid(Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap())
}

fn final_relative_key(file_id: FileId) -> String {
    let uuid = file_id.as_uuid();
    let hyphenated = uuid.to_string();
    let simple = uuid.simple().to_string();
    format!("objects/{}/{}", &simple[..2], hyphenated)
}

fn request(file_id: FileId) -> StoreFileRequest {
    StoreFileRequest::new(
        file_id,
        Box::pin(Cursor::new(CONTENT.to_vec())),
        MediaType::new("application/pdf").unwrap(),
    )
}

#[tokio::test]
async fn stores_hashes_finalizes_and_reads_back_immutable_content() {
    let temp = TempDir::new().unwrap();
    let storage = FileSystemStorage::new(temp.path());
    let id = file_id();

    let stored = storage.put_immutable(request(id)).await.unwrap();

    assert_eq!(stored.storage_key().as_str(), final_relative_key(id));
    assert_eq!(stored.size_bytes().get(), CONTENT.len() as i64);
    let expected_hash: [u8; 32] = Sha256::digest(CONTENT).into();
    assert_eq!(stored.content_hash().as_bytes(), &expected_hash);
    assert!(temp.path().join(stored.storage_key().as_str()).is_file());

    let mut reader = storage.open(stored.storage_key()).await.unwrap();
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).await.unwrap();
    assert_eq!(actual, CONTENT);

    let staging_entries = std::fs::read_dir(temp.path().join("staging"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(staging_entries.is_empty());
}

#[tokio::test]
async fn storage_identity_is_derived_only_from_file_id() {
    let temp = TempDir::new().unwrap();
    let storage = FileSystemStorage::new(temp.path());
    let id = file_id();

    // The storage request intentionally has no original-filename field. Names such as
    // "../../escape.pdf", "同名.pdf", or a 255-character filename remain VersionFile
    // metadata and cannot influence this adapter's storage path.
    let stored = storage.put_immutable(request(id)).await.unwrap();

    assert_eq!(stored.storage_key().as_str(), final_relative_key(id));
}

#[tokio::test]
async fn injected_failure_points_surface_precise_errors_without_false_success() {
    let cases = [
        (FsFailurePoint::Write, StorageError::WriteFailed, false),
        (FsFailurePoint::SyncFile, StorageError::SyncFailed, false),
        (FsFailurePoint::Rename, StorageError::FinalizeFailed, false),
        (FsFailurePoint::SyncDirectory, StorageError::SyncFailed, true),
    ];

    for (point, expected, final_may_exist) in cases {
        let temp = TempDir::new().unwrap();
        let storage = FileSystemStorage::with_failure_point_for_test(temp.path(), point);
        let id = file_id();

        let error = storage.put_immutable(request(id)).await.unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(
            temp.path().join(final_relative_key(id)).exists(),
            final_may_exist,
            "unexpected final-object state for {point:?}"
        );
    }
}

#[tokio::test]
async fn enumerates_staging_final_and_unknown_objects_without_deleting_them() {
    let temp = TempDir::new().unwrap();
    let id = file_id();
    let uuid = id.as_uuid().to_string();
    let final_key = final_relative_key(id);

    std::fs::create_dir_all(temp.path().join("staging")).unwrap();
    std::fs::create_dir_all(temp.path().join("objects/00")).unwrap();
    std::fs::write(temp.path().join(format!("staging/{uuid}.part")), b"stage").unwrap();
    std::fs::write(temp.path().join(&final_key), b"final").unwrap();
    std::fs::write(temp.path().join("staging/not-a-file-id.part"), b"unknown").unwrap();

    let storage = FileSystemStorage::new(temp.path());
    let mut objects = storage.list_objects().await.unwrap();
    objects.sort_by(|left, right| left.relative_key().cmp(right.relative_key()));

    assert_eq!(objects.len(), 3);

    let known_final = objects
        .iter()
        .find(|object| object.relative_key() == final_key)
        .unwrap();
    assert_eq!(known_final.kind(), StorageObjectKind::Final);
    assert_eq!(known_final.file_id(), Some(id));

    let known_staging = objects
        .iter()
        .find(|object| object.relative_key() == format!("staging/{uuid}.part"))
        .unwrap();
    assert_eq!(known_staging.kind(), StorageObjectKind::Staging);
    assert_eq!(known_staging.file_id(), Some(id));

    let unknown = objects
        .iter()
        .find(|object| object.relative_key() == "staging/not-a-file-id.part")
        .unwrap();
    assert_eq!(unknown.kind(), StorageObjectKind::Unknown);
    assert_eq!(unknown.file_id(), None);
    assert!(temp.path().join("staging/not-a-file-id.part").exists());
}
