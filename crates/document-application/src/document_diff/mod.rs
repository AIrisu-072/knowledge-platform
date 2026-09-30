mod evidence;
mod model;
mod ports;
mod projection;
mod service;
mod snapshot;

pub use evidence::{DiffInspectionEvidence, capture_pair_with_evidence};
pub use model::{
    AncillaryChange, AuthorizedDiffDisplay, Change, DiffDisplayItem, DiffRequest, DiffResult,
    LocatorGranularity, SourceEvidence, UnverifiedRegion,
};
pub use ports::{DiffCache, DiffExecutionError, DiffExecutor, DocumentDiffRepository};
pub use projection::{
    AuthorizedComparisonTable, ComparisonRow, ComparisonRowState, project_comparison_table,
};
pub use service::{AuthorizedDiff, DocumentDiffService};
pub use snapshot::{
    DiffCacheKey, DiffPairSnapshot, SnapshotItem, VersionSnapshot, manifest_fingerprint,
};
