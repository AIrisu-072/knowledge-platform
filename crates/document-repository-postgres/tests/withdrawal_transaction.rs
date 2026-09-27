use document_application::{VersioningRepository, WithdrawVersionRecord};
use document_repository_postgres::PostgresDocumentRepository;

#[test]
fn withdrawal_has_an_atomic_repository_port() {
    fn require_port<R: VersioningRepository>(repository: &R, record: WithdrawVersionRecord) {
        let _ = repository.withdraw_version(record);
    }
    let _ = require_port::<PostgresDocumentRepository>;
}
