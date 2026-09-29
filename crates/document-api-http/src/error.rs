use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use document_application::{ApplicationError, InspectionExecutionError, ManagementErrorCode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ValidationFailed,
    AuthenticationRequired,
    Forbidden,
    DocumentNotFound,
    DocumentVersionNotFound,
    FolderNotFound,
    RevisionConflict,
    OperationConflict,
    CursorStale,
    StaleVersion,
    StaleComparisonInput,
    BusinessRuleRejected,
    ReservedDocument,
    FolderCycle,
    RootProtected,
    IdentityUnavailable,
    PublishQualityRejected,
    UnsupportedMediaType,
    DependencyUnavailable,
    Timeout,
    CommitOutcomeUnknown,
    IntegrityViolation,
    Internal,
}

impl ErrorCode {
    pub const ALL: [Self; 23] = [
        Self::ValidationFailed,
        Self::AuthenticationRequired,
        Self::Forbidden,
        Self::DocumentNotFound,
        Self::DocumentVersionNotFound,
        Self::FolderNotFound,
        Self::RevisionConflict,
        Self::OperationConflict,
        Self::CursorStale,
        Self::StaleVersion,
        Self::StaleComparisonInput,
        Self::BusinessRuleRejected,
        Self::ReservedDocument,
        Self::FolderCycle,
        Self::RootProtected,
        Self::IdentityUnavailable,
        Self::PublishQualityRejected,
        Self::UnsupportedMediaType,
        Self::DependencyUnavailable,
        Self::Timeout,
        Self::CommitOutcomeUnknown,
        Self::IntegrityViolation,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::AuthenticationRequired => "AUTHENTICATION_REQUIRED",
            Self::Forbidden => "FORBIDDEN",
            Self::DocumentNotFound => "DOCUMENT_NOT_FOUND",
            Self::DocumentVersionNotFound => "DOCUMENT_VERSION_NOT_FOUND",
            Self::FolderNotFound => "FOLDER_NOT_FOUND",
            Self::RevisionConflict => "REVISION_CONFLICT",
            Self::OperationConflict => "OPERATION_CONFLICT",
            Self::CursorStale => "CURSOR_STALE",
            Self::StaleVersion => "STALE_VERSION",
            Self::StaleComparisonInput => "STALE_COMPARISON_INPUT",
            Self::BusinessRuleRejected => "BUSINESS_RULE_REJECTED",
            Self::ReservedDocument => "RESERVED_DOCUMENT",
            Self::FolderCycle => "FOLDER_CYCLE",
            Self::RootProtected => "ROOT_PROTECTED",
            Self::IdentityUnavailable => "IDENTITY_UNAVAILABLE",
            Self::PublishQualityRejected => "PUBLISH_QUALITY_REJECTED",
            Self::UnsupportedMediaType => "UNSUPPORTED_MEDIA_TYPE",
            Self::DependencyUnavailable => "DEPENDENCY_UNAVAILABLE",
            Self::Timeout => "TIMEOUT",
            Self::CommitOutcomeUnknown => "COMMIT_OUTCOME_UNKNOWN",
            Self::IntegrityViolation => "INTEGRITY_VIOLATION",
            Self::Internal => "INTERNAL",
        }
    }

    pub const fn status(self) -> u16 {
        match self {
            Self::AuthenticationRequired => 401,
            Self::Forbidden => 403,
            Self::DocumentNotFound | Self::DocumentVersionNotFound | Self::FolderNotFound => 404,
            Self::RevisionConflict
            | Self::OperationConflict
            | Self::CursorStale
            | Self::StaleVersion
            | Self::StaleComparisonInput
            | Self::ReservedDocument
            | Self::FolderCycle
            | Self::RootProtected => 409,
            Self::ValidationFailed | Self::BusinessRuleRejected | Self::PublishQualityRejected => {
                422
            }
            Self::UnsupportedMediaType => 415,
            Self::IdentityUnavailable
            | Self::DependencyUnavailable
            | Self::CommitOutcomeUnknown => 503,
            Self::Timeout => 504,
            Self::IntegrityViolation | Self::Internal => 500,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::ValidationFailed => "Validation failed",
            Self::AuthenticationRequired => "Authentication required",
            Self::Forbidden => "Forbidden",
            Self::DocumentNotFound => "Document not found",
            Self::DocumentVersionNotFound => "Document version not found",
            Self::FolderNotFound => "Folder not found",
            Self::RevisionConflict => "Revision conflict",
            Self::OperationConflict => "Operation conflict",
            Self::CursorStale => "Cursor is stale",
            Self::StaleVersion => "Version is stale",
            Self::StaleComparisonInput => "Comparison input is stale",
            Self::BusinessRuleRejected => "Business rule rejected",
            Self::ReservedDocument => "Document is reserved",
            Self::FolderCycle => "Folder cycle",
            Self::RootProtected => "Root folder is protected",
            Self::IdentityUnavailable => "Identity unavailable",
            Self::PublishQualityRejected => "Publish quality rejected",
            Self::UnsupportedMediaType => "Unsupported media type",
            Self::DependencyUnavailable => "Dependency unavailable",
            Self::Timeout => "Timeout",
            Self::CommitOutcomeUnknown => "Commit outcome unknown",
            Self::IntegrityViolation => "Integrity violation",
            Self::Internal => "Internal error",
        }
    }

    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::IdentityUnavailable | Self::DependencyUnavailable | Self::Timeout
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecovery {
    pub document_id: String,
    pub document_version_id: String,
    pub file_id: String,
    pub recovery_endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub pointer: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProblem {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: &'static str,
    pub status: u16,
    pub detail: &'static str,
    pub instance: String,
    pub code: ErrorCode,
    pub trace_id: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<CreateRecovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_retry: Option<bool>,
}

impl ApiProblem {
    pub fn new(code: ErrorCode, instance: &str, trace_id: &str) -> Self {
        Self {
            problem_type: format!("urn:knowledge-platform:problem:{}", code.as_str()),
            title: code.title(),
            status: code.status(),
            detail: code.title(),
            instance: instance.to_owned(),
            code,
            trace_id: trace_id.to_owned(),
            retryable: code.retryable(),
            errors: Vec::new(),
            recovery: None,
            exact_retry: None,
        }
    }

    pub fn from_application(error: ApplicationError, instance: &str, trace_id: &str) -> Self {
        let code = match &error {
            ApplicationError::Forbidden => ErrorCode::Forbidden,
            ApplicationError::Validation(_) => ErrorCode::ValidationFailed,
            ApplicationError::FolderNotFound => ErrorCode::FolderNotFound,
            ApplicationError::DocumentNotFound => ErrorCode::DocumentNotFound,
            ApplicationError::DocumentVersionNotFound | ApplicationError::FileObjectNotFound => {
                ErrorCode::DocumentVersionNotFound
            }
            ApplicationError::StaleVersion => ErrorCode::StaleVersion,
            ApplicationError::CursorStale => ErrorCode::CursorStale,
            ApplicationError::StaleComparisonInput => ErrorCode::StaleComparisonInput,
            ApplicationError::Conflict => ErrorCode::RevisionConflict,
            ApplicationError::BusinessRule => ErrorCode::BusinessRuleRejected,
            ApplicationError::Management(code) => management_code(*code),
            ApplicationError::PublishQualityRejected(_) => ErrorCode::PublishQualityRejected,
            ApplicationError::RepositoryUnavailable | ApplicationError::StorageUnavailable => {
                ErrorCode::DependencyUnavailable
            }
            ApplicationError::IntegrityViolation
            | ApplicationError::SemanticInspectionDeterminismViolation
            | ApplicationError::InvalidWorkerResult => ErrorCode::IntegrityViolation,
            ApplicationError::InspectionFailed(reason) => match reason {
                InspectionExecutionError::UnsupportedDocumentFormat => {
                    ErrorCode::UnsupportedMediaType
                }
                InspectionExecutionError::InspectionTimeout => ErrorCode::Timeout,
                InspectionExecutionError::ExtractorUnavailable => ErrorCode::DependencyUnavailable,
                InspectionExecutionError::InvalidWorkerResult
                | InspectionExecutionError::ParserDisagreement => ErrorCode::IntegrityViolation,
                _ => ErrorCode::BusinessRuleRejected,
            },
            ApplicationError::CommitOutcomeUnknown { .. }
            | ApplicationError::PublishCommitOutcomeUnknown { .. }
            | ApplicationError::ScheduleCommitOutcomeUnknown { .. }
            | ApplicationError::VersionCommitOutcomeUnknown { .. }
            | ApplicationError::PublicationEndCommitOutcomeUnknown { .. }
            | ApplicationError::ManagementCommitOutcomeUnknown { .. }
            | ApplicationError::ReadStateCommitOutcomeUnknown { .. }
            | ApplicationError::FileAccessAuditCommitOutcomeUnknown { .. } => {
                ErrorCode::CommitOutcomeUnknown
            }
            ApplicationError::StorageWriteFailed
            | ApplicationError::StorageSyncFailed
            | ApplicationError::StorageFinalizeFailed
            | ApplicationError::Internal(_) => ErrorCode::Internal,
        };
        let mut problem = Self::new(code, instance, trace_id);
        match error {
            ApplicationError::CommitOutcomeUnknown {
                document_id,
                document_version_id,
                file_id,
            } => {
                problem.recovery = Some(CreateRecovery {
                    document_id: document_id.as_uuid().to_string(),
                    document_version_id: document_version_id.as_uuid().to_string(),
                    file_id: file_id.as_uuid().to_string(),
                    recovery_endpoint: format!(
                        "/v1/document-creation-outcomes/{}",
                        document_id.as_uuid()
                    ),
                });
            }
            ApplicationError::PublishCommitOutcomeUnknown { .. }
            | ApplicationError::ScheduleCommitOutcomeUnknown { .. }
            | ApplicationError::VersionCommitOutcomeUnknown { .. }
            | ApplicationError::PublicationEndCommitOutcomeUnknown { .. }
            | ApplicationError::ManagementCommitOutcomeUnknown { .. }
            | ApplicationError::ReadStateCommitOutcomeUnknown { .. } => {
                problem.exact_retry = Some(true);
                problem.retryable = true;
            }
            _ => {}
        }
        problem
    }
}

fn management_code(code: ManagementErrorCode) -> ErrorCode {
    match code {
        ManagementErrorCode::InvalidInput => ErrorCode::ValidationFailed,
        ManagementErrorCode::NotFound => ErrorCode::DocumentNotFound,
        ManagementErrorCode::Forbidden => ErrorCode::Forbidden,
        ManagementErrorCode::RevisionConflict => ErrorCode::RevisionConflict,
        ManagementErrorCode::OperationConflict => ErrorCode::OperationConflict,
        ManagementErrorCode::CursorStale => ErrorCode::CursorStale,
        ManagementErrorCode::StaleVersion => ErrorCode::StaleVersion,
        ManagementErrorCode::ReservedDocument => ErrorCode::ReservedDocument,
        ManagementErrorCode::FolderCycle => ErrorCode::FolderCycle,
        ManagementErrorCode::RootProtected => ErrorCode::RootProtected,
        ManagementErrorCode::IdentityUnavailable => ErrorCode::IdentityUnavailable,
        ManagementErrorCode::CommitOutcomeUnknown => ErrorCode::CommitOutcomeUnknown,
        ManagementErrorCode::IntegrityViolation => ErrorCode::IntegrityViolation,
    }
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
