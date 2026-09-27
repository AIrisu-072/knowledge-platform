use document_application::{DocumentPublishRepository, PublishVersionRecord};
use document_repository_postgres::PostgresDocumentRepository;

#[test]
fn repository_exposes_replacement_publish_transaction() {
    fn require_port<R: DocumentPublishRepository>(repository: &R, record: PublishVersionRecord) {
        let _ = repository.publish_next_version(record);
    }
    let _ = require_port::<PostgresDocumentRepository>;
}
