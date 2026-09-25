use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum InspectionProfileVersion {
    #[serde(rename = "dsi-v0")]
    DsiV0,
}

impl InspectionProfileVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DsiV0 => "dsi-v0",
        }
    }
}
