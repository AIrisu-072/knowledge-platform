use document_application::RepositoryError;

pub(crate) fn map_statement_error(error: sqlx::Error) -> RepositoryError {
    match error {
        sqlx::Error::PoolClosed
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::Io(_)
        | sqlx::Error::WorkerCrashed => RepositoryError::Unavailable,
        _ => RepositoryError::Internal("postgres operation failed".to_owned()),
    }
}

pub(crate) fn map_commit_error(_error: sqlx::Error) -> RepositoryError {
    RepositoryError::CommitOutcomeUnknown
}

#[cfg(test)]
mod tests {
    use std::io::{Error as IoError, ErrorKind};

    use document_application::RepositoryError;

    #[test]
    fn dependency_statement_errors_are_reported_as_unavailable() {
        let errors = [
            sqlx::Error::PoolClosed,
            sqlx::Error::PoolTimedOut,
            sqlx::Error::Io(IoError::new(ErrorKind::ConnectionReset, "connection reset")),
            sqlx::Error::WorkerCrashed,
        ];

        for error in errors {
            assert_eq!(
                super::map_statement_error(error),
                RepositoryError::Unavailable
            );
        }
    }

    #[test]
    fn non_dependency_statement_errors_remain_internal() {
        let error = super::map_statement_error(sqlx::Error::InvalidArgument("invalid".to_owned()));
        assert_eq!(
            error,
            RepositoryError::Internal("postgres operation failed".to_owned())
        );
    }

    #[test]
    fn any_commit_error_is_reported_as_unknown_outcome() {
        let error = super::map_commit_error(sqlx::Error::PoolClosed);
        assert_eq!(error, RepositoryError::CommitOutcomeUnknown);
    }
}
