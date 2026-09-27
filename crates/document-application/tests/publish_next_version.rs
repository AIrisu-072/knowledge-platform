use document_application::PublishVersionRecord;

#[test]
fn replacement_publish_has_a_distinct_atomic_record() {
    fn require_record(_: PublishVersionRecord) {}
    let _ = require_record;
}
