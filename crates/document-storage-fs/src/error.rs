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


#[cfg(test)]
mod tests {
    use std::io;

    use document_application::StorageError;

    #[test]
    fn open_error_distinguishes_object_unreadable_from_dependency_outage() {
        assert_eq!(
            super::map_open_error(io::Error::from(io::ErrorKind::PermissionDenied)),
            StorageError::ObjectUnreadable,
        );
        assert_eq!(
            super::map_open_error(io::Error::from(io::ErrorKind::NotFound)),
            StorageError::NotFound,
        );
        assert_eq!(
            super::map_open_error(io::Error::from(io::ErrorKind::ConnectionReset)),
            StorageError::Unavailable,
        );
    }
}
