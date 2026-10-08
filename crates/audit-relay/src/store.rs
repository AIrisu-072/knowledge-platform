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

#[cfg(test)]
mod tests {
    /// Every relay source file, embedded.
    const SOURCES: [(&str, &str); 14] = [
        ("bin/audit_relay.rs", include_str!("bin/audit_relay.rs")),
        ("breaker.rs", include_str!("breaker.rs")),
        ("config.rs", include_str!("config.rs")),
        ("handler.rs", include_str!("handler.rs")),
        ("health.rs", include_str!("health.rs")),
        ("ledger.rs", include_str!("ledger.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("monitor.rs", include_str!("monitor.rs")),
        ("reconcile.rs", include_str!("reconcile.rs")),
        ("relay.rs", include_str!("relay.rs")),
        ("replay.rs", include_str!("replay.rs")),
        ("session.rs", include_str!("session.rs")),
        ("source.rs", include_str!("source.rs")),
        ("store.rs", include_str!("store.rs")),
    ];

    /// The relay never builds a Store verdict itself: outside its unit tests
    /// it has no `IngestRow` literal and decodes no row. Every ingest goes
    /// through `audit_store_postgres::PostgresAuditStore`, whose only
    /// `IngestRow` is read from `audit_store.ingest`'s result columns
    /// (`crates/audit-store-postgres/tests/store_ingest.rs`).
    #[test]
    fn the_relay_builds_no_ingest_rows() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut on_disk = std::collections::BTreeSet::new();
        let mut pending = vec![dir.clone()];
        while let Some(next) = pending.pop() {
            for entry in std::fs::read_dir(&next).expect("src") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let relative = path.strip_prefix(&dir).expect("relative");
                    on_disk.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let embedded: std::collections::BTreeSet<String> =
            SOURCES.iter().map(|(n, _)| (*n).to_owned()).collect();
        assert_eq!(embedded, on_disk, "scan every source file");
        let needles = [concat!("IngestRow", " {"), concat!(".into", "_result()")];
        for (name, text) in SOURCES {
            let production = text.split("#[cfg(test)]").next().unwrap_or(text);
            for line in production.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for needle in needles {
                    assert!(!line.contains(needle), "{name}: {line}");
                }
            }
        }
    }
}
