mod evidence;
mod model;
mod ports;
mod snapshot;

pub use evidence::{DiffInspectionEvidence, capture_pair_with_evidence};
pub use model::{
    AncillaryChange, Change, DiffRequest, DiffResult, LocatorGranularity, SourceEvidence,
    UnverifiedRegion,
};
pub use ports::{DiffExecutionError, DiffExecutor, DocumentDiffRepository};
pub use snapshot::{
    DiffCacheKey, DiffPairSnapshot, SnapshotItem, VersionSnapshot, manifest_fingerprint,
};
