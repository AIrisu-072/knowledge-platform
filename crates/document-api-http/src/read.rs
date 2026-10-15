use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Extension, Json, Router};
use document_application::{
    AccessPolicyReadRepository, AccessPolicyReadService, ActionCapabilityReadRepository,
    ActionCapabilityReadService, ApplicationError, AuthoringDocumentSummary, AuthoringQuery,
    DisplayTimestampKind, DocumentDetailPurpose, DocumentDetailRead, DocumentDetailReadService,
    DocumentHistoryEntry, DocumentHistoryRepository, DocumentHistoryService, DocumentListFilter,
    DocumentQueryRepository, DocumentQueryService, DocumentRevisionDetail,
    DocumentRevisionDetailQuery, DocumentRevisionPageQuery, DocumentRevisionReadRepository,
    DocumentRevisionReadService, DocumentRevisionSummary, DocumentSort, EditManifestRepository,
    FolderActionCapabilities, FolderPageQuery, GuiDocumentReadModel, GuiVersionFileSummary,
    GuiVersionSummary, HistoryDocumentSummary, HistoryPageQuery, HistoryQuery,
    IdentityPresentation, IdentityPresentationResolver, IdentityPresentationService, IdentityRef,
    Page, PolicyBindingMode, ProvenanceQuality, PublishedDocumentSummary, PublishedQuery,
    VerifiedActorContext, VersionActionCapabilities, VersionDetail, VersionFileSummary,
    VersionPageQuery, VersionPurpose, VersionRequest, VersionSummary,
};
use document_domain::{
    Action, DocumentId, DocumentVersionId, PolicyGrant, PolicySubjectKind, PolicyTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error::{ApiError, ApiProblem};
use crate::identity::IdentityAdapter;
use crate::limits::ORDINARY_OPERATION_TIMEOUT;
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

pub trait AuthorizedReadRepository:
    DocumentQueryRepository
    + DocumentHistoryRepository
    + EditManifestRepository
    + DocumentRevisionReadRepository
    + ActionCapabilityReadRepository
    + AccessPolicyReadRepository
    + Send
    + Sync
    + 'static
{
}

impl<T> AuthorizedReadRepository for T where
    T: DocumentQueryRepository
        + DocumentHistoryRepository
        + EditManifestRepository
        + DocumentRevisionReadRepository
        + ActionCapabilityReadRepository
        + AccessPolicyReadRepository
        + Send
        + Sync
        + 'static
{
}

struct ReadState<R> {
    repository: Arc<R>,
    identity_presentations: Arc<dyn IdentityPresentationResolver>,
}

impl<R> Clone for ReadState<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            identity_presentations: self.identity_presentations.clone(),
        }
    }
}

#[derive(Clone)]
struct SessionState {
    identity_presentations: Arc<dyn IdentityPresentationResolver>,
}

/// Builds read routes with subject-ID fallback when no presentation source is configured.
/// Hosts with an Identity presentation source should use
/// [`read_router_with_identity_presentation`] to enable display-name enrichment.
pub fn read_router<R: AuthorizedReadRepository>(
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError> {
    read_router_with_identity_presentation(
        repository,
        identity_adapter,
        Arc::new(UnavailableIdentityPresentationResolver),
    )
}

pub fn read_router_with_identity_presentation<R: AuthorizedReadRepository>(
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
    identity_presentations: Arc<dyn IdentityPresentationResolver>,
) -> Result<Router, StartupError> {
    let session_routes = session_routes(identity_presentations.clone());
    let manifest_routes = crate::edit_manifest::edit_manifest_routes(repository.clone());
    let routes = Router::new()
        .route("/v1/documents", get(list_documents::<R>))
        .route("/v1/documents/{document_id}", get(get_document::<R>))
        .route(
            "/v1/documents/{document_id}/versions",
            get(list_versions::<R>),
        )
        .route(
            "/v1/documents/{document_id}/versions/{version_id}",
            get(get_version::<R>),
        )
        .route(
            "/v1/documents/{document_id}/revisions",
            get(list_revisions::<R>),
        )
        .route(
            "/v1/documents/{document_id}/revisions/{revision_id}",
            get(get_revision::<R>),
        )
        .route(
            "/v1/documents/{document_id}/history",
            get(list_history::<R>),
        )
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/files",
            get(list_files::<R>),
        )
        .route(
            "/v1/documents/{document_id}/access-policy",
            get(get_document_policy::<R>),
        )
        .route("/v1/folders/root", get(get_root::<R>))
        .route(
            "/v1/folders/{folder_id}/children",
            get(list_folder_children::<R>),
        )
        .route(
            "/v1/folders/{folder_id}/access-policy",
            get(get_folder_policy::<R>),
        )
        .with_state(ReadState {
            repository,
            identity_presentations,
        })
        .merge(session_routes)
        .merge(manifest_routes);
    protect_routes(
        with_operation_timeout(routes, ORDINARY_OPERATION_TIMEOUT),
        Some(identity_adapter),
    )
}

pub fn session_router(
    identity_adapter: Arc<dyn IdentityAdapter>,
    identity_presentations: Arc<dyn IdentityPresentationResolver>,
) -> Result<Router, StartupError> {
    protect_routes(
        with_operation_timeout(
            session_routes(identity_presentations),
            ORDINARY_OPERATION_TIMEOUT,
        ),
        Some(identity_adapter),
    )
}

fn session_routes(identity_presentations: Arc<dyn IdentityPresentationResolver>) -> Router {
    Router::new()
        .route("/v1/session", get(get_session))
        .with_state(SessionState {
            identity_presentations,
        })
}

struct UnavailableIdentityPresentationResolver;

impl IdentityPresentationResolver for UnavailableIdentityPresentationResolver {
    fn resolve_batch<'a>(
        &'a self,
        _refs: &'a [IdentityRef],
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Vec<IdentityPresentation>,
                        document_application::IdentityPresentationResolutionError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(document_application::IdentityPresentationResolutionError::Unavailable)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentListParams {
    view: Option<String>,
    title_contains: Option<String>,
    folder_id: Option<String>,
    #[serde(default)]
    include_descendants: bool,
    document_type: Option<String>,
    owning_department: Option<String>,
    category: Option<String>,
    created_from: Option<String>,
    created_before: Option<String>,
    sort: Option<String>,
    page_size: Option<u16>,
    cursor: Option<String>,
    unread_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageParams {
    page_size: Option<u16>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurposePageParams {
    purpose: Option<String>,
    page_size: Option<u16>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PurposeParams {
    purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DetailParams {
    view: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishedDocumentDto {
    document_id: Uuid,
    document_version_id: Uuid,
    title: String,
    folder_id: Option<Uuid>,
    folder_name: Option<String>,
    current_version_id: Uuid,
    revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    unread: Option<bool>,
    metadata: Value,
    created_at: String,
    published_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_read_at: Option<String>,
    #[serde(flatten)]
    gui: GuiDocumentFieldsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthoringDocumentDto {
    document_id: Uuid,
    document_version_id: Uuid,
    title: String,
    lifecycle_state: String,
    folder_id: Option<Uuid>,
    folder_name: Option<String>,
    revision: i64,
    current_version_id: Option<Uuid>,
    metadata: Value,
    created_at: String,
    #[serde(flatten)]
    gui: GuiDocumentFieldsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryDocumentDto {
    document_id: Uuid,
    document_version_id: Uuid,
    title: String,
    lifecycle_state: String,
    ended: bool,
    folder_id: Option<Uuid>,
    folder_name: Option<String>,
    revision: i64,
    metadata: Value,
    created_at: String,
    #[serde(flatten)]
    gui: GuiDocumentFieldsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiDocumentFieldsDto {
    display_version: GuiVersionSummaryDto,
    display_revision: Option<DocumentRevisionSummaryDto>,
    read_state: GuiReadStateDto,
    display_timestamp: GuiDisplayTimestampDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiVersionSummaryDto {
    version_id: Uuid,
    version_no: i64,
    base_version_id: Option<Uuid>,
    lifecycle_state: String,
    is_current: bool,
    approved_at: Option<String>,
    scheduled_publish_at: Option<String>,
    published_at: Option<String>,
    withdrawn_at: Option<String>,
    updated_at: String,
    file_summary: GuiVersionFileSummaryDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiVersionFileSummaryDto {
    authoritative_item_count: i64,
    total_size_bytes: i64,
    primary: Option<GuiPrimaryFileSummaryDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiPrimaryFileSummaryDto {
    display_name: String,
    media_type: String,
    size_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiReadStateDto {
    is_read: bool,
    first_read_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiDisplayTimestampDto {
    kind: &'static str,
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentRevisionSummaryDto {
    revision_id: Uuid,
    document_version_id: Uuid,
    major: i64,
    minor: i64,
    label: String,
    created_at: String,
    source_kind: String,
    metadata_snapshot_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionActorDto {
    identity_provider: String,
    principal_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentRevisionDetailDto {
    #[serde(flatten)]
    summary: DocumentRevisionSummaryDto,
    metadata_snapshot: Option<Value>,
    actor: Option<RevisionActorDto>,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "view",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum DocumentListDto {
    Published {
        items: Vec<PublishedDocumentDto>,
        next_cursor: Option<String>,
    },
    Authoring {
        items: Vec<AuthoringDocumentDto>,
        next_cursor: Option<String>,
    },
    History {
        items: Vec<HistoryDocumentDto>,
        next_cursor: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionDto {
    version_id: Uuid,
    version_no: i64,
    base_version_id: Option<Uuid>,
    lifecycle_state: String,
    is_current: bool,
    created_at: String,
    approved_at: Option<String>,
    scheduled_publish_at: Option<String>,
    published_at: Option<String>,
    withdrawn_at: Option<String>,
    updated_at: String,
    file_summary: GuiVersionFileSummaryDto,
    first_read_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionDetailDto {
    #[serde(flatten)]
    version: VersionDto,
    current_publication_schedule_id: Option<Uuid>,
    capabilities: VersionActionCapabilities,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageDto<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDto {
    principal: PrincipalDto,
    presentation: IdentityPresentationDto,
    invocation_kind: &'static str,
    expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrincipalDto {
    identity_provider: String,
    principal_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityPresentationDto {
    #[serde(rename = "ref")]
    reference: IdentityRefDto,
    display_name: Option<String>,
    secondary_text: Option<String>,
    resolution: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityRefDto {
    provider: String,
    kind: &'static str,
    subject_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryEntryDto {
    source_kind: String,
    source_key: String,
    occurred_at: Option<String>,
    actor: Option<ActorDto>,
    action_code: String,
    details: Value,
    provenance_quality: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActorDto {
    identity_provider: String,
    principal_id: String,
    presentation: IdentityPresentationDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDto {
    content_item_id: Uuid,
    representation_id: Uuid,
    logical_path: String,
    ordinal: i32,
    role: String,
    display_name: String,
    media_type: String,
    size_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileListDto {
    items: Vec<FileDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderDto {
    folder_id: Uuid,
    parent_folder_id: Option<Uuid>,
    name: String,
    revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderDetailDto {
    folder_id: Uuid,
    parent_folder_id: Option<Uuid>,
    name: String,
    revision: i64,
    capabilities: FolderActionCapabilities,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderChildrenDto {
    items: Vec<FolderDto>,
    next_cursor: Option<String>,
    capabilities: FolderActionCapabilities,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyTargetDto {
    kind: &'static str,
    id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyGrantDto {
    subject_kind: &'static str,
    identity_provider: String,
    subject_id: String,
    actions: Vec<&'static str>,
    presentation: IdentityPresentationDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccessPolicyDto {
    target: PolicyTargetDto,
    binding_mode: &'static str,
    policy_id: Option<Uuid>,
    policy_revision: i64,
    effective_policy_id: Uuid,
    effective_source: PolicyTargetDto,
    effective_grants: Vec<PolicyGrantDto>,
}

async fn get_session(
    State(state): State<SessionState>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
) -> Result<Json<SessionDto>, ApiError> {
    let path = "/v1/session";
    ctx.ensure_current()
        .map_err(|error| problem(error, path, &trace))?;
    let reference = IdentityRef::from_principal(ctx.principal());
    let presentation = IdentityPresentationService::resolve_batch(
        state.identity_presentations.as_ref(),
        std::slice::from_ref(&reference),
    )
    .await
    .into_iter()
    .next()
    .unwrap_or_else(|| IdentityPresentation::unavailable(reference));
    ctx.ensure_current()
        .map_err(|error| problem(error, path, &trace))?;

    Ok(Json(SessionDto {
        principal: PrincipalDto {
            identity_provider: ctx.principal().identity_provider().to_owned(),
            principal_id: ctx.principal().principal_id().to_owned(),
        },
        presentation: identity_presentation_dto(presentation),
        invocation_kind: ctx.invocation_kind().as_str(),
        expires_at: timestamp(ctx.valid_until()).map_err(|error| problem(error, path, &trace))?,
    }))
}

async fn list_documents<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    query: Result<Query<DocumentListParams>, QueryRejection>,
) -> Result<Json<DocumentListDto>, ApiError> {
    let params = query_params(query, "/v1/documents", &trace)?;
    let view = required(params.view.as_deref(), "view")
        .map_err(|error| problem(error, "/v1/documents", &trace))?;
    let filter =
        document_filter(&params).map_err(|error| problem(error, "/v1/documents", &trace))?;
    let sort = document_sort(params.sort.as_deref())
        .map_err(|error| problem(error, "/v1/documents", &trace))?;
    let service = DocumentQueryService::new(state.repository);
    let response = match view {
        "published" => {
            let page = service
                .list_published_documents(
                    &ctx,
                    PublishedQuery {
                        filter,
                        sort,
                        page_size: params.page_size,
                        cursor: params.cursor,
                        unread_only: params.unread_only.unwrap_or(false),
                    },
                )
                .await
                .map_err(|error| problem(error, "/v1/documents", &trace))?;
            DocumentListDto::Published {
                items: page
                    .items
                    .into_iter()
                    .map(published_dto)
                    .collect::<Result<_, _>>()
                    .map_err(|error| problem(error, "/v1/documents", &trace))?,
                next_cursor: page.next_cursor,
            }
        }
        "authoring" => {
            reject_unread(&params).map_err(|error| problem(error, "/v1/documents", &trace))?;
            let page = service
                .list_authoring_documents(
                    &ctx,
                    AuthoringQuery {
                        filter,
                        sort,
                        page_size: params.page_size,
                        cursor: params.cursor,
                    },
                )
                .await
                .map_err(|error| problem(error, "/v1/documents", &trace))?;
            DocumentListDto::Authoring {
                items: page
                    .items
                    .into_iter()
                    .map(authoring_dto)
                    .collect::<Result<_, _>>()
                    .map_err(|error| problem(error, "/v1/documents", &trace))?,
                next_cursor: page.next_cursor,
            }
        }
        "history" => {
            reject_unread(&params).map_err(|error| problem(error, "/v1/documents", &trace))?;
            let page = service
                .list_history_documents(
                    &ctx,
                    HistoryQuery {
                        filter,
                        sort,
                        page_size: params.page_size,
                        cursor: params.cursor,
                    },
                )
                .await
                .map_err(|error| problem(error, "/v1/documents", &trace))?;
            DocumentListDto::History {
                items: page
                    .items
                    .into_iter()
                    .map(history_document_dto)
                    .collect::<Result<_, _>>()
                    .map_err(|error| problem(error, "/v1/documents", &trace))?,
                next_cursor: page.next_cursor,
            }
        }
        _ => {
            return Err(problem(validation("invalid view"), "/v1/documents", &trace));
        }
    };
    Ok(Json(response))
}

async fn get_document<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    query: Result<Query<DetailParams>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let path = format!("/v1/documents/{document_id}");
    let params = query_params(query, &path, &trace)?;
    let purpose = match required(params.view.as_deref(), "view")
        .map_err(|error| problem(error, &path, &trace))?
    {
        "published" => DocumentDetailPurpose::Published,
        "authoring" => DocumentDetailPurpose::Authoring,
        _ => return Err(problem(validation("invalid view"), &path, &trace)),
    };
    let id = document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let detail = DocumentDetailReadService::new(state.repository.clone())
        .read(&ctx, id, purpose)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let mut value = match detail {
        DocumentDetailRead::Published(summary) => serde_json::to_value(
            published_dto(summary).map_err(|error| problem(error, &path, &trace))?,
        ),
        DocumentDetailRead::Authoring(summary) => serde_json::to_value(
            authoring_dto(summary).map_err(|error| problem(error, &path, &trace))?,
        ),
    }
    .map_err(|_| {
        problem(
            ApplicationError::Internal("response mapping".into()),
            &path,
            &trace,
        )
    })?;
    let capabilities = ActionCapabilityReadService::new(state.repository)
        .read_document(&ctx, id)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    insert_capabilities(&mut value, capabilities).map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(value))
}

async fn list_versions<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    query: Result<Query<PurposePageParams>, QueryRejection>,
) -> Result<Json<PageDto<VersionDto>>, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions");
    let params = query_params(query, &path, &trace)?;
    let purpose = version_purpose(params.purpose.as_deref())
        .map_err(|error| problem(error, &path, &trace))?;
    let page = DocumentHistoryService::new(state.repository)
        .list_document_versions(
            &ctx,
            VersionPageQuery {
                document_id: document_id_value(&document_id)
                    .map_err(|error| problem(error, &path, &trace))?,
                purpose,
                page_size: params.page_size,
                cursor: params.cursor,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(
        version_page_dto(page).map_err(|error| problem(error, &path, &trace))?,
    ))
}

async fn get_version<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
    query: Result<Query<PurposeParams>, QueryRejection>,
) -> Result<Json<VersionDetailDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions/{version_id}");
    let params = query_params(query, &path, &trace)?;
    let request = version_request(&document_id, &version_id, params.purpose.as_deref())
        .map_err(|error| problem(error, &path, &trace))?;
    let detail = DocumentHistoryService::new(state.repository.clone())
        .get_document_version(&ctx, request)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let capabilities = ActionCapabilityReadService::new(state.repository)
        .read_version(&ctx, request)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(VersionDetailDto {
        current_publication_schedule_id: detail.current_publication_schedule_id,
        version: version_detail_dto(detail).map_err(|error| problem(error, &path, &trace))?,
        capabilities,
    }))
}

async fn list_history<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<PageDto<HistoryEntryDto>>, ApiError> {
    let path = format!("/v1/documents/{document_id}/history");
    let params = query_params(query, &path, &trace)?;
    let page = DocumentHistoryService::new(state.repository.clone())
        .list_document_history(
            &ctx,
            HistoryPageQuery {
                document_id: document_id_value(&document_id)
                    .map_err(|error| problem(error, &path, &trace))?,
                page_size: params.page_size,
                cursor: params.cursor,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let actor_refs = page
        .items
        .iter()
        .filter_map(|entry| entry.actor.as_ref())
        .map(IdentityRef::from_principal)
        .collect::<Vec<_>>();
    let actor_presentations = IdentityPresentationService::resolve_batch(
        state.identity_presentations.as_ref(),
        &actor_refs,
    )
    .await;
    let mut actor_presentations = actor_presentations.into_iter();
    let items = page
        .items
        .into_iter()
        .map(|entry| {
            let presentation = entry.actor.as_ref().map(|actor| {
                actor_presentations.next().unwrap_or_else(|| {
                    IdentityPresentation::unavailable(IdentityRef::from_principal(actor))
                })
            });
            history_entry_dto(entry, presentation)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(PageDto {
        items,
        next_cursor: page.next_cursor,
    }))
}

async fn list_revisions<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<PageDto<DocumentRevisionSummaryDto>>, ApiError> {
    let path = format!("/v1/documents/{document_id}/revisions");
    let params = query_params(query, &path, &trace)?;
    let page = DocumentRevisionReadService::new(state.repository)
        .list_document_revisions(
            &ctx,
            DocumentRevisionPageQuery {
                document_id: document_id_value(&document_id)
                    .map_err(|error| problem(error, &path, &trace))?,
                page_size: params.page_size,
                cursor: params.cursor,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let items = page
        .items
        .into_iter()
        .map(revision_summary_dto)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(PageDto {
        items,
        next_cursor: page.next_cursor,
    }))
}

async fn get_revision<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, revision_id)): Path<(String, String)>,
) -> Result<Json<DocumentRevisionDetailDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/revisions/{revision_id}");
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let revision_id = Uuid::parse_str(&revision_id)
        .map_err(|_| problem(validation("invalid revision id"), &path, &trace))?;
    let detail = DocumentRevisionReadService::new(state.repository)
        .get_document_revision(
            &ctx,
            DocumentRevisionDetailQuery {
                document_id,
                revision_id,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(
        document_revision_detail_dto(detail).map_err(|error| problem(error, &path, &trace))?,
    ))
}

async fn list_files<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
    query: Result<Query<PurposeParams>, QueryRejection>,
) -> Result<Json<FileListDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions/{version_id}/files");
    let params = query_params(query, &path, &trace)?;
    let files = DocumentHistoryService::new(state.repository)
        .list_version_files(
            &ctx,
            version_request(&document_id, &version_id, params.purpose.as_deref())
                .map_err(|error| problem(error, &path, &trace))?,
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(FileListDto {
        items: files.into_iter().map(file_dto).collect(),
    }))
}

async fn get_root<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
) -> Result<Json<FolderDetailDto>, ApiError> {
    let root = DocumentQueryService::new(state.repository.clone())
        .get_root_folder(&ctx)
        .await
        .map_err(|error| problem(error, "/v1/folders/root", &trace))?;
    let capabilities = ActionCapabilityReadService::new(state.repository)
        .read_folder(&ctx, root.folder_id)
        .await
        .map_err(|error| problem(error, "/v1/folders/root", &trace))?;
    Ok(Json(FolderDetailDto {
        folder_id: root.folder_id.as_uuid(),
        parent_folder_id: None,
        name: root.name,
        revision: root.revision,
        capabilities,
    }))
}

async fn list_folder_children<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(folder_id): Path<String>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<FolderChildrenDto>, ApiError> {
    let path = format!("/v1/folders/{folder_id}/children");
    let params = query_params(query, &path, &trace)?;
    let parent_folder_id =
        folder_id_value(&folder_id).map_err(|error| problem(error, &path, &trace))?;
    let page = DocumentQueryService::new(state.repository.clone())
        .list_child_folders(
            &ctx,
            FolderPageQuery {
                parent_folder_id,
                page_size: params.page_size,
                cursor: params.cursor,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let capabilities = ActionCapabilityReadService::new(state.repository)
        .read_folder(&ctx, parent_folder_id)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(FolderChildrenDto {
        items: page
            .items
            .into_iter()
            .map(|folder| FolderDto {
                folder_id: folder.folder_id.as_uuid(),
                parent_folder_id: Some(folder.parent_folder_id.as_uuid()),
                name: folder.name,
                revision: folder.revision,
            })
            .collect(),
        next_cursor: page.next_cursor,
        capabilities,
    }))
}

async fn get_document_policy<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
) -> Result<Json<AccessPolicyDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/access-policy");
    policy_response(
        state,
        ctx,
        PolicyTarget::Document(
            document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?,
        ),
        &path,
        &trace,
    )
    .await
}

async fn get_folder_policy<R: AuthorizedReadRepository>(
    State(state): State<ReadState<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(folder_id): Path<String>,
) -> Result<Json<AccessPolicyDto>, ApiError> {
    let path = format!("/v1/folders/{folder_id}/access-policy");
    policy_response(
        state,
        ctx,
        PolicyTarget::Folder(
            folder_id_value(&folder_id).map_err(|error| problem(error, &path, &trace))?,
        ),
        &path,
        &trace,
    )
    .await
}

async fn policy_response<R: AuthorizedReadRepository>(
    state: ReadState<R>,
    ctx: VerifiedActorContext,
    target: PolicyTarget,
    path: &str,
    trace: &TraceContext,
) -> Result<Json<AccessPolicyDto>, ApiError> {
    let policy = AccessPolicyReadService::new(state.repository.clone())
        .read(&ctx, target)
        .await
        .map_err(|error| problem(error, path, trace))?;
    let identity_refs = policy
        .effective_grants
        .iter()
        .map(|grant| IdentityRef::from_policy_subject(grant.subject()))
        .collect::<Vec<_>>();
    let presentations = IdentityPresentationService::resolve_batch(
        state.identity_presentations.as_ref(),
        &identity_refs,
    )
    .await;
    let effective_grants = policy
        .effective_grants
        .into_iter()
        .zip(presentations)
        .map(|(grant, presentation)| policy_grant_dto(grant, presentation))
        .collect();
    Ok(Json(AccessPolicyDto {
        target: policy_target_dto(policy.target),
        binding_mode: match policy.binding_mode {
            PolicyBindingMode::Inherit => "inherit",
            PolicyBindingMode::Explicit => "explicit",
        },
        policy_id: policy.policy_id.map(|id| id.as_uuid()),
        policy_revision: policy.policy_revision,
        effective_policy_id: policy.effective_policy_id.as_uuid(),
        effective_source: policy_target_dto(policy.effective_source),
        effective_grants,
    }))
}

fn query_params<T>(
    query: Result<Query<T>, QueryRejection>,
    path: &str,
    trace: &TraceContext,
) -> Result<T, ApiError> {
    query
        .map(|Query(value)| value)
        .map_err(|_| problem(validation("invalid query"), path, trace))
}

fn required<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, ApplicationError> {
    value.ok_or_else(|| validation(&format!("{field} is required")))
}

fn validation(message: &str) -> ApplicationError {
    ApplicationError::Validation(message.to_owned())
}

fn problem(error: ApplicationError, path: &str, trace: &TraceContext) -> ApiError {
    ApiProblem::from_application(error, path, &trace.trace_id).into()
}

fn document_id_value(value: &str) -> Result<DocumentId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentId::from_uuid)
        .map_err(|_| validation("invalid documentId"))
}

fn folder_id_value(value: &str) -> Result<document_domain::FolderId, ApplicationError> {
    Uuid::parse_str(value)
        .map(document_domain::FolderId::from_uuid)
        .map_err(|_| validation("invalid folderId"))
}

fn version_id_value(value: &str) -> Result<DocumentVersionId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentVersionId::from_uuid)
        .map_err(|_| validation("invalid versionId"))
}

fn document_filter(params: &DocumentListParams) -> Result<DocumentListFilter, ApplicationError> {
    Ok(DocumentListFilter {
        exact_document_id: None,
        title_contains: params.title_contains.clone(),
        folder_id: params
            .folder_id
            .as_deref()
            .map(folder_id_value)
            .transpose()?,
        include_descendants: params.include_descendants,
        document_type: params.document_type.clone(),
        owning_department: params.owning_department.clone(),
        category: params.category.clone(),
        created_from: params
            .created_from
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
        created_before: params
            .created_before
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
    })
}

fn parse_timestamp(value: &str) -> Result<OffsetDateTime, ApplicationError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| validation("invalid date-time"))
}

fn document_sort(value: Option<&str>) -> Result<DocumentSort, ApplicationError> {
    match value.unwrap_or("createdAtDesc") {
        "createdAtDesc" => Ok(DocumentSort::CreatedAtDesc),
        "titleAsc" => Ok(DocumentSort::TitleAsc),
        "publishedAtDesc" => Ok(DocumentSort::PublishedAtDesc),
        _ => Err(validation("invalid sort")),
    }
}

fn reject_unread(params: &DocumentListParams) -> Result<(), ApplicationError> {
    if params.unread_only.is_some() {
        Err(validation("unreadOnly is only valid for published view"))
    } else {
        Ok(())
    }
}

fn version_purpose(value: Option<&str>) -> Result<VersionPurpose, ApplicationError> {
    match required(value, "purpose")? {
        "published" => Ok(VersionPurpose::Published),
        "authoring" => Ok(VersionPurpose::Authoring),
        "history" => Ok(VersionPurpose::History),
        _ => Err(validation("invalid purpose")),
    }
}

fn version_request(
    document_id: &str,
    version_id: &str,
    purpose: Option<&str>,
) -> Result<VersionRequest, ApplicationError> {
    Ok(VersionRequest {
        document_id: document_id_value(document_id)?,
        document_version_id: version_id_value(version_id)?,
        purpose: version_purpose(purpose)?,
    })
}

fn timestamp(value: OffsetDateTime) -> Result<String, ApplicationError> {
    value
        .format(&Rfc3339)
        .map_err(|_| ApplicationError::IntegrityViolation)
}

fn optional_timestamp(value: Option<OffsetDateTime>) -> Result<Option<String>, ApplicationError> {
    value.map(timestamp).transpose()
}

fn gui_fields_dto(value: GuiDocumentReadModel) -> Result<GuiDocumentFieldsDto, ApplicationError> {
    let first_read_at = optional_timestamp(value.read_state.first_read_at)?;
    let display_timestamp = GuiDisplayTimestampDto {
        kind: match value.display_timestamp.kind {
            DisplayTimestampKind::RevisionCreatedAt => "revisionCreatedAt",
            DisplayTimestampKind::WorkingUpdatedAt => "workingUpdatedAt",
        },
        value: timestamp(value.display_timestamp.value)?,
    };
    Ok(GuiDocumentFieldsDto {
        display_version: gui_version_summary_dto(value.display_version)?,
        display_revision: value
            .display_revision
            .map(revision_summary_dto)
            .transpose()?,
        read_state: GuiReadStateDto {
            is_read: value.read_state.is_read(),
            first_read_at,
        },
        display_timestamp,
    })
}

fn insert_capabilities<T: Serialize>(
    value: &mut Value,
    capabilities: T,
) -> Result<(), ApplicationError> {
    let encoded =
        serde_json::to_value(capabilities).map_err(|_| ApplicationError::IntegrityViolation)?;
    value
        .as_object_mut()
        .ok_or(ApplicationError::IntegrityViolation)?
        .insert("capabilities".into(), encoded);
    Ok(())
}

fn gui_version_summary_dto(
    value: GuiVersionSummary,
) -> Result<GuiVersionSummaryDto, ApplicationError> {
    Ok(GuiVersionSummaryDto {
        version_id: value.document_version_id.as_uuid(),
        version_no: value.version_no,
        base_version_id: value.base_document_version_id.map(|id| id.as_uuid()),
        lifecycle_state: value.lifecycle_state,
        is_current: value.is_current,
        approved_at: optional_timestamp(value.approved_at)?,
        scheduled_publish_at: optional_timestamp(value.scheduled_publish_at)?,
        published_at: optional_timestamp(value.published_at)?,
        withdrawn_at: optional_timestamp(value.withdrawn_at)?,
        updated_at: timestamp(value.updated_at)?,
        file_summary: gui_file_summary_dto(value.file_summary),
    })
}

fn gui_file_summary_dto(value: GuiVersionFileSummary) -> GuiVersionFileSummaryDto {
    GuiVersionFileSummaryDto {
        authoritative_item_count: value.authoritative_item_count,
        total_size_bytes: value.total_size_bytes,
        primary: value.primary.map(|primary| GuiPrimaryFileSummaryDto {
            display_name: primary.display_name,
            media_type: primary.media_type,
            size_bytes: primary.size_bytes,
        }),
    }
}

fn revision_summary_dto(
    value: DocumentRevisionSummary,
) -> Result<DocumentRevisionSummaryDto, ApplicationError> {
    let metadata_snapshot_status = match value.metadata_snapshot_status.as_str() {
        "complete" => "complete",
        "unavailable_legacy" => "unavailableLegacy",
        _ => return Err(ApplicationError::IntegrityViolation),
    };
    Ok(DocumentRevisionSummaryDto {
        revision_id: value.revision_id,
        document_version_id: value.document_version_id.as_uuid(),
        major: value.major_no,
        minor: value.minor_no,
        label: format!("{}.{}", value.major_no, value.minor_no),
        created_at: timestamp(value.created_at)?,
        source_kind: value.source_kind,
        metadata_snapshot_status: metadata_snapshot_status.to_owned(),
    })
}

fn document_revision_detail_dto(
    value: DocumentRevisionDetail,
) -> Result<DocumentRevisionDetailDto, ApplicationError> {
    let actor = value.actor.map(|principal| RevisionActorDto {
        identity_provider: principal.identity_provider().to_owned(),
        principal_id: principal.principal_id().to_owned(),
    });
    Ok(DocumentRevisionDetailDto {
        summary: revision_summary_dto(value.summary)?,
        metadata_snapshot: value.metadata_snapshot,
        actor,
        reason: value.reason,
    })
}

fn published_dto(
    value: PublishedDocumentSummary,
) -> Result<PublishedDocumentDto, ApplicationError> {
    Ok(PublishedDocumentDto {
        document_id: value.document_id.as_uuid(),
        document_version_id: value.document_version_id.as_uuid(),
        title: value.title,
        folder_id: value.folder_id.map(|id| id.as_uuid()),
        folder_name: value.folder_name,
        current_version_id: value.document_version_id.as_uuid(),
        revision: value.document_revision,
        unread: Some(!value.gui.read_state.is_read()),
        metadata: value.document_metadata,
        created_at: timestamp(value.created_at)?,
        published_at: timestamp(value.published_at)?,
        first_read_at: optional_timestamp(value.first_read_at)?,
        gui: gui_fields_dto(value.gui)?,
    })
}

fn authoring_dto(
    value: AuthoringDocumentSummary,
) -> Result<AuthoringDocumentDto, ApplicationError> {
    Ok(AuthoringDocumentDto {
        document_id: value.document_id.as_uuid(),
        document_version_id: value.document_version_id.as_uuid(),
        title: value.title,
        lifecycle_state: value.lifecycle_state.to_ascii_lowercase(),
        folder_id: value.folder_id.map(|id| id.as_uuid()),
        folder_name: value.folder_name,
        revision: value.document_revision,
        current_version_id: value.current_version_id.map(|id| id.as_uuid()),
        metadata: value.document_metadata,
        created_at: timestamp(value.created_at)?,
        gui: gui_fields_dto(value.gui)?,
    })
}

fn history_document_dto(
    value: HistoryDocumentSummary,
) -> Result<HistoryDocumentDto, ApplicationError> {
    Ok(HistoryDocumentDto {
        document_id: value.document_id.as_uuid(),
        document_version_id: value.document_version_id.as_uuid(),
        title: value.title,
        lifecycle_state: value.lifecycle_state.to_ascii_lowercase(),
        ended: value.ended,
        folder_id: value.folder_id.map(|id| id.as_uuid()),
        folder_name: value.folder_name,
        revision: value.document_revision,
        metadata: value.document_metadata,
        created_at: timestamp(value.created_at)?,
        gui: gui_fields_dto(value.gui)?,
    })
}

fn version_page_dto(page: Page<VersionSummary>) -> Result<PageDto<VersionDto>, ApplicationError> {
    Ok(PageDto {
        items: page
            .items
            .into_iter()
            .map(version_summary_dto)
            .collect::<Result<_, _>>()?,
        next_cursor: page.next_cursor,
    })
}

fn version_summary_dto(value: VersionSummary) -> Result<VersionDto, ApplicationError> {
    Ok(VersionDto {
        version_id: value.document_version_id.as_uuid(),
        version_no: value.version_no,
        base_version_id: value.base_document_version_id.map(|id| id.as_uuid()),
        lifecycle_state: value.lifecycle_state.to_ascii_lowercase(),
        is_current: value.is_current,
        created_at: timestamp(value.created_at)?,
        approved_at: optional_timestamp(value.approved_at)?,
        scheduled_publish_at: optional_timestamp(value.scheduled_publish_at)?,
        published_at: optional_timestamp(value.published_at)?,
        withdrawn_at: optional_timestamp(value.withdrawn_at)?,
        updated_at: timestamp(value.updated_at)?,
        file_summary: gui_file_summary_dto(value.file_summary),
        first_read_at: optional_timestamp(value.first_read_at)?,
        title: None,
        metadata: None,
    })
}

fn version_detail_dto(value: VersionDetail) -> Result<VersionDto, ApplicationError> {
    let mut dto = version_summary_dto(value.summary)?;
    dto.title = Some(value.title);
    dto.metadata = Some(value.metadata);
    Ok(dto)
}

fn history_entry_dto(
    value: DocumentHistoryEntry,
    presentation: Option<IdentityPresentation>,
) -> Result<HistoryEntryDto, ApplicationError> {
    Ok(HistoryEntryDto {
        source_kind: value.source_kind,
        source_key: value.source_key,
        occurred_at: optional_timestamp(value.occurred_at)?,
        actor: value.actor.map(|actor| {
            let reference = IdentityRef::from_principal(&actor);
            let presentation =
                presentation.unwrap_or_else(|| IdentityPresentation::unavailable(reference));
            ActorDto {
                identity_provider: actor.identity_provider().to_owned(),
                principal_id: actor.principal_id().to_owned(),
                presentation: identity_presentation_dto(presentation),
            }
        }),
        action_code: value.action_code,
        details: value.details,
        provenance_quality: match value.provenance_quality {
            ProvenanceQuality::OperationLedger => "operationLedger",
            ProvenanceQuality::VersionFallback => "versionFallback",
            ProvenanceQuality::LegacyUnknown => "legacyUnknown",
        },
    })
}

fn identity_presentation_dto(value: IdentityPresentation) -> IdentityPresentationDto {
    IdentityPresentationDto {
        reference: IdentityRefDto {
            provider: value.reference.provider,
            kind: value.reference.kind.as_str(),
            subject_id: value.reference.subject_id,
        },
        display_name: value.display_name,
        secondary_text: value.secondary_text,
        resolution: value.resolution.as_str(),
    }
}

fn file_dto(value: VersionFileSummary) -> FileDto {
    FileDto {
        content_item_id: value.content_item_id,
        representation_id: value.representation_id,
        logical_path: value.logical_path,
        ordinal: value.ordinal,
        role: value.role,
        display_name: value.safe_display_name,
        media_type: value.media_type,
        size_bytes: value.size_bytes,
    }
}

fn policy_target_dto(value: PolicyTarget) -> PolicyTargetDto {
    match value {
        PolicyTarget::Document(id) => PolicyTargetDto {
            kind: "document",
            id: id.as_uuid(),
        },
        PolicyTarget::Folder(id) => PolicyTargetDto {
            kind: "folder",
            id: id.as_uuid(),
        },
    }
}

fn policy_grant_dto(value: PolicyGrant, presentation: IdentityPresentation) -> PolicyGrantDto {
    let subject = value.subject();
    PolicyGrantDto {
        subject_kind: match subject.kind() {
            PolicySubjectKind::Principal => "principal",
            PolicySubjectKind::Group => "group",
            PolicySubjectKind::Role => "role",
        },
        identity_provider: subject.identity_provider().to_owned(),
        subject_id: subject.subject_id().to_owned(),
        actions: value
            .actions()
            .iter()
            .map(|action| match action {
                Action::Read => "read",
                Action::ReadHistory => "readHistory",
                Action::Write => "write",
                Action::Publish => "publish",
                Action::Administer => "administer",
            })
            .collect(),
        presentation: identity_presentation_dto(presentation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document_application::ActionAvailability;

    fn version_detail() -> VersionDetailDto {
        let available = ActionAvailability::available();
        VersionDetailDto {
            version: VersionDto {
                version_id: Uuid::now_v7(),
                version_no: 1,
                base_version_id: None,
                lifecycle_state: "working".into(),
                is_current: false,
                created_at: "2026-10-05T00:00:00Z".into(),
                approved_at: None,
                scheduled_publish_at: None,
                published_at: None,
                withdrawn_at: None,
                updated_at: "2026-10-05T00:00:00Z".into(),
                file_summary: GuiVersionFileSummaryDto {
                    authoritative_item_count: 0,
                    total_size_bytes: 0,
                    primary: None,
                },
                first_read_at: None,
                title: Some("予約対象".into()),
                metadata: Some(serde_json::json!({})),
            },
            current_publication_schedule_id: None,
            capabilities: VersionActionCapabilities {
                edit: available,
                rebase: available,
                publish: available,
                withdraw: available,
                schedule_publication: available,
                cancel_publication_schedule: available,
                download: available,
            },
        }
    }

    #[test]
    fn version_detail_serializes_current_publication_schedule_id_as_explicit_null() {
        let body = serde_json::to_value(version_detail()).unwrap();
        assert_eq!(body.get("currentPublicationScheduleId"), Some(&Value::Null));
    }

    #[test]
    fn version_detail_serializes_the_exact_schedule_id_without_adding_it_to_version_summary() {
        let schedule_id = Uuid::now_v7();
        let mut detail = version_detail();
        detail.current_publication_schedule_id = Some(schedule_id);
        let summary = serde_json::to_value(&detail.version).unwrap();
        assert!(summary.get("currentPublicationScheduleId").is_none());
        let body = serde_json::to_value(detail).unwrap();
        assert_eq!(
            body["currentPublicationScheduleId"],
            schedule_id.to_string()
        );
    }
}
