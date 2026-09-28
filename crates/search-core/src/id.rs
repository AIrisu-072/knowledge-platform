//! Stable Search and Discovery identifiers, distinct from source-native labels.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
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

typed_id!(SourceId);
typed_id!(ResourceId);
typed_id!(LogicalResourceId);
typed_id!(RepresentationId);
typed_id!(ResourceVersionId);
typed_id!(UsageProfileId);
typed_id!(DiscoveryEvaluationId);
typed_id!(BindingId);
typed_id!(NeedId);
typed_id!(SessionId);
typed_id!(ClaimId);
typed_id!(GapId);
typed_id!(RelationId);
typed_id!(AssertionId);
typed_id!(ProjectionGenerationId);
typed_id!(PredicateId);
typed_id!(FactId);
