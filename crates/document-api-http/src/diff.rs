use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Extension, Json, Router};
use document_application::document_diff::{
    AncillaryChange, Change, ComparisonRow, ComparisonRowState, DiffCache, DiffExecutor,
    DiffInspectionEvidence, DiffRequest, DocumentDiffRepository, DocumentDiffService,
    LocatorGranularity, SourceEvidence, UnverifiedRegion, project_comparison_table,
};
use document_application::{FileStorage, VerifiedActorContext, VersionFileAccessRepository};
use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, RelocationKind,
    SourceLocator, UnverifiedReason,
};
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
    DocumentDiffRepository + VersionFileAccessRepository + DiffCache + Send + Sync + 'static
{
}

impl<T> DiffApiRepository for T where
    T: DocumentDiffRepository + VersionFileAccessRepository + DiffCache + Send + Sync + 'static
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
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ComparisonResponse {
    Diff(DiffProjectionDto),
    Table(ComparisonTableProjectionDto),
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
    let authorized = service
        .compare(
            &ctx,
            DiffRequest {
                document_id: document_id_value(&document_id)
                    .map_err(|error| problem(error, &path, &trace))?,
                base_version_id: DocumentVersionId::from_uuid(request.base_version_id),
                target_version_id: DocumentVersionId::from_uuid(request.target_version_id),
                profile,
            },
        )
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    let response = match request.projection {
        ProjectionRequest::Diff => ComparisonResponse::Diff(DiffProjectionDto {
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
        }),
        ProjectionRequest::ComparisonTable => {
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
    };
    Ok(Json(response))
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
    use super::{SourceLocatorDto, locator_dto};
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
}
