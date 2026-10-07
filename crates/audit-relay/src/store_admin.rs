//! The Store functions the relay uses besides `ingest`/`probe`: content-free
//! receipts, relay control events, status, and the regression report
//! (design §6.4, §10.4, §11, §12).
//!
//! A small trait so that tests can stub one call (e.g. a Store function that
//! is not deployed yet) while the rest goes to the real Store.

use audit_core::port::BoxFuture;
use audit_core::{OutageCode, classify_sqlstate};
use audit_store_postgres::AdminError;
use audit_store_postgres::admin::{AuditAdmin, ControlReceipt, Receipt, StoreStatusRow};
use uuid::Uuid;

/// Store-side administrative calls of the relay.
pub trait StoreAdmin: Send + Sync {
    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<Receipt>, AdminError>>;

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: i32,
    ) -> BoxFuture<'a, Result<Vec<Receipt>, AdminError>>;

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceipt>, AdminError>>;

    /// Records a relay control event and returns its seq.
    fn record_relay_control<'a>(
        &'a self,
        event_type: &'a str,
        event_id: Uuid,
        code: &'a str,
        counts: Option<&'a serde_json::Value>,
    ) -> BoxFuture<'a, Result<i64, AdminError>>;

    fn store_status(&self) -> BoxFuture<'_, Result<StoreStatusRow, AdminError>>;

    /// Asks the Store to enter recovery mode after the relay detected that
    /// its acknowledged receipts are missing (design §11). Best effort.
    fn report_regression<'a>(
        &'a self,
        seq: i64,
        event_id: Uuid,
        envelope_digest: &'a [u8; 32],
    ) -> BoxFuture<'a, Result<(), AdminError>>;
}

/// [`StoreAdmin`] over one Store login.
#[derive(Clone, Debug)]
pub struct PgStoreAdmin {
    admin: AuditAdmin,
}

impl PgStoreAdmin {
    pub fn new(admin: AuditAdmin) -> Self {
        Self { admin }
    }

    pub fn admin(&self) -> &AuditAdmin {
        &self.admin
    }
}

impl StoreAdmin for PgStoreAdmin {
    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<Receipt>, AdminError>> {
        Box::pin(self.admin.lookup_receipts(event_ids))
    }

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: i32,
    ) -> BoxFuture<'a, Result<Vec<Receipt>, AdminError>> {
        Box::pin(self.admin.list_source_receipts(source, after_seq, limit))
    }

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceipt>, AdminError>> {
        Box::pin(self.admin.lookup_control_receipts(seqs))
    }

    fn record_relay_control<'a>(
        &'a self,
        event_type: &'a str,
        event_id: Uuid,
        code: &'a str,
        counts: Option<&'a serde_json::Value>,
    ) -> BoxFuture<'a, Result<i64, AdminError>> {
        Box::pin(
            self.admin
                .record_relay_control(event_type, event_id, code, counts),
        )
    }

    fn store_status(&self) -> BoxFuture<'_, Result<StoreStatusRow, AdminError>> {
        Box::pin(self.admin.store_status())
    }

    fn report_regression<'a>(
        &'a self,
        seq: i64,
        event_id: Uuid,
        envelope_digest: &'a [u8; 32],
    ) -> BoxFuture<'a, Result<(), AdminError>> {
        Box::pin(async move {
            sqlx::query("SELECT * FROM audit_store.report_regression($1, $2, $3)")
                .bind(seq)
                .bind(event_id)
                .bind(envelope_digest.to_vec())
                .fetch_all(self.admin.pool())
                .await
                .map(|_| ())
                .map_err(AdminError::from)
        })
    }
}

/// The outage code of a failed administrative call: everything that is not
/// a Store refusal is an outage that holds the work (design §6.3).
pub fn admin_outage_code(error: &AdminError) -> &'static str {
    match error {
        AdminError::RecoveryRequired => OutageCode::RecoveryRequired.as_str(),
        AdminError::PostureInvalid => OutageCode::PostureInvalid.as_str(),
        AdminError::Unavailable { code } => code.as_str(),
        AdminError::Database { sqlstate, .. } => classify_sqlstate(sqlstate)
            .unwrap_or(OutageCode::Unclassified)
            .as_str(),
        AdminError::Denied { .. } | AdminError::Rejected { .. } | AdminError::Protocol(_) => {
            OutageCode::Unclassified.as_str()
        }
    }
}
