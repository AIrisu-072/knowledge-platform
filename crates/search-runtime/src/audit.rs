//! B5 (P7-R04A source side): the typed, append-only Search Audit source.
//!
//! Each producer appends its event on the same connection and transaction as
//! the business change, so the change and its Audit row commit together; an
//! INSERT failure rolls the change back. The row carries closed classes and
//! bounded non-secret references only — never a token, query, content,
//! provider location or free-form data. Delivery to the Audit store is a
//! separate concern with its own state columns.

use sqlx::PgConnection;
use uuid::Uuid;

/// One typed Search/system Audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAuditEvent {
    event_id: Uuid,
    event_class: &'static str,
    event_type: &'static str,
    origin_component: &'static str,
    actor_kind: &'static str,
    actor_ref: Option<String>,
    subject_kind: &'static str,
    subject_ref: Option<String>,
    result: &'static str,
    reason_code: &'static str,
}

impl SearchAuditEvent {
    /// The host registration inventory moved to a new revision.
    pub fn host_registration_changed(writer_ref: &str, revision_ref: &str) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_class: "CONFIGURATION",
            event_type: "host.registration.changed",
            origin_component: "search.host_inventory",
            actor_kind: "VerifiedPrincipal",
            actor_ref: Some(writer_ref.to_owned()),
            subject_kind: "HostInventory",
            subject_ref: Some(revision_ref.to_owned()),
            result: "SUCCEEDED",
            reason_code: "PUBLISHED",
        }
    }

    pub fn event_id(&self) -> Uuid {
        self.event_id
    }
}

/// Appends `event` inside the caller's transaction.
pub async fn append_search_audit_on(
    connection: &mut PgConnection,
    event: &SearchAuditEvent,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO search_audit_outbox_events (event_id,schema_version,event_class, \
         event_type,origin_component,actor_kind,actor_ref,subject_kind,subject_ref, \
         result,reason_code,occurred_at) \
         VALUES ($1,'v1',$2,$3,$4,$5,$6,$7,$8,$9,$10,clock_timestamp())",
    )
    .bind(event.event_id)
    .bind(event.event_class)
    .bind(event.event_type)
    .bind(event.origin_component)
    .bind(event.actor_kind)
    .bind(&event.actor_ref)
    .bind(event.subject_kind)
    .bind(&event.subject_ref)
    .bind(event.result)
    .bind(event.reason_code)
    .execute(connection)
    .await?;
    Ok(())
}
