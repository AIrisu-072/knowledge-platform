#[cfg(test)]
mod tests {
    use document_application::RepositoryError;

    #[test]
    fn any_commit_error_is_reported_as_unknown_outcome() {
        let error = super::map_commit_error(sqlx::Error::PoolClosed);
        assert_eq!(error, RepositoryError::CommitOutcomeUnknown);
    }
}
