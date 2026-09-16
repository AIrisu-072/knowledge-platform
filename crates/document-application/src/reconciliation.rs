use time::{Duration, OffsetDateTime};

use crate::{StorageObjectInfo, StorageObjectKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationClassification {
    Healthy,
    StaleStaging,
    Orphan,
    IntegrityViolation,
}

pub fn classify(
    db_referenced: bool,
    object: Option<&StorageObjectInfo>,
    now: OffsetDateTime,
    grace: Duration,
) -> Option<ReconciliationClassification> {
    match (db_referenced, object) {
        (true, Some(object)) if object.kind() == StorageObjectKind::Final => {
            Some(ReconciliationClassification::Healthy)
        }
        (true, _) => Some(ReconciliationClassification::IntegrityViolation),
        (false, Some(object)) if now - object.modified_at() >= grace => match object.kind() {
            StorageObjectKind::Staging => Some(ReconciliationClassification::StaleStaging),
            StorageObjectKind::Final => Some(ReconciliationClassification::Orphan),
            StorageObjectKind::Unknown => None,
        },
        (false, _) => None,
    }
}
