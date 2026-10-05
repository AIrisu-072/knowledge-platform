//! P1-B02: same-key body bundle staging, seal validation and publication state.
//!
//! A bundle moves `Staging → Validated → Published` (or is discarded). Once
//! validated its artifacts are immutable. Publication re-verifies the validated
//! receipt under the runtime lock and only then moves the projection pointer.

use std::collections::BTreeMap;

use document_domain::DocumentId;
use search_application::SearchError;
use search_core::id::ResourceId;
use search_core::knowledge_unit::{KnowledgeUnit, UnitId, text_sha256};
use search_core::projection::{CompiledResourceProjection, ProjectionGenerationKey};
use search_tantivy::IndexedUnitDoc;
use sha2::{Digest, Sha256};

use crate::body_manifest::{
    ArtifactReceipt, BodyCoverageArtifact, BodyCoverageItem, BodyUnitManifest,
    GenerationBundleReceipt, compute_bundle_receipt,
};

fn failed(reason: &str) -> SearchError {
    SearchError::OperationFailed(format!("body bundle: {reason}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum BundleState {
    #[default]
    Staging,
    Validated(GenerationBundleReceipt),
    Published(GenerationBundleReceipt),
}

#[derive(Debug, Default)]
struct StagedBundle {
    unit_manifest: Option<BodyUnitManifest>,
    coverage: Option<BodyCoverageArtifact>,
    state: BundleState,
}

/// A published body bundle: its receipt, Unit manifest and coverage artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedBody {
    pub receipt: GenerationBundleReceipt,
    pub manifest: BodyUnitManifest,
    pub coverage: BodyCoverageArtifact,
}

/// Runtime-owned bundle state keyed by generation. Lexical and Graph receipts
/// are recorded for every generation the runtime builds; only a staged Unit
/// manifest turns a generation into a body bundle.
#[derive(Debug, Default)]
pub(crate) struct BundleRegistry {
    staged: BTreeMap<ProjectionGenerationKey, StagedBundle>,
    lexical: BTreeMap<ProjectionGenerationKey, ArtifactReceipt>,
    graph: BTreeMap<ProjectionGenerationKey, ArtifactReceipt>,
}

impl BundleRegistry {
    fn staging(&mut self, key: ProjectionGenerationKey) -> Result<&mut StagedBundle, SearchError> {
        let bundle = self.staged.entry(key).or_default();
        if bundle.state != BundleState::Staging {
            return Err(failed("artifacts are immutable after validation"));
        }
        Ok(bundle)
    }

    fn unsealed(&self, key: ProjectionGenerationKey) -> Result<(), SearchError> {
        match self.staged.get(&key).map(|bundle| &bundle.state) {
            None | Some(BundleState::Staging) => Ok(()),
            Some(_) => Err(failed("artifacts are immutable after validation")),
        }
    }

    pub(crate) fn record_lexical(&mut self, receipt: ArtifactReceipt) -> Result<(), SearchError> {
        self.unsealed(receipt.key)?;
        if self.lexical.insert(receipt.key, receipt).is_some() {
            return Err(failed("lexical artifact staged twice"));
        }
        Ok(())
    }

    pub(crate) fn record_graph(&mut self, receipt: ArtifactReceipt) -> Result<(), SearchError> {
        self.unsealed(receipt.key)?;
        if self.graph.insert(receipt.key, receipt).is_some() {
            return Err(failed("graph artifact staged twice"));
        }
        Ok(())
    }

    pub(crate) fn forget_lexical(&mut self, key: ProjectionGenerationKey) {
        if self.unsealed(key).is_ok() {
            self.lexical.remove(&key);
        }
    }

    pub(crate) fn forget_graph(&mut self, key: ProjectionGenerationKey) {
        if self.unsealed(key).is_ok() {
            self.graph.remove(&key);
        }
    }

    pub(crate) fn stage_manifest(&mut self, manifest: BodyUnitManifest) -> Result<(), SearchError> {
        let bundle = self.staging(manifest.key)?;
        if bundle.unit_manifest.replace(manifest).is_some() {
            return Err(failed("Unit manifest staged twice"));
        }
        Ok(())
    }

    pub(crate) fn stage_coverage(
        &mut self,
        artifact: BodyCoverageArtifact,
    ) -> Result<(), SearchError> {
        let bundle = self.staging(artifact.key)?;
        if bundle.coverage.replace(artifact).is_some() {
            return Err(failed("coverage artifact staged twice"));
        }
        Ok(())
    }

    /// Recompute every receipt from staged data and seal the actual lexical Unit
    /// documents against the manifest.
    pub(crate) fn validate(
        &mut self,
        key: ProjectionGenerationKey,
        projection_digest: &str,
        documents: Vec<IndexedUnitDoc>,
    ) -> Result<GenerationBundleReceipt, SearchError> {
        let lexical = self.lexical.get(&key).copied();
        let graph = self.graph.get(&key).copied();
        let bundle = self.staging(key)?;
        let receipt = recompute(key, bundle, lexical, graph, projection_digest)?;
        let manifest = bundle
            .unit_manifest
            .as_ref()
            .ok_or_else(|| failed("Unit manifest missing"))?;
        seal_lexical(manifest, &documents)?;
        bundle.state = BundleState::Validated(receipt.clone());
        Ok(receipt)
    }

    /// `Ok(None)` for a projection-only generation without a bundle. A staged but
    /// unvalidated bundle, or one whose artifacts no longer match, cannot publish.
    pub(crate) fn publishable(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<Option<GenerationBundleReceipt>, SearchError> {
        let Some(bundle) = self.staged.get(&key) else {
            return Ok(None);
        };
        let BundleState::Validated(validated) = &bundle.state else {
            return Err(failed("bundle is not validated"));
        };
        let digest = format!("sha256:{}", hex(&validated.projection_digest));
        let again = recompute(
            key,
            bundle,
            self.lexical.get(&key).copied(),
            self.graph.get(&key).copied(),
            &digest,
        )?;
        if &again != validated {
            return Err(failed("validated bundle changed before publication"));
        }
        Ok(Some(validated.clone()))
    }

    pub(crate) fn mark_published(&mut self, key: ProjectionGenerationKey) {
        if let Some(bundle) = self.staged.get_mut(&key)
            && let BundleState::Validated(receipt) = &bundle.state
        {
            bundle.state = BundleState::Published(receipt.clone());
        }
    }

    pub(crate) fn published(
        &self,
        key: ProjectionGenerationKey,
    ) -> Option<GenerationBundleReceipt> {
        match self.staged.get(&key).map(|bundle| &bundle.state) {
            Some(BundleState::Published(receipt)) => Some(receipt.clone()),
            _ => None,
        }
    }

    /// The immutable artifacts of a published bundle, for Source-owned readers.
    pub(crate) fn published_body(&self, key: ProjectionGenerationKey) -> Option<PublishedBody> {
        let bundle = self.staged.get(&key)?;
        let BundleState::Published(receipt) = &bundle.state else {
            return None;
        };
        Some(PublishedBody {
            receipt: receipt.clone(),
            manifest: bundle.unit_manifest.clone()?,
            coverage: bundle.coverage.clone()?,
        })
    }

    /// Remove an unpublished bundle. A published bundle stays immutable for
    /// in-flight pins and is never discarded through this path.
    pub(crate) fn discard(&mut self, key: ProjectionGenerationKey) -> Result<bool, SearchError> {
        match self.staged.get(&key).map(|bundle| &bundle.state) {
            Some(BundleState::Published(_)) => Err(failed("published bundle cannot be discarded")),
            None => Ok(false),
            Some(_) => {
                self.lexical.remove(&key);
                self.graph.remove(&key);
                Ok(self.staged.remove(&key).is_some())
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn recompute(
    key: ProjectionGenerationKey,
    bundle: &StagedBundle,
    lexical: Option<ArtifactReceipt>,
    graph: Option<ArtifactReceipt>,
    projection_digest: &str,
) -> Result<GenerationBundleReceipt, SearchError> {
    let manifest = bundle
        .unit_manifest
        .as_ref()
        .ok_or_else(|| failed("Unit manifest missing"))?;
    let coverage = bundle
        .coverage
        .as_ref()
        .ok_or_else(|| failed("coverage artifact missing"))?;
    if coverage != &derive_coverage(manifest)? {
        return Err(failed("coverage is not derived from the Unit manifest"));
    }
    let lexical = lexical.ok_or_else(|| failed("lexical artifact missing"))?;
    let graph = graph.ok_or_else(|| failed("graph artifact missing"))?;
    compute_bundle_receipt(
        key,
        &manifest.source_snapshot,
        projection_digest,
        manifest,
        coverage,
        lexical,
        graph,
    )
    .map_err(|error| failed(&error.to_string()))
}

/// The coverage artifact is derived only from the manifest's own item set.
pub(crate) fn derive_coverage(
    manifest: &BodyUnitManifest,
) -> Result<BodyCoverageArtifact, SearchError> {
    let items = manifest
        .entries
        .iter()
        .map(|entry| {
            Ok(BodyCoverageItem {
                version: entry.version.clone(),
                part: entry.part.clone(),
                operation: entry.operation,
                coverage: entry.coverage.clone(),
                raw: entry.raw.clone(),
                unit_count: u32::try_from(entry.units.len()).map_err(|_| failed("unit count"))?,
            })
        })
        .collect::<Result<Vec<_>, SearchError>>()?;
    Ok(BodyCoverageArtifact {
        key: manifest.key,
        items,
    })
}

/// Bijective seal: every Supported/Partial Unit has exactly one searchable
/// document with the same identity, binding and text, and no other document
/// exists. Unsupported and failed items therefore contribute zero documents.
pub fn seal_lexical(
    manifest: &BodyUnitManifest,
    documents: &[IndexedUnitDoc],
) -> Result<(), SearchError> {
    let mut units: BTreeMap<UnitId, &KnowledgeUnit> = BTreeMap::new();
    for unit in manifest.entries.iter().flat_map(|entry| &entry.units) {
        if units.insert(unit.unit_id, unit).is_some() {
            return Err(failed("duplicate Unit in manifest"));
        }
    }
    if documents.len() != units.len() {
        return Err(failed("lexical seal: document count differs from Units"));
    }
    let mut seen = BTreeMap::new();
    for document in documents {
        if document.generation != manifest.key {
            return Err(failed("lexical seal: document generation"));
        }
        let unit = units
            .get(&document.unit_id)
            .ok_or_else(|| failed("lexical seal: unknown document"))?;
        if seen.insert(document.unit_id, ()).is_some() {
            return Err(failed("lexical seal: duplicate document"));
        }
        let matches = document.parent_resource == unit.version.resource_id
            && document.version == unit.version
            && document.part == unit.part
            && document.authoritative_representation_ref
                == unit.provenance.authoritative_representation_ref
            && document.raw == unit.provenance.raw
            && document.ordinal == unit.ordinal
            && document.kind == unit.kind
            && document.locator == unit.locator
            && document.profile == unit.provenance.profile
            && document.text == unit.text
            && document.text_sha256 == unit.text_sha256
            && text_sha256(&document.text) == unit.text_sha256;
        if !matches {
            return Err(failed("lexical seal: document differs from Unit"));
        }
    }
    Ok(())
}

/// Canonical Graph staged input: typed n-ary relations by RelationId plus the
/// Document owner mapping of auxiliary Resources.
pub fn graph_receipt(
    key: ProjectionGenerationKey,
    projections: &[CompiledResourceProjection],
    ownership: &[(ResourceId, DocumentId)],
) -> Result<ArtifactReceipt, SearchError> {
    let mut relations: Vec<_> = projections
        .iter()
        .flat_map(|projection| &projection.relations)
        .collect();
    relations.sort_by_key(|relation| relation.relation_id);
    relations.dedup_by_key(|relation| relation.relation_id);
    let mut owners: Vec<_> = ownership
        .iter()
        .map(|(resource, document)| (resource.as_uuid(), document.as_uuid()))
        .collect();
    owners.sort();
    owners.dedup();
    let mut hasher = Sha256::new();
    hasher.update(b"document-graph-input:v1\0");
    hasher.update((relations.len() as u32).to_be_bytes());
    for relation in &relations {
        let bytes = serde_json::to_vec(relation).map_err(|_| failed("graph relation encoding"))?;
        hasher.update((bytes.len() as u32).to_be_bytes());
        hasher.update(&bytes);
    }
    hasher.update((owners.len() as u32).to_be_bytes());
    for (resource, document) in &owners {
        hasher.update(resource.as_bytes());
        hasher.update(document.as_bytes());
    }
    Ok(ArtifactReceipt {
        key,
        digest: hasher.finalize().into(),
        count: relations.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
    use search_core::knowledge_unit::{
        ContentPartRef, ExtractionProfileId, FormatId, NativeLocator, RawBinding,
        ResourceVersionRef, UnitKind, UnitProvenance,
    };
    use search_extraction_core::{BodyCoverage, ItemOperationState};
    use uuid::Uuid;

    use super::*;
    use crate::body_manifest::BodyItemEntry;

    fn key() -> ProjectionGenerationKey {
        ProjectionGenerationKey {
            source_id: SourceId::from_uuid(Uuid::from_u128(1)),
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(2)),
        }
    }

    fn profile() -> ExtractionProfileId {
        ExtractionProfileId::parse(&format!("sha256:{}", "a".repeat(64))).unwrap()
    }

    fn unit(ordinal: u32, text: &str) -> KnowledgeUnit {
        let version = ResourceVersionRef {
            source_id: key().source_id,
            resource_id: ResourceId::from_uuid(Uuid::from_u128(10)),
            source_native_version: "v1".into(),
        };
        let part = ContentPartRef {
            source_native_part_id: "part".into(),
            logical_path: "doc/primary".into(),
            ordinal: 0,
        };
        let locator = NativeLocator::Text {
            line_start: ordinal,
            line_end: ordinal + 1,
        };
        KnowledgeUnit {
            unit_id: UnitId::derive(&version, &part, &profile(), &locator, ordinal).unwrap(),
            version,
            part,
            parent_unit_id: None,
            ordinal,
            kind: UnitKind::PlainText,
            text: text.into(),
            locator,
            text_sha256: text_sha256(text),
            provenance: UnitProvenance {
                source_snapshot: "snapshot".into(),
                authoritative_representation_ref: "representation".into(),
                raw: RawBinding {
                    sha256: [1; 32],
                    size_bytes: 8,
                    media_type: "text/plain".into(),
                },
                detected_format: FormatId::Text,
                archive_inner_format: None,
                profile: profile(),
                parser_build_id: "build".into(),
            },
        }
    }

    fn manifest(units: Vec<KnowledgeUnit>) -> BodyUnitManifest {
        let first = units[0].clone();
        BodyUnitManifest {
            key: key(),
            source_snapshot: "snapshot".into(),
            entries: vec![BodyItemEntry {
                version: first.version.clone(),
                part: first.part.clone(),
                authoritative_representation_ref: "representation".into(),
                raw: first.provenance.raw.clone(),
                detected_format: Some(FormatId::Text),
                profile: Some(profile()),
                parser_build_id: "build".into(),
                archive_plan: None,
                operation: ItemOperationState::Completed,
                coverage: Some(BodyCoverage::Supported),
                units,
            }],
        }
    }

    fn document(unit: &KnowledgeUnit) -> IndexedUnitDoc {
        IndexedUnitDoc {
            generation: key(),
            parent_resource: unit.version.resource_id,
            version: unit.version.clone(),
            part: unit.part.clone(),
            authoritative_representation_ref: unit
                .provenance
                .authoritative_representation_ref
                .clone(),
            raw: unit.provenance.raw.clone(),
            unit_id: unit.unit_id,
            ordinal: unit.ordinal,
            kind: unit.kind,
            locator: unit.locator.clone(),
            profile: unit.provenance.profile.clone(),
            text_sha256: unit.text_sha256,
            text: unit.text.clone(),
        }
    }

    fn receipt(byte: u8) -> ArtifactReceipt {
        ArtifactReceipt {
            key: key(),
            digest: [byte; 32],
            count: 1,
        }
    }

    const PROJECTION: &str =
        "sha256:0202020202020202020202020202020202020202020202020202020202020202";

    #[test]
    fn lexical_seal_requires_a_bijection_with_matching_text() {
        let units = vec![unit(0, "東京"), unit(1, "同文。")];
        let manifest = manifest(units.clone());
        let documents: Vec<_> = units.iter().map(document).collect();
        seal_lexical(&manifest, &documents).unwrap();

        let missing = &documents[..1];
        let duplicated = [documents.clone(), vec![documents[0].clone()]].concat();
        let mut swapped = documents.clone();
        swapped[0].text = "大阪".into();
        let mut foreign = documents.clone();
        foreign[1].generation.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(3));
        for broken in [missing.to_vec(), duplicated, swapped, foreign] {
            assert!(seal_lexical(&manifest, &broken).is_err());
        }
    }

    #[test]
    fn bundle_is_immutable_after_validation_and_publishes_only_when_validated() {
        let units = vec![unit(0, "東京")];
        let manifest = manifest(units.clone());
        let coverage = derive_coverage(&manifest).unwrap();
        let mut registry = BundleRegistry::default();
        // A projection-only generation has no bundle and publishes as before.
        assert_eq!(registry.publishable(key()).unwrap(), None);
        registry.record_lexical(receipt(5)).unwrap();
        registry.record_graph(receipt(6)).unwrap();
        registry.stage_manifest(manifest.clone()).unwrap();
        assert!(registry.stage_manifest(manifest.clone()).is_err());
        assert!(registry.publishable(key()).is_err(), "unvalidated bundle");
        // Coverage that is not derived from the manifest is rejected.
        let mut wrong = coverage.clone();
        wrong.items[0].unit_count = 9;
        registry.stage_coverage(wrong).unwrap();
        assert!(
            registry
                .validate(key(), PROJECTION, units.iter().map(document).collect())
                .is_err()
        );

        let mut registry = BundleRegistry::default();
        registry.record_lexical(receipt(5)).unwrap();
        registry.record_graph(receipt(6)).unwrap();
        registry.stage_manifest(manifest.clone()).unwrap();
        registry.stage_coverage(coverage.clone()).unwrap();
        let validated = registry
            .validate(key(), PROJECTION, units.iter().map(document).collect())
            .unwrap();
        assert!(registry.stage_coverage(coverage).is_err());
        assert!(registry.record_lexical(receipt(7)).is_err());
        assert_eq!(
            registry.publishable(key()).unwrap(),
            Some(validated.clone())
        );
        registry.mark_published(key());
        assert_eq!(registry.published(key()), Some(validated));
        assert!(
            registry.discard(key()).is_err(),
            "published bundles stay for pins"
        );
    }
}
