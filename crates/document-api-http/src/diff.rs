use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Extension, Json, Router};
use document_application::document_diff::{
    AncillaryChange, AuthorizedDiffDisplay, Change, ComparisonRow, ComparisonRowState, DiffCache,
    DiffDisplayItem, DiffExecutor, DiffInspectionEvidence, DiffRequest, DocumentDiffRepository,
    DocumentDiffService, LocatorGranularity, SourceEvidence, UnverifiedRegion,
    project_comparison_table,
};
use document_application::{
    DocumentRevisionReadRepository, FileStorage, MetadataChange, RevisionComparisonService,
    RevisionContentComparison, VerifiedActorContext, VersionFileAccessRepository,
};
use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, MAX_DISPLAY_PAGE_BYTES_V0,
    RelocationKind, SourceLocator, UnverifiedReason,
};
use document_diff_core::{DisplayCell, DisplayFragment, DisplayUnavailableReason};
use document_domain::{DocumentVersionId, FileId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::identity::IdentityAdapter;
use crate::limits::DIFF_OPERATION_TIMEOUT;
use crate::management::{document_id_value, problem, validation};
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

pub trait DiffApiRepository:
    DocumentDiffRepository
    + DocumentRevisionReadRepository
    + VersionFileAccessRepository
    + DiffCache
    + Send
    + Sync
    + 'static
{
}

impl<T> DiffApiRepository for T where
    T: DocumentDiffRepository
        + DocumentRevisionReadRepository
        + VersionFileAccessRepository
        + DiffCache
        + Send
        + Sync
        + 'static
{
}

struct DiffState<R, F, E, I> {
    repository: Arc<R>,
    storage: Arc<F>,
    executor: Arc<E>,
    inspection: Arc<I>,
}

impl<R, F, E, I> Clone for DiffState<R, F, E, I> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            storage: self.storage.clone(),
            executor: self.executor.clone(),
            inspection: self.inspection.clone(),
        }
    }
}

pub fn diff_router<R, F, E, I>(
    repository: Arc<R>,
    storage: Arc<F>,
    executor: Arc<E>,
    inspection: Arc<I>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError>
where
    R: DiffApiRepository,
    F: FileStorage + 'static,
    E: DiffExecutor + 'static,
    I: DiffInspectionEvidence + 'static,
{
    let routes = Router::new()
        .route(
            "/v1/documents/{document_id}/comparisons",
            post(compare::<R, F, E, I>),
        )
        .route(
            "/v1/documents/{document_id}/revision-comparisons",
            post(compare_revisions::<R, F, E, I>),
        )
        .with_state(DiffState {
            repository,
            storage,
            executor,
            inspection,
        });
    protect_routes(
        with_operation_timeout(routes, DIFF_OPERATION_TIMEOUT),
        Some(identity_adapter),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComparisonRequest {
    base_version_id: Uuid,
    target_version_id: Uuid,
    profile: ProfileRequest,
    projection: ProjectionRequest,
    page_size: Option<u16>,
    cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum ProfileRequest {
    #[serde(rename = "document-diff-v0")]
    V0,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProjectionRequest {
    Diff,
    ComparisonTable,
    Display,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevisionComparisonRequest {
    base_revision_id: Uuid,
    target_revision_id: Uuid,
    projection: ProjectionRequest,
    page_size: Option<u16>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionComparisonResponseDto {
    projection: &'static str,
    base_revision: RevisionDto,
    target_revision: RevisionDto,
    content_comparison_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_digest: Option<String>,
    changes: Vec<ChangeDto>,
    rows: Vec<ComparisonRowDto>,
    unverified_regions: Vec<UnverifiedRegionDto>,
    ancillary_changes: Vec<AncillaryChangeDto>,
    metadata_comparison_status: &'static str,
    metadata_changes: Vec<MetadataChangeDto>,
    base_metadata_snapshot_digest: Option<String>,
    target_metadata_snapshot_digest: Option<String>,
    audit_event_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_audit_event_id: Option<Uuid>,
    display_items: Vec<DiffDisplayItemDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_size: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_audit_event_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_result_audit_event_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionDto {
    revision_id: Uuid,
    document_version_id: Uuid,
    major: i64,
    minor: i64,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataChangeDto {
    path: String,
    base_value: Option<serde_json::Value>,
    target_value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ComparisonResponse {
    Diff(DiffProjectionDto),
    Table(ComparisonTableProjectionDto),
    Display(DiffDisplayProjectionDto),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffDisplayProjectionDto {
    projection: &'static str,
    verdict: &'static str,
    coverage: &'static str,
    result_digest: String,
    items: Vec<DiffDisplayItemDto>,
    unverified_regions: Vec<UnverifiedRegionDto>,
    page_size: u16,
    next_cursor: Option<String>,
    audit_event_id: Uuid,
    result_audit_event_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffDisplayItemDto {
    change_index: u32,
    operation: Option<&'static str>,
    relocation: Option<&'static str>,
    facet: String,
    base_locator: Option<SourceLocatorDto>,
    target_locator: Option<SourceLocatorDto>,
    base: Option<DisplayFragmentDto>,
    target: Option<DisplayFragmentDto>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum DisplayFragmentDto {
    Text {
        text: String,
        truncated: bool,
        locator: SourceLocatorDto,
    },
    Table {
        cells: Vec<DisplayCell>,
        truncated: bool,
        locator: SourceLocatorDto,
    },
    Structural {
        summary: String,
        locator: SourceLocatorDto,
    },
    Unavailable {
        reason: &'static str,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffProjectionDto {
    projection: &'static str,
    verdict: &'static str,
    coverage: &'static str,
    result_digest: String,
    changes: Vec<ChangeDto>,
    unverified_regions: Vec<UnverifiedRegionDto>,
    ancillary_changes: Vec<AncillaryChangeDto>,
    audit_event_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonTableProjectionDto {
    projection: &'static str,
    verdict: &'static str,
    coverage: &'static str,
    result_digest: String,
    rows: Vec<ComparisonRowDto>,
    unverified_regions: Vec<UnverifiedRegionDto>,
    audit_event_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeDto {
    operation: Option<&'static str>,
    relocation: Option<&'static str>,
    facet: String,
    base: Option<SourceEvidenceDto>,
    target: Option<SourceEvidenceDto>,
    reason_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UnverifiedRegionDto {
    reason: &'static str,
    base: Option<SourceEvidenceDto>,
    target: Option<SourceEvidenceDto>,
    navigation_hint: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AncillaryChangeDto {
    kind: String,
    base_digest: Option<String>,
    target_digest: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonRowDto {
    state: &'static str,
    facet: String,
    operation: Option<&'static str>,
    relocation: Option<&'static str>,
    base: Option<SourceEvidenceDto>,
    target: Option<SourceEvidenceDto>,
    reason: String,
    navigation_hint: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceEvidenceDto {
    document_id: Uuid,
    version_id: Uuid,
    content_item_id: Uuid,
    representation_id: Uuid,
    file_id: Uuid,
    raw_sha256: String,
    inspection_profile: &'static str,
    locator: SourceLocatorDto,
    granularity: &'static str,
    parser_provenance: String,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SourceLocatorDto {
    ContentItem,
    TextSpan {
        line: u32,
        byte_start: u32,
        byte_end: u32,
    },
    CsvCell {
        row: u32,
        column: u32,
    },
    HtmlNode {
        path: String,
    },
    OfficePath {
        path: String,
    },
    SheetCell {
        sheet: String,
        cell: String,
    },
    VbaModule {
        module: String,
        procedure: Option<String>,
    },
    SlideObject {
        slide: u32,
        object: Option<String>,
    },
    PdfPage {
        page: u32,
        region: Option<[u32; 4]>,
    },
}

async fn compare<R, F, E, I>(
    State(state): State<DiffState<R, F, E, I>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    payload: Result<Json<ComparisonRequest>, JsonRejection>,
) -> Result<Json<ComparisonResponse>, ApiError>
where
    R: DiffApiRepository,
    F: FileStorage + 'static,
    E: DiffExecutor + 'static,
    I: DiffInspectionEvidence + 'static,
{
    let path = format!("/v1/documents/{document_id}/comparisons");
    let request = payload
        .map(|Json(request)| request)
        .map_err(|_| problem(validation("invalid comparison request"), &path, &trace))?;
    let profile = match request.profile {
        ProfileRequest::V0 => DiffProfileVersion::V0,
    };
    let service = DocumentDiffService::new(
        state.repository,
        state.storage,
        state.executor,
        state.inspection,
    );
    let diff_request = DiffRequest {
        document_id: document_id_value(&document_id)
            .map_err(|error| problem(error, &path, &trace))?,
        base_version_id: DocumentVersionId::from_uuid(request.base_version_id),
        target_version_id: DocumentVersionId::from_uuid(request.target_version_id),
        profile,
    };
    let response = match request.projection {
        ProjectionRequest::Diff => {
            let authorized = service
                .compare(&ctx, diff_request)
                .await
                .map_err(|error| problem(error, &path, &trace))?;
            ComparisonResponse::Diff(DiffProjectionDto {
                projection: "diff",
                verdict: verdict(authorized.result.verdict),
                coverage: coverage(authorized.result.coverage),
                result_digest: hex(authorized.result_digest),
                changes: authorized
                    .result
                    .changes
                    .into_iter()
                    .map(change_dto)
                    .collect(),
                unverified_regions: authorized
                    .result
                    .unverified_regions
                    .into_iter()
                    .map(unverified_dto)
                    .collect(),
                ancillary_changes: authorized
                    .result
                    .ancillary_changes
                    .into_iter()
                    .map(ancillary_dto)
                    .collect(),
                audit_event_id: authorized.audit_event_id,
            })
        }
        ProjectionRequest::ComparisonTable => {
            let authorized = service
                .compare(&ctx, diff_request)
                .await
                .map_err(|error| problem(error, &path, &trace))?;
            let rows = project_comparison_table(&authorized.result)
                .into_iter()
                .map(row_dto)
                .collect();
            ComparisonResponse::Table(ComparisonTableProjectionDto {
                projection: "comparisonTable",
                verdict: verdict(authorized.result.verdict),
                coverage: coverage(authorized.result.coverage),
                result_digest: hex(authorized.result_digest),
                rows,
                unverified_regions: authorized
                    .result
                    .unverified_regions
                    .into_iter()
                    .map(unverified_dto)
                    .collect(),
                audit_event_id: authorized.audit_event_id,
            })
        }
        ProjectionRequest::Display => {
            let authorized = service
                .compare_display(
                    &ctx,
                    diff_request,
                    request.page_size,
                    request.cursor,
                    serde_json::json!({"kind": "versionComparison"}),
                )
                .await
                .map_err(|error| problem(error, &path, &trace))?;
            ComparisonResponse::Display(display_projection_dto(authorized))
        }
    };
    if matches!(request.projection, ProjectionRequest::Display)
        && !display_page_within_limit(&response)
    {
        return Err(problem(
            document_application::ApplicationError::InvalidWorkerResult,
            &path,
            &trace,
        ));
    }
    Ok(Json(response))
}

async fn compare_revisions<R, F, E, I>(
    State(state): State<DiffState<R, F, E, I>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    payload: Result<Json<RevisionComparisonRequest>, JsonRejection>,
) -> Result<Json<RevisionComparisonResponseDto>, ApiError>
where
    R: DiffApiRepository,
    F: FileStorage + 'static,
    E: DiffExecutor + 'static,
    I: DiffInspectionEvidence + 'static,
{
    let path = format!("/v1/documents/{document_id}/revision-comparisons");
    let request = payload.map(|Json(request)| request).map_err(|_| {
        problem(
            validation("invalid revision comparison request"),
            &path,
            &trace,
        )
    })?;
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let diff = Arc::new(DocumentDiffService::new(
        state.repository.clone(),
        state.storage.clone(),
        state.executor.clone(),
        state.inspection.clone(),
    ));
    let service = RevisionComparisonService::new(state.repository, diff.clone());
    let mut comparison = service
        .compare(
            &ctx,
            document_id,
            request.base_revision_id,
            request.target_revision_id,
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;

    let mut display: Option<AuthorizedDiffDisplay> = None;
    let mut display_page_size = None;
    if matches!(request.projection, ProjectionRequest::Display) {
        let page_size = request.page_size.unwrap_or(50);
        if !(1..=100).contains(&page_size) {
            return Err(problem(
                validation("display page size must be within 1..=100"),
                &path,
                &trace,
            ));
        }
        display_page_size = Some(page_size);
        match &comparison.content {
            RevisionContentComparison::SameAuthoritativeVersion => {
                if request.cursor.is_some() {
                    return Err(problem(
                        document_application::ApplicationError::CursorStale,
                        &path,
                        &trace,
                    ));
                }
            }
            RevisionContentComparison::DifferentAuthoritativeVersions(authorized) => {
                display = Some(
                    diff.compare_display(
                        &ctx,
                        DiffRequest {
                            document_id,
                            base_version_id: comparison.base.summary.document_version_id,
                            target_version_id: comparison.target.summary.document_version_id,
                            profile: DiffProfileVersion::V0,
                        },
                        request.page_size,
                        request.cursor.clone(),
                        serde_json::json!({
                            "baseRevisionId": comparison.base.summary.revision_id,
                            "targetRevisionId": comparison.target.summary.revision_id,
                            "baseMetadataSnapshotDigest": comparison.metadata.base_snapshot_digest,
                            "targetMetadataSnapshotDigest": comparison.metadata.target_snapshot_digest,
                            "contentResultDigest": authorized.result_digest,
                            "projection": "display"
                        }),
                    )
                    .await
                    .map_err(|error| problem(error, &path, &trace))?,
                );
            }
        }
        comparison = service
            .compare(
                &ctx,
                document_id,
                request.base_revision_id,
                request.target_revision_id,
            )
            .await
            .map_err(|error| problem(error, &path, &trace))?;
    }

    let projection = match request.projection {
        ProjectionRequest::Diff => "diff",
        ProjectionRequest::ComparisonTable => "comparisonTable",
        ProjectionRequest::Display => "display",
    };
    let mut response = RevisionComparisonResponseDto {
        projection,
        base_revision: revision_dto(&comparison.base),
        target_revision: revision_dto(&comparison.target),
        content_comparison_status: "sameAuthoritativeVersion",
        verdict: None,
        coverage: None,
        result_digest: None,
        changes: Vec::new(),
        rows: Vec::new(),
        unverified_regions: Vec::new(),
        ancillary_changes: Vec::new(),
        metadata_comparison_status: comparison.metadata.status.as_str(),
        metadata_changes: comparison
            .metadata
            .changes
            .into_iter()
            .map(metadata_change_dto)
            .collect(),
        base_metadata_snapshot_digest: comparison.metadata.base_snapshot_digest.map(hex),
        target_metadata_snapshot_digest: comparison.metadata.target_snapshot_digest.map(hex),
        audit_event_id: comparison.audit_event_id,
        content_audit_event_id: comparison.content_audit_event_id,
        display_items: Vec::new(),
        page_size: display_page_size,
        next_cursor: None,
        display_audit_event_id: None,
        display_result_audit_event_id: None,
    };
    if let RevisionContentComparison::DifferentAuthoritativeVersions(authorized) =
        comparison.content
    {
        response.content_comparison_status = "differentAuthoritativeVersions";
        response.verdict = Some(verdict(authorized.result.verdict));
        response.coverage = Some(coverage(authorized.result.coverage));
        response.result_digest = Some(hex(authorized.result_digest));
        response.unverified_regions = authorized
            .result
            .unverified_regions
            .iter()
            .cloned()
            .map(unverified_dto)
            .collect();
        match request.projection {
            ProjectionRequest::Diff => {
                response.changes = authorized
                    .result
                    .changes
                    .into_iter()
                    .map(change_dto)
                    .collect();
                response.ancillary_changes = authorized
                    .result
                    .ancillary_changes
                    .into_iter()
                    .map(ancillary_dto)
                    .collect();
            }
            ProjectionRequest::ComparisonTable => {
                response.rows = project_comparison_table(&authorized.result)
                    .into_iter()
                    .map(row_dto)
                    .collect();
            }
            ProjectionRequest::Display => {}
        }
    }
    if let Some(display) = display {
        response.unverified_regions = display
            .unverified_regions
            .into_iter()
            .map(unverified_dto)
            .collect();
        response.display_items = display.items.into_iter().map(display_item_dto).collect();
        response.page_size = Some(display.page_size);
        response.next_cursor = display.next_cursor;
        response.display_audit_event_id = Some(display.display_audit_event_id);
        response.display_result_audit_event_id = Some(display.result_audit_event_id);
    }
    if matches!(request.projection, ProjectionRequest::Display)
        && !display_page_within_limit(&response)
    {
        return Err(problem(
            document_application::ApplicationError::InvalidWorkerResult,
            &path,
            &trace,
        ));
    }
    Ok(Json(response))
}

fn revision_dto(revision: &document_application::DocumentRevisionDetail) -> RevisionDto {
    RevisionDto {
        revision_id: revision.summary.revision_id,
        document_version_id: revision.summary.document_version_id.as_uuid(),
        major: revision.summary.major_no,
        minor: revision.summary.minor_no,
        created_at: revision
            .summary
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    }
}

fn metadata_change_dto(change: MetadataChange) -> MetadataChangeDto {
    MetadataChangeDto {
        path: change.json_pointer,
        base_value: change.base_value,
        target_value: change.target_value,
    }
}

fn change_dto(change: Change) -> ChangeDto {
    ChangeDto {
        operation: change.operation.map(operation),
        relocation: change.relocation.map(relocation),
        facet: change.facet,
        base: change.base.map(source_dto),
        target: change.target.map(source_dto),
        reason_code: change.reason_code,
    }
}

fn unverified_dto(region: UnverifiedRegion) -> UnverifiedRegionDto {
    UnverifiedRegionDto {
        reason: unverified_reason(region.reason),
        base: region.base.map(source_dto),
        target: region.target.map(source_dto),
        navigation_hint: region.navigation_hint,
    }
}

fn ancillary_dto(change: AncillaryChange) -> AncillaryChangeDto {
    AncillaryChangeDto {
        kind: change.kind,
        base_digest: change.base_digest.map(hex),
        target_digest: change.target_digest.map(hex),
    }
}

fn row_dto(row: ComparisonRow) -> ComparisonRowDto {
    ComparisonRowDto {
        state: match row.state {
            ComparisonRowState::Confirmed => "confirmed",
            ComparisonRowState::Unverified => "unverified",
            ComparisonRowState::Ancillary => "ancillary",
        },
        facet: row.facet,
        operation: row.operation.map(operation),
        relocation: row.relocation.map(relocation),
        base: row.base.map(source_dto),
        target: row.target.map(source_dto),
        reason: external_reason(&row.reason),
        navigation_hint: row.navigation_hint,
    }
}

fn display_projection_dto(authorized: AuthorizedDiffDisplay) -> DiffDisplayProjectionDto {
    DiffDisplayProjectionDto {
        projection: "display",
        verdict: verdict(authorized.result.verdict),
        coverage: coverage(authorized.result.coverage),
        result_digest: hex(authorized.result_digest),
        items: authorized.items.into_iter().map(display_item_dto).collect(),
        unverified_regions: authorized
            .unverified_regions
            .into_iter()
            .map(unverified_dto)
            .collect(),
        page_size: authorized.page_size,
        next_cursor: authorized.next_cursor,
        audit_event_id: authorized.display_audit_event_id,
        result_audit_event_id: authorized.result_audit_event_id,
    }
}

fn display_page_within_limit(response: &impl Serialize) -> bool {
    serde_json::to_vec(response).is_ok_and(|bytes| bytes.len() <= MAX_DISPLAY_PAGE_BYTES_V0)
}

fn display_item_dto(item: DiffDisplayItem) -> DiffDisplayItemDto {
    DiffDisplayItemDto {
        change_index: item.change_index,
        operation: item.operation.map(operation),
        relocation: item.relocation.map(relocation),
        facet: item.facet,
        base_locator: item.base_locator.map(locator_dto),
        target_locator: item.target_locator.map(locator_dto),
        base: item.base.map(display_fragment_dto),
        target: item.target.map(display_fragment_dto),
    }
}

fn display_fragment_dto(fragment: DisplayFragment) -> DisplayFragmentDto {
    match fragment {
        DisplayFragment::Text {
            text,
            truncated,
            locator,
        } => DisplayFragmentDto::Text {
            text,
            truncated,
            locator: locator_dto(locator),
        },
        DisplayFragment::Table {
            cells,
            truncated,
            locator,
        } => DisplayFragmentDto::Table {
            cells,
            truncated,
            locator: locator_dto(locator),
        },
        DisplayFragment::Structural { summary, locator } => DisplayFragmentDto::Structural {
            summary,
            locator: locator_dto(locator),
        },
        DisplayFragment::Unavailable { reason } => DisplayFragmentDto::Unavailable {
            reason: match reason {
                DisplayUnavailableReason::NonTextual => "nonTextual",
                DisplayUnavailableReason::Unverified => "unverified",
                DisplayUnavailableReason::ResourceLimit => "resourceLimit",
                DisplayUnavailableReason::Unsupported => "unsupported",
            },
        },
    }
}

fn source_dto(source: SourceEvidence) -> SourceEvidenceDto {
    SourceEvidenceDto {
        document_id: source.document_id.as_uuid(),
        version_id: source.version_id.as_uuid(),
        content_item_id: source.content_item_id,
        representation_id: source.authoritative_representation_id,
        file_id: file_id(source.file_id),
        raw_sha256: hex(source.raw_sha256),
        inspection_profile: source.inspection_profile.as_str(),
        locator: locator_dto(source.locator),
        granularity: match source.granularity {
            LocatorGranularity::Exact => "exact",
            LocatorGranularity::Parent => "parent",
            LocatorGranularity::ContentItem => "contentItem",
        },
        parser_provenance: source.parser_provenance,
    }
}

fn locator_dto(locator: SourceLocator) -> SourceLocatorDto {
    match locator {
        SourceLocator::ContentItem => SourceLocatorDto::ContentItem,
        SourceLocator::TextSpan {
            line,
            byte_start,
            byte_end,
        } => SourceLocatorDto::TextSpan {
            line,
            byte_start,
            byte_end,
        },
        SourceLocator::CsvCell { row, column } => SourceLocatorDto::CsvCell { row, column },
        SourceLocator::HtmlNode { path } => SourceLocatorDto::HtmlNode { path },
        SourceLocator::OfficePath { path } => SourceLocatorDto::OfficePath { path },
        SourceLocator::SheetCell { sheet, cell } => SourceLocatorDto::SheetCell { sheet, cell },
        SourceLocator::VbaModule { module, procedure } => {
            SourceLocatorDto::VbaModule { module, procedure }
        }
        SourceLocator::SlideObject { slide, object } => {
            SourceLocatorDto::SlideObject { slide, object }
        }
        SourceLocator::PdfPage { page, region } => SourceLocatorDto::PdfPage { page, region },
    }
}

fn verdict(value: ContentVerdict) -> &'static str {
    match value {
        ContentVerdict::Same => "same",
        ContentVerdict::Different => "different",
        ContentVerdict::Unknown => "unknown",
    }
}

fn coverage(value: DiffCoverage) -> &'static str {
    match value {
        DiffCoverage::Full => "full",
        DiffCoverage::Partial => "partial",
        DiffCoverage::None => "none",
    }
}

fn operation(value: ChangeOperation) -> &'static str {
    match value {
        ChangeOperation::Added => "added",
        ChangeOperation::Removed => "removed",
        ChangeOperation::Modified => "modified",
    }
}

fn relocation(value: RelocationKind) -> &'static str {
    match value {
        RelocationKind::Moved => "moved",
        RelocationKind::Reordered => "reordered",
        RelocationKind::Renamed => "renamed",
    }
}

fn unverified_reason(value: UnverifiedReason) -> &'static str {
    match value {
        UnverifiedReason::UnsupportedSemanticConstruct => "unsupportedSemanticConstruct",
        UnverifiedReason::CorruptedSource => "corruptedSource",
        UnverifiedReason::MissingInspectionEvidence => "missingInspectionEvidence",
        UnverifiedReason::AmbiguousAlignment => "ambiguousAlignment",
        UnverifiedReason::ResourceLimit => "resourceLimit",
    }
}

fn external_reason(value: &str) -> String {
    match value {
        "unsupported_semantic_construct" => "unsupportedSemanticConstruct".into(),
        "corrupted_source" => "corruptedSource".into(),
        "missing_inspection_evidence" => "missingInspectionEvidence".into(),
        "ambiguous_alignment" => "ambiguousAlignment".into(),
        "resource_limit" => "resourceLimit".into(),
        value => value.to_owned(),
    }
}

fn file_id(value: FileId) -> Uuid {
    value.as_uuid()
}

fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{SourceLocatorDto, display_page_within_limit, locator_dto};
    use document_diff_core::SourceLocator;
    use serde_json::json;

    #[test]
    fn every_source_locator_uses_the_openapi_shape() {
        let cases = [
            (SourceLocator::ContentItem, json!({"kind": "contentItem"})),
            (
                SourceLocator::TextSpan {
                    line: 1,
                    byte_start: 2,
                    byte_end: 3,
                },
                json!({"kind": "textSpan", "line": 1, "byteStart": 2, "byteEnd": 3}),
            ),
            (
                SourceLocator::CsvCell { row: 2, column: 3 },
                json!({"kind": "csvCell", "row": 2, "column": 3}),
            ),
            (
                SourceLocator::HtmlNode {
                    path: "p[1]".into(),
                },
                json!({"kind": "htmlNode", "path": "p[1]"}),
            ),
            (
                SourceLocator::OfficePath {
                    path: "word/document.xml".into(),
                },
                json!({"kind": "officePath", "path": "word/document.xml"}),
            ),
            (
                SourceLocator::SheetCell {
                    sheet: "Sheet1".into(),
                    cell: "A1".into(),
                },
                json!({"kind": "sheetCell", "sheet": "Sheet1", "cell": "A1"}),
            ),
            (
                SourceLocator::VbaModule {
                    module: "Module1".into(),
                    procedure: Some("Run".into()),
                },
                json!({"kind": "vbaModule", "module": "Module1", "procedure": "Run"}),
            ),
            (
                SourceLocator::SlideObject {
                    slide: 2,
                    object: None,
                },
                json!({"kind": "slideObject", "slide": 2, "object": null}),
            ),
            (
                SourceLocator::PdfPage {
                    page: 3,
                    region: Some([1, 2, 30, 40]),
                },
                json!({"kind": "pdfPage", "page": 3, "region": [1, 2, 30, 40]}),
            ),
        ];
        for (source, expected) in cases {
            let dto: SourceLocatorDto = locator_dto(source);
            assert_eq!(serde_json::to_value(dto).unwrap(), expected);
        }
    }

    #[test]
    fn display_page_limit_accepts_exactly_one_mibibyte_and_rejects_one_byte_over() {
        use document_diff_core::MAX_DISPLAY_PAGE_BYTES_V0;

        fn body_of_size(size: usize) -> serde_json::Value {
            let mut body = json!({"page": ""});
            let empty_size = serde_json::to_vec(&body).unwrap().len();
            body["page"] = json!("x".repeat(size - empty_size));
            assert_eq!(serde_json::to_vec(&body).unwrap().len(), size);
            body
        }

        assert!(display_page_within_limit(&body_of_size(
            MAX_DISPLAY_PAGE_BYTES_V0
        )));
        assert!(!display_page_within_limit(&body_of_size(
            MAX_DISPLAY_PAGE_BYTES_V0 + 1
        )));
    }
}
