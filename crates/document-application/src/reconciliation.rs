use document_domain::FileId;
use time::{Duration, OffsetDateTime};

use crate::{StorageObjectInfo, StorageObjectKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationClassification {
    Healthy,
    StaleStaging,
    Orphan,
    IntegrityViolation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationFinding {
    file_id: FileId,
    object: Option<StorageObjectInfo>,
    classification: ReconciliationClassification,
}

impl ReconciliationFinding {
    pub(crate) fn new(
        file_id: FileId,
        object: Option<StorageObjectInfo>,
        classification: ReconciliationClassification,
    ) -> Self {
        Self {
            file_id,
            object,
            classification,
        }
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub const fn object(&self) -> Option<&StorageObjectInfo> {
        self.object.as_ref()
    }

    pub const fn classification(&self) -> ReconciliationClassification {
        self.classification
    }
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
