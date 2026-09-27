use document_application::{PublicationScheduleRepository, SchedulePublishRecord};
use document_repository_postgres::PostgresDocumentRepository;

#[test]
fn reservation_has_an_atomic_repository_port() {
    fn require_port<R: PublicationScheduleRepository>(
        repository: &R,
        record: SchedulePublishRecord,
    ) {
        let _ = repository.reserve(record);
    }
    let _ = require_port::<PostgresDocumentRepository>;
}
