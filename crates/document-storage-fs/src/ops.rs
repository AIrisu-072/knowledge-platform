use document_application::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FsFailurePoint {
    Write,
    SyncFile,
    Rename,
    SyncDirectory,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StorageOps {
    failure_point: Option<FsFailurePoint>,
}

impl StorageOps {
    pub(crate) const fn normal() -> Self {
        Self {
            failure_point: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn fail_at(failure_point: FsFailurePoint) -> Self {
        Self {
            failure_point: Some(failure_point),
        }
    }

    pub(crate) fn check(&self, point: FsFailurePoint) -> Result<(), StorageError> {
        if self.failure_point != Some(point) {
            return Ok(());
        }

        Err(match point {
            FsFailurePoint::Write => StorageError::WriteFailed,
            FsFailurePoint::SyncFile | FsFailurePoint::SyncDirectory => StorageError::SyncFailed,
            FsFailurePoint::Rename => StorageError::FinalizeFailed,
        })
    }
}
