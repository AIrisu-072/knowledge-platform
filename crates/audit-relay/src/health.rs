//! `audit-relay health` (design §12): content-free JSON that keeps the four
//! kinds of evidence apart (design §1.7):
//! - produced: staged rows and their registration,
//! - delivered: acknowledged deliveries and the queue,
//! - stored: the Store head and gate,
//! - verified: the last origin=store `audit.integrity.verified`.
//!
//! Labels are fixed codes; no principal, resource or payload appears.

use std::collections::BTreeMap;
use std::sync::Arc;

use audit_core::DocumentStagingProjection;
use audit_core::codes::RejectionCode;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::breaker::{Regression, acked_head, check_regression};
use crate::handler::CATALOG_SKEW;
use crate::reconcile::Reconciler;
use crate::session::posture;
use crate::store_admin::{StoreAdmin, admin_outage_code};

/// What the health report includes beyond the defaults.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthOptions {
    /// Forecast quarantine codes of the rows not yet delivered.
    pub forecast: bool,
    /// Run the reconcile classification (read-only, nothing recorded).
    pub reconcile: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HealthError {
    #[error("source database error ({0})")]
    Source(String),
}

fn source_error(error: sqlx::Error) -> HealthError {
    HealthError::Source(match &error {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
        _ => "unavailable".to_owned(),
    })
}

/// The forecast code of one projection: what the handler would do.
pub fn forecast_code(row: &DocumentStagingProjection) -> &'static str {
    if !row.source_intact {
        return RejectionCode::SourceDigestMismatch.as_str();
    }
    match audit_core::project(row) {
        Ok(_) => "deliverable",
        Err(rejection) => match rejection.code {
            RejectionCode::UnknownEventType | RejectionCode::UnknownField => CATALOG_SKEW,
            code => code.as_str(),
        },
    }
}

async fn forecast(source: &PgPool) -> Result<BTreeMap<&'static str, i64>, HealthError> {
    let mut counts = BTreeMap::new();
    let mut after: Option<Uuid> = None;
    loop {
        let rows =
            sqlx::query("SELECT event_id, projection FROM audit_relay.preview_pending($1, 500)")
                .bind(after)
                .fetch_all(source)
                .await
                .map_err(source_error)?;
        if rows.is_empty() {
            break;
        }
        for row in &rows {
            let id: Uuid = row.try_get("event_id").map_err(source_error)?;
            after = Some(id);
            let projection: Value = row.try_get("projection").map_err(source_error)?;
            let code = match serde_json::from_value::<DocumentStagingProjection>(projection) {
                Ok(row) => forecast_code(&row),
                Err(_) => crate::handler::PROJECTION_INVALID,
            };
            *counts.entry(code).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

fn count(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
}

/// Builds the health JSON.
pub async fn health(
    source: &PgPool,
    admin: Arc<dyn StoreAdmin>,
    options: HealthOptions,
) -> Result<Value, HealthError> {
    let status: Value = sqlx::query_scalar("SELECT audit_relay.status()")
        .fetch_one(source)
        .await
        .map_err(source_error)?;
    let violations = posture(source).await.map_err(source_error)?;
    let mut alarms: Vec<String> = Vec::new();

    let store = admin.store_status().await;
    let stored = match &store {
        Ok(store) => {
            let gate = if store.recovery_mode {
                "store_recovery_required".to_owned()
            } else if !store.posture_ok {
                "store_posture_invalid".to_owned()
            } else {
                match acked_head(source, store.recovery_epoch)
                    .await
                    .map_err(source_error)?
                {
                    None => "ok".to_owned(),
                    Some(acked) => {
                        match check_regression(admin.as_ref(), store.head_seq, &acked).await {
                            Regression::Intact => "ok".to_owned(),
                            Regression::Regressed => "store_regressed".to_owned(),
                            Regression::Unavailable(code) => code.as_str().to_owned(),
                        }
                    }
                }
            };
            json!({
                "available": gate == "ok",
                "gate": gate,
                "head_seq": store.head_seq,
                "recovery_epoch": store.recovery_epoch,
                "recovery_mode": store.recovery_mode,
                "posture_ok": store.posture_ok,
            })
        }
        Err(error) => json!({
            "available": false,
            "gate": admin_outage_code(error),
            "head_seq": null,
            "recovery_epoch": null,
            "recovery_mode": null,
            "posture_ok": null,
        }),
    };
    let gate = stored["gate"].as_str().unwrap_or("unknown").to_owned();
    if gate != "ok" {
        alarms.push(match gate.as_str() {
            "store_recovery_required" | "store_posture_invalid" | "store_regressed" => gate.clone(),
            _ => "store_unavailable".to_owned(),
        });
    }
    let verified = match &store {
        Ok(store) => json!({
            "last_verified_seq": store.last_verified_seq,
            "last_verified_at": store.last_verified_at,
            "outcome": store.last_verified_outcome,
            "unverified_events": store.head_seq - store.last_verified_seq.unwrap_or(0),
        }),
        Err(_) => json!({
            "last_verified_seq": null, "last_verified_at": null, "outcome": null,
            "unverified_events": null,
        }),
    };
    if let Ok(store) = &store
        && store
            .last_verified_outcome
            .as_deref()
            .is_some_and(|outcome| outcome != "ok")
    {
        alarms.push("verification_failed".into());
    }

    if count(&status, "unregistered") > 0 {
        alarms.push("unregistered_rows".into());
    }
    if count(&status["registration"], "repair") > 0 {
        alarms.push("repair_registered".into());
    }
    if count(&status, "catalog_skew_held") > 0 {
        alarms.push("catalog_skew_held".into());
    }
    if count(&status, "outage_held") > 0 {
        alarms.push("outage_held".into());
    }
    if count(&status, "quarantined_total") > 0 {
        alarms.push("quarantined".into());
    }
    let installed = &status["installed"];
    if installed
        .as_object()
        .is_some_and(|flags| flags.values().any(|flag| flag != &Value::Bool(true)))
    {
        alarms.push("installation_incomplete".into());
    }
    if !violations.is_empty() {
        alarms.push("relay_posture_invalid".into());
    }

    let forecast_value = if options.forecast {
        json!(forecast(source).await?)
    } else {
        Value::Null
    };
    let reconcile_value = if options.reconcile {
        match Reconciler::new(source.clone(), admin.clone())
            .classify()
            .await
        {
            Ok(classification) => {
                for alarm in classification.counts.alarms() {
                    if alarm != "unregistered" {
                        alarms.push(alarm.to_owned());
                    }
                }
                json!({
                    "watermark": classification.watermark,
                    "counts": classification.counts,
                })
            }
            Err(_) => {
                alarms.push("reconcile_unavailable".into());
                Value::Null
            }
        }
    } else {
        Value::Null
    };
    alarms.sort();
    alarms.dedup();

    let mut posture_codes: Vec<&str> = violations.iter().map(|v| v.violation.as_str()).collect();
    posture_codes.sort_unstable();
    posture_codes.dedup();
    Ok(json!({
        "produced": {
            "staged": count(&status, "staged"),
            "registered": count(&status, "registered"),
            "unregistered": count(&status, "unregistered"),
            "registration": status["registration"],
            "legacy_marked": count(&status, "legacy_marked"),
        },
        "delivered": {
            "delivered": count(&status, "delivered"),
            "pending": count(&status, "pending"),
            "leased": count(&status, "leased"),
            "retry_waiting": count(&status, "retry_waiting"),
            "outage_held": count(&status, "outage_held"),
            "catalog_skew_held": count(&status, "catalog_skew_held"),
            "quarantined": status["quarantined"],
            "quarantined_total": count(&status, "quarantined_total"),
            "oldest_pending_age_seconds": status["oldest_pending_age_seconds"],
            "max_acked_store_seq": status["max_acked_store_seq"],
            "replayed": count(&status, "replayed"),
        },
        "stored": stored,
        "verified": verified,
        "installation": {
            "installed": status["installed"],
            "policy_revision": status["policy_revision"],
            "posture_violations": posture_codes,
        },
        "forecast": forecast_value,
        "reconcile": reconcile_value,
        "alarms": alarms,
    }))
}
