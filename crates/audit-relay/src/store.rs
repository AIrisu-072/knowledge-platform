//! The relay's Store calls (design §6.2–§6.4, §10.1, §10.4, §11, §12).
//!
//! Everything goes through the `audit_core::AuditStore` port (ingest, probe,
//! report_regression, receipt lookups, relay control events). [`RelayStore`]
//! adds the two content-free reads the port does not cover: the Store status
//! (health) and the declared lost ranges of recovery epochs (reconcile).
//!
//! Store logins (design §10.1):
//! - the relay service (`run`): `audit_store_ingest` +
//!   `audit_store_relay_control` + `audit_store_reconciler`, bound to the
//!   registered source service principal;
//! - the relay operator (`reconcile`, `replay`, `health`):
//!   `audit_store_relay_control` + `audit_store_reconciler` only, bound to
//!   the operator's own principal. It never holds `audit_store_ingest`.

use audit_core::port::BoxFuture;
use audit_core::{AuditStore, OutageCode, StoreError};
use audit_store_postgres::PostgresAuditStore;
pub use audit_store_postgres::{LostRange, StoreStatusRow};

/// The Store port plus the relay's two status reads.
pub trait RelayStore: AuditStore {
    /// `audit_store.store_status()` (reconciler role).
    fn store_status(&self) -> BoxFuture<'_, Result<StoreStatusRow, StoreError>>;

    /// `audit_store.lookup_lost_ranges()` (reconciler role): the declared
    /// lost range of every recovery epoch.
    fn lookup_lost_ranges(&self) -> BoxFuture<'_, Result<Vec<LostRange>, StoreError>>;
}

impl RelayStore for PostgresAuditStore {
    fn store_status(&self) -> BoxFuture<'_, Result<StoreStatusRow, StoreError>> {
        Box::pin(PostgresAuditStore::store_status(self))
    }

    fn lookup_lost_ranges(&self) -> BoxFuture<'_, Result<Vec<LostRange>, StoreError>> {
        Box::pin(PostgresAuditStore::lookup_lost_ranges(self))
    }
}

/// The outage class of a failed Store call. Verdicts (`conflict`,
/// `rejected`) only come from `ingest`; anywhere else they would be a
/// protocol surprise and count as `store_other`.
pub fn outage_of(error: &StoreError) -> OutageCode {
    error.outage_code().unwrap_or(OutageCode::Other)
}
