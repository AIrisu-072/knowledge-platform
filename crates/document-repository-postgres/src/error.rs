use document_application::RepositoryError;

pub(crate) fn map_statement_error(_error: sqlx::Error) -> RepositoryError {
    RepositoryError::Internal("postgres operation failed".to_owned())
}

pub(crate) fn map_commit_error(_error: sqlx::Error) -> RepositoryError {
    RepositoryError::CommitOutcomeUnknown
}

#[cfg(test)]
mod tests {
    use document_application::RepositoryError;

    #[test]
    fn any_commit_error_is_reported_as_unknown_outcome() {
        let error = super::map_commit_error(sqlx::Error::PoolClosed);
        assert_eq!(error, RepositoryError::CommitOutcomeUnknown);
    }
}
