use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);

        impl $name {
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}

typed_id!(DocumentId);
typed_id!(DocumentVersionId);
typed_id!(FileId);
typed_id!(FolderId);
typed_id!(EventId);
typed_id!(AuditEventId);
