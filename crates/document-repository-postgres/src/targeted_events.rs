use document_application::RepositoryError;
use document_domain::{PrincipalRef, ResourceRef};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_statement_error;

pub(crate) fn resource_parts(resource: ResourceRef) -> (&'static str, Uuid) {
    match resource {
        ResourceRef::Document(id) => ("Document", id.as_uuid()),
        ResourceRef::Folder(id) => ("Folder", id.as_uuid()),
        ResourceRef::AccessPolicy(id) => ("AccessPolicy", id.as_uuid()),
    }
}

pub(crate) async fn insert_targeted_events(
    tx: &mut Transaction<'_, Postgres>,
    resource: ResourceRef,
    domain_type: &'static str,
    audit_type: &'static str,
    actor: &PrincipalRef,
    occurred_at: OffsetDateTime,
    payload: Value,
) -> Result<(), RepositoryError> {
    let (resource_type, resource_id) = resource_parts(resource);
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$6)",
    )
    .bind(Uuid::now_v7())
    .bind(domain_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(payload.clone())
    .bind(occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(map_statement_error)?;

    sqlx::query(
        "INSERT INTO audit_outbox_events \
         (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
          resource_type,resource_id,resource_version_id,result,data,occurred_at) \
         VALUES ($1,$2,'urn:knowledge-platform:document-platform',$3,$4,$5,$6,$7,NULL,'success',$8,$9)",
    )
    .bind(Uuid::now_v7())
    .bind(audit_type)
    .bind(format!("{}/{}", resource_type.to_ascii_lowercase(), resource_id))
    .bind(actor.identity_provider())
    .bind(actor.principal_id())
    .bind(resource_type)
    .bind(resource_id)
    .bind(payload)
    .bind(occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    Ok(())
}

pub(crate) async fn record_authorization_denied(
    pool: &sqlx::PgPool,
    actor: &PrincipalRef,
    action_code: &'static str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO audit_outbox_events \
         (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
          resource_type,resource_id,resource_version_id,result,data,occurred_at) \
         VALUES ($1,'authorization.denied','urn:knowledge-platform:document-platform', \
                 'authorization/denied',$2,$3,'AccessPolicy',$4,NULL,'denied',$5,now())",
    )
    .bind(Uuid::now_v7())
    .bind(actor.identity_provider())
    .bind(actor.principal_id())
    .bind(Uuid::nil())
    .bind(json!({"action_code": action_code, "reason_code": "forbidden"}))
    .execute(pool)
    .await
    .map_err(map_statement_error)?;
    Ok(())
}
