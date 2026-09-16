use std::io;

use document_application::StorageError;

pub(crate) fn map_open_error(error: io::Error) -> StorageError {
    if error.kind() == io::ErrorKind::NotFound {
        StorageError::NotFound
    } else {
        StorageError::Unavailable
    }
}

pub(crate) fn map_unavailable_error(_error: io::Error) -> StorageError {
    StorageError::Unavailable
}

pub(crate) fn map_write_error(_error: io::Error) -> StorageError {
    StorageError::WriteFailed
}

pub(crate) fn map_sync_error(_error: io::Error) -> StorageError {
    StorageError::SyncFailed
}

pub(crate) fn map_finalize_error(_error: io::Error) -> StorageError {
    StorageError::FinalizeFailed
}
