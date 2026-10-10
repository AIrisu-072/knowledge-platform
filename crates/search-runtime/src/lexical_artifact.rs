//! P7-05: file-backed lexical artifacts.
//!
//! A generation directory is built under the trusted staging root, every file
//! and directory is fsynced, it is renamed once to the immutable final path
//! derived only from the trusted root and the key, and the parent is fsynced.
//! The seal reopens the real index: the logical P1 digest is recomputed from
//! what is stored, every searchable Unit document is matched one-to-one with
//! the Unit manifest, and the file tree digest covers every byte. Files and
//! database rows cannot commit atomically, so READY, CAS, pin and return all
//! reopen and revalidate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use search_application::search_core::id::SourceId;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::source::DiscoverableSource;
use search_source_document::{
    ArtifactReceipt, BodyUnitManifest, seal_lexical_hashes, unit_manifest_receipt_from_segments,
    unit_seal_entries,
};
use search_tantivy::{PersistedLexical, TantivyLexicalIndex, UnitSealEntry};

use crate::payload::UnitManifestSummaryV1;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

pub const LEXICAL_INDEX_FORMAT_VERSION: &str = "tantivy-dir-v1";

/// What a reopened, sealed lexical generation directory contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalSealV1 {
    pub key: ProjectionGenerationKey,
    pub schema_version: String,
    pub analyzer_version: String,
    pub logical_digest: [u8; 32],
    pub logical_count: u64,
    pub searchable_doc_count: u64,
    pub unit_seal_digest: [u8; 32],
    pub unit_seal_count: u64,
    pub tree_digest: [u8; 32],
    pub index_relpath: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexicalArtifactError {
    /// Staging or final path is not the trusted path for this key.
    Path,
    /// The final directory already exists or the rename/fsync failed.
    Io,
    /// The directory does not reopen as this generation's index.
    Index,
    /// Searchable Unit documents differ from the Unit manifest.
    Seal,
    /// The reopened logical digest differs from the expected P1 receipt.
    Receipt,
    /// The saved artifact row differs from what is on disk now.
    Drift,
    /// The database refused the row: no BUILDING parent with a live guard.
    Rejected,
    StoreUnknown,
}

impl From<sqlx::Error> for LexicalArtifactError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23514" | "23503" | "23505" | "42501") => Self::Rejected,
            _ => Self::StoreUnknown,
        }
    }
}

fn sha256_text(digest: &[u8; 32]) -> String {
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(unix)]
fn sync_path(path: &Path) -> Result<(), LexicalArtifactError> {
    std::fs::File::open(path)
        .and_then(|handle| handle.sync_all())
        .map_err(|_| LexicalArtifactError::Io)
}

#[cfg(not(unix))]
fn sync_path(path: &Path) -> Result<(), LexicalArtifactError> {
    if path.is_file() {
        std::fs::File::open(path)
            .and_then(|handle| handle.sync_all())
            .map_err(|_| LexicalArtifactError::Io)?;
    }
    Ok(())
}

/// Index lock files are transient and excluded from the tree digest.
fn is_lock(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "lock")
}

/// Every regular file below `dir`, as sorted `/`-separated relative paths.
fn tree_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, LexicalArtifactError> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).map_err(|_| LexicalArtifactError::Io)? {
            let entry = entry.map_err(|_| LexicalArtifactError::Io)?;
            let kind = entry.file_type().map_err(|_| LexicalArtifactError::Io)?;
            let path = entry.path();
            if kind.is_symlink() {
                return Err(LexicalArtifactError::Index);
            } else if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && !is_lock(&path) {
                let relative = path
                    .strip_prefix(dir)
                    .map_err(|_| LexicalArtifactError::Path)?
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((relative, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// File digests computed in this process, by file identity (device, inode,
/// size, modification time). A file hard-linked into a later generation keeps
/// its identity (linking changes only the change time), so its bytes are
/// hashed once per process.
type FileDigestCache = Mutex<HashMap<[u64; 5], (u64, [u8; 32])>>;

fn file_digest_cache() -> &'static FileDigestCache {
    static CACHE: OnceLock<FileDigestCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Cached file digests kept per process before the cache is emptied.
const CACHED_FILE_DIGESTS: usize = 200_000;

#[cfg(unix)]
fn file_identity(path: &Path) -> Option<[u64; 5]> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some([
        meta.dev(),
        meta.ino(),
        meta.size(),
        u64::try_from(meta.mtime()).ok()?,
        u64::try_from(meta.mtime_nsec()).ok()?,
    ])
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Option<[u64; 5]> {
    None
}

/// Length and SHA-256 of one file, from the cache when its identity is known.
fn file_digest(path: &Path) -> Result<(u64, [u8; 32]), LexicalArtifactError> {
    let identity = file_identity(path);
    if let Some(identity) = identity
        && let Some(hit) = file_digest_cache()
            .lock()
            .map_err(|_| LexicalArtifactError::Io)?
            .get(&identity)
    {
        return Ok(*hit);
    }
    let bytes = std::fs::read(path).map_err(|_| LexicalArtifactError::Io)?;
    let digest = (bytes.len() as u64, Sha256::digest(&bytes).into());
    if let Some(identity) = identity {
        let mut cache = file_digest_cache()
            .lock()
            .map_err(|_| LexicalArtifactError::Io)?;
        if cache.len() >= CACHED_FILE_DIGESTS {
            cache.clear();
        }
        cache.insert(identity, digest);
    }
    Ok(digest)
}

fn tree_digest(dir: &Path) -> Result<[u8; 32], LexicalArtifactError> {
    let mut hasher = Sha256::new();
    hasher.update(b"lexical-tree:v1\0");
    let files = tree_files(dir)?;
    hasher.update((files.len() as u64).to_be_bytes());
    for (relative, path) in files {
        let (length, digest) = file_digest(&path)?;
        frame(&mut hasher, relative.as_bytes());
        hasher.update(length.to_be_bytes());
        hasher.update(digest);
    }
    Ok(hasher.finalize().into())
}

type SealCache = Mutex<HashMap<(ProjectionGenerationKey, [u8; 32], [u8; 32]), LexicalSealV1>>;

/// Seals computed in this process, by generation key, file tree digest and
/// Unit manifest digest.
fn sealed_cache() -> &'static SealCache {
    static CACHE: OnceLock<SealCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Seals kept per process before the cache is emptied.
const CACHED_SEALS: usize = 64;

/// The directory half of a lexical seal (see
/// [`LexicalArtifactStore::inspect_final`]).
pub struct InspectedLexical {
    key: ProjectionGenerationKey,
    tree: [u8; 32],
    /// `None` when this process sealed the same files before.
    read: Option<PersistedRead>,
}

struct PersistedRead {
    persisted: PersistedLexical,
    unit_seal: Result<[u8; 32], LexicalArtifactError>,
}

/// `lexical-unit-seal:v2`: every searchable Unit document's ID and the digest
/// of its stored fields and text, in Unit ID order.
fn unit_seal(units: &[UnitSealEntry]) -> Result<[u8; 32], LexicalArtifactError> {
    let mut ordered: Vec<&UnitSealEntry> = units.iter().collect();
    ordered.sort_unstable_by_key(|entry| entry.unit_id);
    let mut hasher = Sha256::new();
    hasher.update(b"lexical-unit-seal:v2\0");
    hasher.update((ordered.len() as u64).to_be_bytes());
    for entry in ordered {
        frame(&mut hasher, &entry.unit_id.text_bytes());
        hasher.update(entry.hash);
    }
    Ok(hasher.finalize().into())
}

/// Lexical artifact directories under one trusted root plus their rows.
#[derive(Clone)]
pub struct LexicalArtifactStore {
    root: PathBuf,
    pool: PgPool,
}

impl LexicalArtifactStore {
    pub fn new(root: impl Into<PathBuf>, pool: PgPool) -> Self {
        Self {
            root: root.into(),
            pool,
        }
    }

    pub fn relpath(key: ProjectionGenerationKey) -> String {
        format!(
            "generations/{}/{}",
            key.source_id.as_uuid(),
            key.generation_id.as_uuid()
        )
    }

    /// The only directory a builder may stage this key's index in.
    pub fn staging_dir(&self, key: ProjectionGenerationKey) -> PathBuf {
        self.root.join("staging").join(format!(
            "{}-{}",
            key.source_id.as_uuid(),
            key.generation_id.as_uuid()
        ))
    }

    pub fn final_dir(&self, key: ProjectionGenerationKey) -> PathBuf {
        self.root.join(Self::relpath(key))
    }

    /// The Unit index of the newest finalized generation of `source_id`, as a
    /// base for the next build. Only efficiency depends on this choice: the
    /// seal compares every searchable Unit with the new manifest.
    pub fn latest_units_dir(&self, source_id: SourceId) -> Option<PathBuf> {
        let parent = self
            .root
            .join("generations")
            .join(source_id.as_uuid().to_string());
        std::fs::read_dir(parent)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let units = entry.path().join("units");
                let modified = std::fs::metadata(entry.path()).ok()?.modified().ok()?;
                units.is_dir().then_some((modified, units))
            })
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, units)| units)
    }

    /// Seals what is on disk now for `manifest`, without any database row.
    pub fn seal_from_disk(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        unit_manifest: &BodyUnitManifest,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        if unit_manifest.key != manifest.key() {
            return Err(LexicalArtifactError::Seal);
        }
        // One pass gives the receipt's segment digests and the Units' seal
        // entries; unchanged items reuse the entries sealed before.
        let (segments, expected) =
            unit_seal_entries(unit_manifest).map_err(|_| LexicalArtifactError::Seal)?;
        let units = unit_manifest_receipt_from_segments(unit_manifest.key, &segments)
            .map_err(|_| LexicalArtifactError::Seal)?
            .digest;
        self.seal_with(manifest, source, units, |persisted| {
            seal_lexical_hashes(&expected, persisted).is_ok()
        })
    }

    /// [`Self::seal_from_disk`] against a Unit manifest summary (T12).
    pub fn seal_from_summary(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        summary: &UnitManifestSummaryV1,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        if summary.key != manifest.key() {
            return Err(LexicalArtifactError::Seal);
        }
        self.seal_with(manifest, source, summary.receipt.digest, |persisted| {
            seal_lexical_hashes(&summary.units, persisted).is_ok()
        })
    }

    /// Seals the final directory of `manifest` against the Unit manifest whose
    /// receipt digest is `units`; `matches` compares its Units with the
    /// persisted Unit entries.
    fn seal_with(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        units: [u8; 32],
        matches: impl FnOnce(&[UnitSealEntry]) -> bool + Send,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let key = manifest.key();
        let inspected = InspectedLexical {
            key,
            tree: tree_digest(&self.final_dir(key))?,
            read: None,
        };
        self.finish_seal(manifest, source, inspected, units, matches)
    }

    /// The half of a seal read from the final directory alone: its file tree
    /// digest and, unless this process sealed the same files before, the
    /// persisted Unit entries and their seal digest. It needs no Unit
    /// manifest, so a re-verification reads it while restoring the payloads.
    pub fn inspect_final(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
    ) -> Result<InspectedLexical, LexicalArtifactError> {
        let key = manifest.key();
        let dir = self.final_dir(key);
        let tree = tree_digest(&dir)?;
        let sealed = sealed_cache()
            .lock()
            .map_err(|_| LexicalArtifactError::Io)?
            .keys()
            .any(|(at, files, _)| *at == key && *files == tree);
        let read = if sealed {
            None
        } else {
            let persisted = TantivyLexicalIndex::inspect_persisted(manifest, source, &dir)
                .map_err(|_| LexicalArtifactError::Index)?;
            let unit_seal = unit_seal(&persisted.units);
            Some(PersistedRead {
                persisted,
                unit_seal,
            })
        };
        Ok(InspectedLexical { key, tree, read })
    }

    /// Completes `inspected` against the Unit manifest whose receipt digest
    /// is `units`; `matches` compares its Units with the persisted entries.
    fn finish_seal(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        inspected: InspectedLexical,
        units: [u8; 32],
        matches: impl FnOnce(&[UnitSealEntry]) -> bool + Send,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let key = manifest.key();
        if inspected.key != key {
            return Err(LexicalArtifactError::Seal);
        }
        let tree = inspected.tree;
        // The same files and the same Unit manifest were sealed in this
        // process: reuse that seal (SD-T11 5).
        if let Some(seal) = sealed_cache()
            .lock()
            .map_err(|_| LexicalArtifactError::Io)?
            .get(&(key, tree, units))
        {
            return Ok(seal.clone());
        }
        let (persisted, unit_seal_digest) = match inspected.read {
            Some(PersistedRead {
                persisted,
                unit_seal,
            }) => {
                if !matches(&persisted.units) {
                    return Err(LexicalArtifactError::Seal);
                }
                (persisted, unit_seal)
            }
            None => {
                let persisted =
                    TantivyLexicalIndex::inspect_persisted(manifest, source, &self.final_dir(key))
                        .map_err(|_| LexicalArtifactError::Index)?;
                // Each side sorts every Unit; the comparison and the seal
                // digest run side by side.
                let (same, digest) = std::thread::scope(|scope| {
                    let same = scope.spawn(|| matches(&persisted.units));
                    let digest = unit_seal(&persisted.units);
                    (same.join().unwrap_or(false), digest)
                });
                if !same {
                    return Err(LexicalArtifactError::Seal);
                }
                (persisted, digest)
            }
        };
        let unit_count =
            u64::try_from(persisted.units.len()).map_err(|_| LexicalArtifactError::Seal)?;
        let seal = LexicalSealV1 {
            key,
            schema_version: persisted.schema_version.into(),
            analyzer_version: persisted.analyzer_version.into(),
            logical_digest: persisted.logical.digest,
            logical_count: persisted.logical.count,
            searchable_doc_count: persisted.resource_docs + unit_count,
            unit_seal_digest: unit_seal_digest?,
            unit_seal_count: unit_count,
            tree_digest: tree,
            index_relpath: Self::relpath(key),
        };
        let mut cache = sealed_cache()
            .lock()
            .map_err(|_| LexicalArtifactError::Io)?;
        if cache.len() >= CACHED_SEALS {
            cache.clear();
        }
        cache.insert((key, tree, units), seal.clone());
        Ok(seal)
    }

    /// Moves the staged directory to its immutable final path, seals it against
    /// the Unit manifest and the expected P1 lexical receipt, and records the
    /// artifact row (admitted only under the generation's live full guard).
    pub async fn finalize(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        staged: &Path,
        expected: &ArtifactReceipt,
        unit_manifest: &BodyUnitManifest,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let key = manifest.key();
        if staged != self.staging_dir(key) || expected.key != key {
            return Err(LexicalArtifactError::Path);
        }
        let target = self.final_dir(key);
        if target.exists() {
            return Err(LexicalArtifactError::Io);
        }
        let mut directories = vec![staged.to_path_buf()];
        for (_, path) in tree_files(staged)? {
            sync_path(&path)?;
            if let Some(parent) = path.parent() {
                directories.push(parent.to_path_buf());
            }
        }
        directories.sort();
        directories.dedup();
        for directory in &directories {
            sync_path(directory)?;
        }
        let parent = target.parent().ok_or(LexicalArtifactError::Path)?;
        std::fs::create_dir_all(parent).map_err(|_| LexicalArtifactError::Io)?;
        std::fs::rename(staged, &target).map_err(|_| LexicalArtifactError::Io)?;
        sync_path(parent)?;
        if let Some(staging_parent) = staged.parent() {
            sync_path(staging_parent)?;
        }
        let seal = self.seal_from_disk(manifest, source, unit_manifest)?;
        if seal.logical_digest != expected.digest || seal.logical_count != expected.count {
            return Err(LexicalArtifactError::Receipt);
        }
        sqlx::query(
            "INSERT INTO search_lexical_artifact (source_id,generation_id,index_relpath, \
             index_format_version,lexical_schema_version,tree_digest,logical_digest, \
             searchable_doc_count,unit_seal_digest,unit_seal_count,finalized_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,clock_timestamp())",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(&seal.index_relpath)
        .bind(LEXICAL_INDEX_FORMAT_VERSION)
        .bind(&seal.schema_version)
        .bind(sha256_text(&seal.tree_digest))
        .bind(sha256_text(&seal.logical_digest))
        .bind(i64::try_from(seal.searchable_doc_count).map_err(|_| LexicalArtifactError::Seal)?)
        .bind(sha256_text(&seal.unit_seal_digest))
        .bind(i64::try_from(seal.unit_seal_count).map_err(|_| LexicalArtifactError::Seal)?)
        .execute(&self.pool)
        .await?;
        Ok(seal)
    }

    /// Reopens the final directory and compares it with the saved row.
    pub async fn reopen_and_validate(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        unit_manifest: &BodyUnitManifest,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let seal = self.seal_from_disk(manifest, source, unit_manifest)?;
        self.matches_row(manifest, seal).await
    }

    /// [`Self::reopen_and_validate`] against a Unit manifest summary (T12).
    pub async fn reopen_and_validate_summary(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        summary: &UnitManifestSummaryV1,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let seal = self.seal_from_summary(manifest, source, summary)?;
        self.matches_row(manifest, seal).await
    }

    /// [`Self::reopen_and_validate_summary`] from the directory half read
    /// beforehand by [`Self::inspect_final`].
    pub async fn validate_inspected_summary(
        &self,
        manifest: &ProjectionGenerationManifest,
        source: &DiscoverableSource,
        inspected: InspectedLexical,
        summary: &UnitManifestSummaryV1,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        if summary.key != manifest.key() {
            return Err(LexicalArtifactError::Seal);
        }
        let seal = self.finish_seal(
            manifest,
            source,
            inspected,
            summary.receipt.digest,
            |persisted| seal_lexical_hashes(&summary.units, persisted).is_ok(),
        )?;
        self.matches_row(manifest, seal).await
    }

    /// `seal` when it equals the saved artifact row of `manifest`.
    async fn matches_row(
        &self,
        manifest: &ProjectionGenerationManifest,
        seal: LexicalSealV1,
    ) -> Result<LexicalSealV1, LexicalArtifactError> {
        let key = manifest.key();
        let row = sqlx::query(
            "SELECT index_relpath, index_format_version, lexical_schema_version, tree_digest, \
             logical_digest, searchable_doc_count, unit_seal_digest, unit_seal_count \
             FROM search_lexical_artifact WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(LexicalArtifactError::Drift)?;
        let same = row.try_get::<String, _>("index_relpath")? == seal.index_relpath
            && row.try_get::<String, _>("index_format_version")? == LEXICAL_INDEX_FORMAT_VERSION
            && row.try_get::<String, _>("lexical_schema_version")? == seal.schema_version
            && row.try_get::<String, _>("tree_digest")? == sha256_text(&seal.tree_digest)
            && row.try_get::<String, _>("logical_digest")? == sha256_text(&seal.logical_digest)
            && u64::try_from(row.try_get::<i64, _>("searchable_doc_count")?).ok()
                == Some(seal.searchable_doc_count)
            && row.try_get::<String, _>("unit_seal_digest")? == sha256_text(&seal.unit_seal_digest)
            && u64::try_from(row.try_get::<i64, _>("unit_seal_count")?).ok()
                == Some(seal.unit_seal_count);
        if !same {
            return Err(LexicalArtifactError::Drift);
        }
        Ok(seal)
    }
}
