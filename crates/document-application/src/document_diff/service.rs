use std::sync::Arc;

use document_diff_core::{
    AlignmentBudget, AlignmentKind, ChangeOperation, ContentVerdict, DiffCoverage,
    DiffProfileVersion, ItemAnchor, RelocationKind, ResourceProfileVersion, SourceLocator,
    UnverifiedReason, WorkerDiffRequest, WorkerProtocolVersion, align_items,
};
use uuid::Uuid;

use crate::{
    ApplicationError, FileStorage, VerifiedActorContext, VersionFileAccessRepository,
    VersionFileAccessService, VersionFileRequest, VersionRequest,
};

use super::ports::{DiffExecutionError, DiffExecutor};
use super::snapshot::normalize_title;
use super::{
    AncillaryChange, AuthorizedComparisonTable, Change, DiffCache, DiffCacheKey,
    DiffInspectionEvidence, DiffPairSnapshot, DiffRequest, DiffResult, DocumentDiffRepository,
    LocatorGranularity, SnapshotItem, SourceEvidence, UnverifiedRegion, VersionSnapshot,
    capture_pair_with_evidence, project_comparison_table,
};

pub struct AuthorizedDiff {
    pub result: DiffResult,
    pub result_digest: [u8; 32],
    pub audit_event_id: Uuid,
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
