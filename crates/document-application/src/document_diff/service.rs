use std::sync::Arc;

use document_diff_core::{
    AlignmentBudget, AlignmentKind, ChangeOperation, ContentVerdict, DiffCoverage,
    DiffProfileVersion, DisplayFragment, DisplayUnavailableReason, FormatId, ItemAnchor,
    MAX_DISPLAY_FRAGMENT_BYTES_V0, MAX_DISPLAY_PAGE_BYTES_V0, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerDisplayRequest, WorkerProtocolVersion, align_items,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ApplicationError, CursorBinding, CursorPosition, DocumentSort, FileStorage, QueryKind,
    VerifiedActorContext, VersionFileAccessRepository, VersionFileAccessService,
    VersionFileRequest, VersionRequest, decode_cursor, encode_cursor, fingerprint_json,
    principal_fingerprint,
};

use super::ports::{DiffExecutionError, DiffExecutor};
use super::snapshot::normalize_title;
use super::{
    AncillaryChange, AuthorizedComparisonTable, AuthorizedDiffDisplay, Change, DiffCache,
    DiffCacheKey, DiffDisplayItem, DiffInspectionEvidence, DiffPairSnapshot, DiffRequest,
    DiffResult, DocumentDiffRepository, LocatorGranularity, SnapshotItem, SourceEvidence,
    UnverifiedRegion, VersionSnapshot, capture_pair_with_evidence, project_comparison_table,
};

pub struct AuthorizedDiff {
    pub result: DiffResult,
    pub result_digest: [u8; 32],
    pub audit_event_id: Uuid,
    pub pair: super::DiffPairSnapshot,
    pub cache_hit: bool,
}

pub struct DocumentDiffService<R, F, E, I> {
    repository: Arc<R>,
    storage: Arc<F>,
    executor: Arc<E>,
    inspection: Arc<I>,
}

impl<R, F, E, I> DocumentDiffService<R, F, E, I>
where
    R: DocumentDiffRepository + VersionFileAccessRepository + DiffCache,
    F: FileStorage,
    E: DiffExecutor,
    I: DiffInspectionEvidence,
{
    pub fn new(repository: Arc<R>, storage: Arc<F>, executor: Arc<E>, inspection: Arc<I>) -> Self {
        Self {
            repository,
            storage,
            executor,
            inspection,
        }
    }

    pub async fn compare(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<AuthorizedDiff, ApplicationError> {
        actor.ensure_current()?;
        request
            .validate()
            .map_err(|message| ApplicationError::Validation(message.into()))?;
        let pair = capture_pair_with_evidence(
            self.repository.as_ref(),
            self.inspection.as_ref(),
            actor,
            request,
        )
        .await?;
        pair.validate()
            .map_err(|_| ApplicationError::IntegrityViolation)?;
        let key = DiffCacheKey::from_pair(&pair, request.profile, ResourceProfileVersion::V0);
        match self.repository.get(&key).await {
            Ok(Some(cached)) => {
                if cached.validate().is_err()
                    || DiffCacheKey::from_result(&cached) != key
                    || cached.document_id != pair.document_id
                    || cached.base_version_id != pair.base.version_id
                    || cached.target_version_id != pair.target.version_id
                {
                    return Err(ApplicationError::IntegrityViolation);
                }
                let result_digest = cached.canonical_digest();
                let audit_event_id = self
                    .repository
                    .authorize_and_audit_result(actor, &pair, &cached, true, None)
                    .await?;
                return Ok(AuthorizedDiff {
                    result: cached,
                    result_digest,
                    audit_event_id,
                    pair,
                    cache_hit: true,
                });
            }
            Ok(None) | Err(crate::RepositoryError::Unavailable) => {}
            Err(error) => return Err(error.into()),
        }
        let result = self.compose(actor, &pair, request.profile).await?;
        result
            .validate()
            .map_err(|_| ApplicationError::InvalidWorkerResult)?;
        let result_digest = result.canonical_digest();
        let audit_event_id = self
            .repository
            .authorize_and_audit_result(actor, &pair, &result, false, None)
            .await?;
        if cacheable(&result) {
            match self
                .repository
                .put(key, result.clone(), result_digest)
                .await
            {
                Ok(()) | Err(crate::RepositoryError::Unavailable) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(AuthorizedDiff {
            result,
            result_digest,
            audit_event_id,
            pair,
            cache_hit: false,
        })
    }

    pub async fn comparison_table(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<AuthorizedComparisonTable, ApplicationError> {
        let authorized = self.compare(actor, request).await?;
        let rows = project_comparison_table(&authorized.result);
        Ok(AuthorizedComparisonTable {
            rows,
            verdict: authorized.result.verdict,
            coverage: authorized.result.coverage,
            result_digest: authorized.result_digest,
            audit_event_id: authorized.audit_event_id,
        })
    }

    pub async fn compare_display(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
        page_size: Option<u16>,
        cursor: Option<String>,
        display_binding: Value,
    ) -> Result<AuthorizedDiffDisplay, ApplicationError> {
        let page_size = page_size.unwrap_or(50);
        if !(1..=100).contains(&page_size) {
            return Err(ApplicationError::Validation(
                "display page size must be within 1..=100".into(),
            ));
        }
        let authorized = self.compare(actor, request).await?;
        let binding = CursorBinding {
            kind: QueryKind::DiffDisplay,
            sort: DocumentSort::CreatedAtDesc,
            filter_fingerprint: fingerprint_json(&json!({
                "documentId": request.document_id.as_uuid(),
                "baseVersionId": request.base_version_id.as_uuid(),
                "targetVersionId": request.target_version_id.as_uuid(),
                "profile": request.profile.as_str(),
                "resultDigest": authorized.result_digest,
                "displayBinding": display_binding,
            }))?,
            principal_fingerprint: principal_fingerprint(actor)?,
            access_revision: 0,
        };
        let start = cursor
            .as_deref()
            .map(|token| decode_cursor(token, &binding))
            .transpose()?
            .map(|position| {
                if position.document_id != request.document_id.as_uuid() {
                    return Err(ApplicationError::CursorStale);
                }
                position
                    .sort_time_micros
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(ApplicationError::CursorStale)
            })
            .transpose()?
            .unwrap_or(0);
        let change_count = authorized.result.changes.len();
        let total = change_count
            .checked_add(authorized.result.unverified_regions.len())
            .ok_or(ApplicationError::IntegrityViolation)?;
        if start >= total && (total > 0 || cursor.is_some()) {
            return Err(ApplicationError::CursorStale);
        }
        let end = start.saturating_add(page_size as usize).min(total);
        let mut items = Vec::with_capacity(end.saturating_sub(start));
        let mut unverified_regions = Vec::new();
        let mut page_bytes = 2_usize;
        for position in start..end {
            if position >= change_count {
                let region = &authorized.result.unverified_regions[position - change_count];
                let item_bytes = serde_json::to_vec(region)
                    .map_err(|_| ApplicationError::InvalidWorkerResult)?
                    .len();
                if page_bytes.saturating_add(item_bytes).saturating_add(1)
                    > MAX_DISPLAY_PAGE_BYTES_V0 - 4096
                {
                    if items.is_empty() && unverified_regions.is_empty() {
                        return Err(ApplicationError::InvalidWorkerResult);
                    }
                    break;
                }
                page_bytes = page_bytes.saturating_add(item_bytes).saturating_add(1);
                unverified_regions.push(region.clone());
                continue;
            }
            let change = &authorized.result.changes[position];
            let base = match &change.base {
                Some(source) => Some(
                    self.extract_display_source(actor, &authorized, source)
                        .await?,
                ),
                None => None,
            };
            let target = match &change.target {
                Some(source) => Some(
                    self.extract_display_source(actor, &authorized, source)
                        .await?,
                ),
                None => None,
            };
            let mut item = DiffDisplayItem {
                change_index: u32::try_from(position)
                    .map_err(|_| ApplicationError::IntegrityViolation)?,
                operation: change.operation,
                relocation: change.relocation,
                facet: change.facet.clone(),
                base_locator: change.base.as_ref().map(|source| source.locator.clone()),
                target_locator: change.target.as_ref().map(|source| source.locator.clone()),
                base,
                target,
            };
            bound_display_item(&mut item)?;
            let item_bytes = serde_json::to_vec(&item)
                .map_err(|_| ApplicationError::InvalidWorkerResult)?
                .len();
            if page_bytes.saturating_add(item_bytes).saturating_add(1)
                > MAX_DISPLAY_PAGE_BYTES_V0 - 4096
            {
                if items.is_empty() && unverified_regions.is_empty() {
                    return Err(ApplicationError::InvalidWorkerResult);
                }
                break;
            }
            page_bytes = page_bytes.saturating_add(item_bytes).saturating_add(1);
            items.push(item);
        }
        let next_offset = start.saturating_add(items.len() + unverified_regions.len());
        let next_cursor = if next_offset < total {
            Some(encode_cursor(
                &binding,
                &CursorPosition {
                    document_id: request.document_id.as_uuid(),
                    sort_time_micros: Some(
                        i64::try_from(next_offset)
                            .map_err(|_| ApplicationError::IntegrityViolation)?,
                    ),
                    sort_title: None,
                    sort_revision_key: None,
                },
            )?)
        } else {
            None
        };
        let correlation_id = authorized.audit_event_id.to_string();
        let display_audit_event_id = self
            .repository
            .authorize_and_audit_result(
                actor,
                &authorized.pair,
                &authorized.result,
                authorized.cache_hit,
                Some(&correlation_id),
            )
            .await?;
        Ok(AuthorizedDiffDisplay {
            result: authorized.result,
            result_digest: authorized.result_digest,
            result_audit_event_id: authorized.audit_event_id,
            display_audit_event_id,
            items,
            unverified_regions,
            page_size,
            next_cursor,
        })
    }

    async fn extract_display_source(
        &self,
        actor: &VerifiedActorContext,
        authorized: &AuthorizedDiff,
        source: &SourceEvidence,
    ) -> Result<DisplayFragment, ApplicationError> {
        source
            .locator
            .validate()
            .map_err(|_| ApplicationError::IntegrityViolation)?;
        let snapshot = if source.version_id == authorized.pair.base.version_id {
            &authorized.pair.base
        } else if source.version_id == authorized.pair.target.version_id {
            &authorized.pair.target
        } else {
            return Err(ApplicationError::IntegrityViolation);
        };
        if source.document_id != authorized.pair.document_id {
            return Err(ApplicationError::IntegrityViolation);
        }
        let item = snapshot
            .items
            .iter()
            .find(|item| item.content_item_id == source.content_item_id)
            .ok_or(ApplicationError::IntegrityViolation)?;
        if item.file_id != source.file_id
            || item.authoritative_representation_id != source.authoritative_representation_id
            || item.raw_sha256 != source.raw_sha256
        {
            return Err(ApplicationError::IntegrityViolation);
        }
        let Some(format) = item.format else {
            return Ok(DisplayFragment::Unavailable {
                reason: DisplayUnavailableReason::Unsupported,
            });
        };
        if !display_locator_supported(format, &source.locator) {
            return Ok(DisplayFragment::Unavailable {
                reason: match format {
                    FormatId::Docx
                    | FormatId::Xlsx
                    | FormatId::Xlsm
                    | FormatId::Pptx
                    | FormatId::Pdf => DisplayUnavailableReason::NonTextual,
                    _ => DisplayUnavailableReason::Unsupported,
                },
            });
        }
        let correlation_id = authorized.audit_event_id;
        let opened = VersionFileAccessService::new(self.repository.clone(), self.storage.clone())
            .open_version_file(
                actor,
                VersionFileRequest {
                    version: VersionRequest {
                        document_id: snapshot.document_id,
                        document_version_id: snapshot.version_id,
                        purpose: snapshot.reference_purpose,
                    },
                    content_item_id: item.content_item_id,
                    representation_id: item.authoritative_representation_id,
                    correlation_id: Some(correlation_id),
                },
            )
            .await?;
        let response = self
            .executor
            .extract_display(
                WorkerDisplayRequest {
                    protocol_version: WorkerProtocolVersion::V0,
                    format,
                    raw_sha256: item.raw_sha256,
                    size_bytes: item.size_bytes,
                    locator: source.locator.clone(),
                    max_fragment_bytes: MAX_DISPLAY_FRAGMENT_BYTES_V0 as u32,
                },
                opened.content,
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(DiffExecutionError::Timeout | DiffExecutionError::ResourceLimit) => {
                return Ok(DisplayFragment::Unavailable {
                    reason: DisplayUnavailableReason::ResourceLimit,
                });
            }
            Err(DiffExecutionError::RawBindingMismatch) => {
                return Err(ApplicationError::IntegrityViolation);
            }
            Err(DiffExecutionError::InvalidWorkerResult) => {
                return Err(ApplicationError::InvalidWorkerResult);
            }
            Err(DiffExecutionError::Unavailable) => {
                return Err(ApplicationError::Internal(
                    "diff display worker unavailable".into(),
                ));
            }
        };
        response
            .validate_against(&WorkerDisplayRequest {
                protocol_version: WorkerProtocolVersion::V0,
                format,
                raw_sha256: item.raw_sha256,
                size_bytes: item.size_bytes,
                locator: source.locator.clone(),
                max_fragment_bytes: MAX_DISPLAY_FRAGMENT_BYTES_V0 as u32,
            })
            .map_err(|_| ApplicationError::InvalidWorkerResult)?;
        Ok(response.fragment)
    }

    async fn compose(
        &self,
        actor: &VerifiedActorContext,
        pair: &DiffPairSnapshot,
        profile: DiffProfileVersion,
    ) -> Result<DiffResult, ApplicationError> {
        let base: Vec<_> = pair.base.items.iter().map(anchor).collect();
        let target: Vec<_> = pair.target.items.iter().map(anchor).collect();
        let mut budget = AlignmentBudget::new(8_000_000);
        let alignment = align_items(&base, &target, &mut budget);
        let mut changes = Vec::new();
        let mut unverified_regions = Vec::new();
        let mut ancillary_changes = Vec::new();
        let mut verified_any = false;
        for matched in alignment.pairs {
            let old = &pair.base.items[matched.base_index];
            let new = &pair.target.items[matched.target_index];
            let old_source = |locator| source(&pair.base, old, locator, "document-diff-v0");
            let new_source = |locator| source(&pair.target, new, locator, "document-diff-v0");
            let relocation = match matched.kind {
                AlignmentKind::Exact => None,
                AlignmentKind::Reordered => Some(RelocationKind::Reordered),
                AlignmentKind::Relocated => Some(RelocationKind::Moved),
            };
            if let Some(relocation) = relocation {
                changes.push(Change {
                    operation: None,
                    relocation: Some(relocation),
                    facet: "content_item".into(),
                    base: Some(old_source(SourceLocator::ContentItem)),
                    target: Some(new_source(SourceLocator::ContentItem)),
                    reason_code: "manifest_position_changed".into(),
                });
                verified_any = true;
            }
            if old.format != new.format || old.inspection_profile != new.inspection_profile {
                changes.push(Change {
                    operation: Some(ChangeOperation::Modified),
                    relocation: None,
                    facet: "format".into(),
                    base: Some(old_source(SourceLocator::ContentItem)),
                    target: Some(new_source(SourceLocator::ContentItem)),
                    reason_code: "authoritative_format_changed".into(),
                });
                unverified_regions.push(UnverifiedRegion {
                    base: Some(old_source(SourceLocator::ContentItem)),
                    target: Some(new_source(SourceLocator::ContentItem)),
                    reason: UnverifiedReason::UnsupportedSemanticConstruct,
                    navigation_hint: Some("両原本の該当項目を確認してください".into()),
                });
                verified_any = true;
                continue;
            }
            let Some(format) = old.format else {
                missing_evidence(&mut unverified_regions, &pair.base, old, &pair.target, new);
                continue;
            };
            if old.inspection_binding_digest.is_none()
                || new.inspection_binding_digest.is_none()
                || old.semantic_fingerprint.is_none()
                || new.semantic_fingerprint.is_none()
            {
                missing_evidence(&mut unverified_regions, &pair.base, old, &pair.target, new);
                continue;
            }
            let file_access =
                VersionFileAccessService::new(self.repository.clone(), self.storage.clone());
            let old_open = file_access
                .open_version_file(actor, file_request(&pair.base, old))
                .await?;
            let new_open = file_access
                .open_version_file(actor, file_request(&pair.target, new))
                .await?;
            let worker_request = WorkerDiffRequest {
                protocol_version: WorkerProtocolVersion::V0,
                diff_profile_version: profile,
                resource_profile_version: ResourceProfileVersion::V0,
                format,
                base_raw_sha256: old.raw_sha256,
                base_size_bytes: old.size_bytes,
                target_raw_sha256: new.raw_sha256,
                target_size_bytes: new.size_bytes,
            };
            let response = match self
                .executor
                .compare(worker_request.clone(), old_open.content, new_open.content)
                .await
            {
                Ok(response) => response,
                Err(DiffExecutionError::ResourceLimit | DiffExecutionError::Timeout) => {
                    unverified_regions.push(UnverifiedRegion {
                        base: Some(old_source(SourceLocator::ContentItem)),
                        target: Some(new_source(SourceLocator::ContentItem)),
                        reason: UnverifiedReason::ResourceLimit,
                        navigation_hint: Some(
                            "資源上限に達したため両原本を確認してください".into(),
                        ),
                    });
                    continue;
                }
                Err(DiffExecutionError::RawBindingMismatch) => {
                    return Err(ApplicationError::IntegrityViolation);
                }
                Err(DiffExecutionError::InvalidWorkerResult) => {
                    return Err(ApplicationError::InvalidWorkerResult);
                }
                Err(DiffExecutionError::Unavailable) => {
                    return Err(ApplicationError::Internal("diff worker unavailable".into()));
                }
            };
            response
                .validate_against(&worker_request)
                .map_err(|_| ApplicationError::InvalidWorkerResult)?;
            let had_worker_changes = !response.changes.is_empty();
            for change in response.changes {
                changes.push(Change {
                    operation: change.operation,
                    relocation: change.relocation,
                    facet: change.facet,
                    base: change.base.map(|locator| {
                        source(&pair.base, old, locator, &response.parser_provenance)
                    }),
                    target: change.target.map(|locator| {
                        source(&pair.target, new, locator, &response.parser_provenance)
                    }),
                    reason_code: change.reason_code,
                });
            }
            for region in response.unverified_regions {
                unverified_regions.push(UnverifiedRegion {
                    base: region.base.map(|locator| {
                        source(&pair.base, old, locator, &response.parser_provenance)
                    }),
                    target: region.target.map(|locator| {
                        source(&pair.target, new, locator, &response.parser_provenance)
                    }),
                    reason: region.reason,
                    navigation_hint: region.navigation_hint,
                });
            }
            for ancillary in response.ancillary_changes {
                ancillary_changes.push(AncillaryChange {
                    kind: ancillary.kind,
                    base_digest: ancillary.base_digest,
                    target_digest: ancillary.target_digest,
                });
            }
            if response.coverage != DiffCoverage::None {
                verified_any = true;
            }
            if response.coverage == DiffCoverage::Full
                && !had_worker_changes
                && old.semantic_fingerprint != new.semantic_fingerprint
            {
                changes.push(Change {
                    operation: Some(ChangeOperation::Modified),
                    relocation: None,
                    facet: "content_item".into(),
                    base: Some(old_source(SourceLocator::ContentItem)),
                    target: Some(new_source(SourceLocator::ContentItem)),
                    reason_code: "semantic_fingerprint_changed_location_unknown".into(),
                });
                unverified_regions.push(UnverifiedRegion {
                    base: Some(old_source(SourceLocator::ContentItem)),
                    target: Some(new_source(SourceLocator::ContentItem)),
                    reason: UnverifiedReason::UnsupportedSemanticConstruct,
                    navigation_hint: Some("詳細位置が未特定のため両原本を確認してください".into()),
                });
            }
        }
        for index in alignment.unmatched_base {
            let item = &pair.base.items[index];
            changes.push(Change {
                operation: Some(ChangeOperation::Removed),
                relocation: None,
                facet: "content_item".into(),
                base: Some(source(
                    &pair.base,
                    item,
                    SourceLocator::ContentItem,
                    "document-diff-v0",
                )),
                target: None,
                reason_code: "manifest_item_removed".into(),
            });
            verified_any = true;
        }
        for index in alignment.unmatched_target {
            let item = &pair.target.items[index];
            changes.push(Change {
                operation: Some(ChangeOperation::Added),
                relocation: None,
                facet: "content_item".into(),
                base: None,
                target: Some(source(
                    &pair.target,
                    item,
                    SourceLocator::ContentItem,
                    "document-diff-v0",
                )),
                reason_code: "manifest_item_added".into(),
            });
            verified_any = true;
        }
        for cluster in alignment.unresolved {
            let reason = if alignment.exhausted {
                UnverifiedReason::ResourceLimit
            } else {
                UnverifiedReason::AmbiguousAlignment
            };
            for index in cluster.base_indices {
                let item = &pair.base.items[index];
                unverified_regions.push(UnverifiedRegion {
                    base: Some(source(
                        &pair.base,
                        item,
                        SourceLocator::ContentItem,
                        "document-diff-v0",
                    )),
                    target: None,
                    reason,
                    navigation_hint: Some("旧版原本の未対応項目を確認してください".into()),
                });
            }
            for index in cluster.target_indices {
                let item = &pair.target.items[index];
                unverified_regions.push(UnverifiedRegion {
                    base: None,
                    target: Some(source(
                        &pair.target,
                        item,
                        SourceLocator::ContentItem,
                        "document-diff-v0",
                    )),
                    reason,
                    navigation_hint: Some("新版原本の未対応項目を確認してください".into()),
                });
            }
        }
        if normalize_title(&pair.base.title) != normalize_title(&pair.target.title) {
            let old = &pair.base.items[0];
            let new = &pair.target.items[0];
            changes.push(Change {
                operation: Some(ChangeOperation::Modified),
                relocation: None,
                facet: "title".into(),
                base: Some(source(
                    &pair.base,
                    old,
                    SourceLocator::ContentItem,
                    "document-diff-v0",
                )),
                target: Some(source(
                    &pair.target,
                    new,
                    SourceLocator::ContentItem,
                    "document-diff-v0",
                )),
                reason_code: "version_title_changed".into(),
            });
            verified_any = true;
        }
        if pair.base.version_metadata_digest != pair.target.version_metadata_digest {
            ancillary_changes.push(AncillaryChange {
                kind: "version_metadata".into(),
                base_digest: Some(pair.base.version_metadata_digest),
                target_digest: Some(pair.target.version_metadata_digest),
            });
        }
        let (verdict, coverage) = if !changes.is_empty() {
            (
                ContentVerdict::Different,
                if unverified_regions.is_empty() {
                    DiffCoverage::Full
                } else {
                    DiffCoverage::Partial
                },
            )
        } else if unverified_regions.is_empty() {
            (ContentVerdict::Same, DiffCoverage::Full)
        } else {
            (
                ContentVerdict::Unknown,
                if verified_any {
                    DiffCoverage::Partial
                } else {
                    DiffCoverage::None
                },
            )
        };
        Ok(DiffResult {
            document_id: pair.document_id,
            base_version_id: pair.base.version_id,
            target_version_id: pair.target.version_id,
            base_snapshot_digest: pair.base.snapshot_digest(),
            target_snapshot_digest: pair.target.snapshot_digest(),
            profile,
            resource_profile: ResourceProfileVersion::V0,
            verdict,
            coverage,
            changes,
            unverified_regions,
            ancillary_changes,
        })
    }
}

fn cacheable(result: &DiffResult) -> bool {
    result.coverage == DiffCoverage::Full
        || (result.coverage == DiffCoverage::Partial
            && result.unverified_regions.iter().all(|region| {
                matches!(
                    region.reason,
                    UnverifiedReason::UnsupportedSemanticConstruct
                        | UnverifiedReason::AmbiguousAlignment
                        | UnverifiedReason::CorruptedSource
                )
            }))
}

fn anchor(item: &SnapshotItem) -> ItemAnchor {
    ItemAnchor {
        logical_path: item.logical_path.clone(),
        ordinal: item.ordinal,
        format: item.format,
        inspection_profile: item.inspection_profile,
        semantic_fingerprint: item.semantic_fingerprint,
    }
}

fn file_request(version: &VersionSnapshot, item: &SnapshotItem) -> VersionFileRequest {
    VersionFileRequest {
        version: VersionRequest {
            document_id: version.document_id,
            document_version_id: version.version_id,
            purpose: version.reference_purpose,
        },
        content_item_id: item.content_item_id,
        representation_id: item.authoritative_representation_id,
        correlation_id: None,
    }
}

fn source(
    version: &VersionSnapshot,
    item: &SnapshotItem,
    locator: SourceLocator,
    provenance: &str,
) -> SourceEvidence {
    let granularity = match &locator {
        SourceLocator::ContentItem => LocatorGranularity::ContentItem,
        SourceLocator::OfficePath { path } if !path.contains("#p[") && !path.contains("/p[") => {
            LocatorGranularity::Parent
        }
        SourceLocator::SheetCell { cell, .. } if cell.contains(':') => LocatorGranularity::Parent,
        SourceLocator::SlideObject { object: None, .. }
        | SourceLocator::PdfPage { region: None, .. }
        | SourceLocator::VbaModule {
            procedure: None, ..
        } => LocatorGranularity::Parent,
        _ => LocatorGranularity::Exact,
    };
    SourceEvidence {
        document_id: version.document_id,
        version_id: version.version_id,
        content_item_id: item.content_item_id,
        authoritative_representation_id: item.authoritative_representation_id,
        file_id: item.file_id,
        raw_sha256: item.raw_sha256,
        inspection_profile: item.inspection_profile,
        locator,
        granularity,
        parser_provenance: provenance.into(),
    }
}

fn missing_evidence(
    regions: &mut Vec<UnverifiedRegion>,
    old_version: &VersionSnapshot,
    old: &SnapshotItem,
    new_version: &VersionSnapshot,
    new: &SnapshotItem,
) {
    regions.push(UnverifiedRegion {
        base: Some(source(
            old_version,
            old,
            SourceLocator::ContentItem,
            "document-diff-v0",
        )),
        target: Some(source(
            new_version,
            new,
            SourceLocator::ContentItem,
            "document-diff-v0",
        )),
        reason: UnverifiedReason::MissingInspectionEvidence,
        navigation_hint: Some("検査証拠がないため両原本を確認してください".into()),
    });
}

fn display_locator_supported(format: FormatId, locator: &SourceLocator) -> bool {
    matches!(
        (format, locator),
        (FormatId::Txt, SourceLocator::TextSpan { .. })
            | (FormatId::Csv, SourceLocator::CsvCell { .. })
            | (FormatId::Html, SourceLocator::HtmlNode { .. })
    )
}

fn bound_display_item(item: &mut DiffDisplayItem) -> Result<(), ApplicationError> {
    let cap_fragment = |fragment: &mut Option<DisplayFragment>| {
        if fragment.as_ref().is_some_and(|fragment| {
            document_diff_core::serialized_fragment_bytes(fragment) > MAX_DISPLAY_FRAGMENT_BYTES_V0
        }) {
            *fragment = Some(DisplayFragment::Unavailable {
                reason: DisplayUnavailableReason::ResourceLimit,
            });
        }
    };
    cap_fragment(&mut item.base);
    cap_fragment(&mut item.target);
    loop {
        let bytes = serde_json::to_vec(item).map_err(|_| ApplicationError::InvalidWorkerResult)?;
        if bytes.len() <= 32 * 1024 {
            return Ok(());
        }
        let base_size = item
            .base
            .as_ref()
            .map(document_diff_core::serialized_fragment_bytes)
            .unwrap_or(0);
        let target_size = item
            .target
            .as_ref()
            .map(document_diff_core::serialized_fragment_bytes)
            .unwrap_or(0);
        if base_size == 0 && target_size == 0 {
            return Err(ApplicationError::InvalidWorkerResult);
        }
        let replace = if base_size >= target_size {
            &mut item.base
        } else {
            &mut item.target
        };
        *replace = Some(DisplayFragment::Unavailable {
            reason: DisplayUnavailableReason::ResourceLimit,
        });
    }
}

#[cfg(test)]
mod display_bound_tests {
    use super::*;

    const ITEM_LIMIT: usize = 32 * 1024;

    fn empty_item() -> DiffDisplayItem {
        let locator = SourceLocator::TextSpan {
            line: 1,
            byte_start: 0,
            byte_end: 1,
        };
        let base = DisplayFragment::Text {
            text: String::new(),
            truncated: false,
            locator: locator.clone(),
        };
        let target = DisplayFragment::Text {
            text: String::new(),
            truncated: false,
            locator: locator.clone(),
        };
        DiffDisplayItem {
            change_index: 0,
            operation: Some(ChangeOperation::Modified),
            relocation: None,
            facet: "visible_text".into(),
            base_locator: Some(locator.clone()),
            target_locator: Some(locator),
            base: Some(base),
            target: Some(target),
        }
    }

    fn item_with_size(size: usize) -> DiffDisplayItem {
        let mut item = empty_item();
        let empty_size = serde_json::to_vec(&item).unwrap().len();
        let content_size = size - empty_size;
        let base_size = content_size / 2;
        let target_size = content_size - base_size;
        if let Some(DisplayFragment::Text { text, .. }) = item.base.as_mut() {
            *text = "x".repeat(base_size);
        }
        if let Some(DisplayFragment::Text { text, .. }) = item.target.as_mut() {
            *text = "x".repeat(target_size);
        }
        assert_eq!(serde_json::to_vec(&item).unwrap().len(), size);
        assert!(item.base.as_ref().is_some_and(|fragment| {
            document_diff_core::serialized_fragment_bytes(fragment) <= MAX_DISPLAY_FRAGMENT_BYTES_V0
        }));
        assert!(item.target.as_ref().is_some_and(|fragment| {
            document_diff_core::serialized_fragment_bytes(fragment) <= MAX_DISPLAY_FRAGMENT_BYTES_V0
        }));
        item
    }

    #[test]
    fn display_item_accepts_exact_32_kib_and_bounds_one_byte_over() {
        let mut exact = item_with_size(ITEM_LIMIT);
        bound_display_item(&mut exact).unwrap();
        assert_eq!(serde_json::to_vec(&exact).unwrap().len(), ITEM_LIMIT);

        let mut over = item_with_size(ITEM_LIMIT + 1);
        bound_display_item(&mut over).unwrap();
        assert!(serde_json::to_vec(&over).unwrap().len() <= ITEM_LIMIT);
        assert!(
            matches!(
                over.base,
                Some(DisplayFragment::Unavailable {
                    reason: DisplayUnavailableReason::ResourceLimit
                })
            ) || matches!(
                over.target,
                Some(DisplayFragment::Unavailable {
                    reason: DisplayUnavailableReason::ResourceLimit
                })
            )
        );
    }
}
