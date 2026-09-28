use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use document_application::RepositoryError;
use document_domain::normalize_folder_name;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{SYSTEM_ROOT_FOLDER_ID, error::map_statement_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FolderPreflightCategory {
    InvalidName,
    NonNormalizedName,
    NormalizationCollision,
    MissingParent,
    MultipleRoots,
    UnexpectedRoot,
    Cycle,
    Unrooted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FolderPreflightFinding {
    pub folder_id: Uuid,
    pub category: FolderPreflightCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPreflightReport {
    pub findings: Vec<FolderPreflightFinding>,
}

impl FolderPreflightReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Run under a Folder write stop immediately before migration 0007.
/// Only Folder IDs and classifications leave this function; names stay local.
pub async fn preflight_folder_names(
    pool: &PgPool,
) -> Result<FolderPreflightReport, RepositoryError> {
    let rows = sqlx::query("SELECT folder_id,parent_folder_id,name FROM folders")
        .fetch_all(pool)
        .await
        .map_err(map_statement_error)?;
    let mut folders = HashMap::new();
    for row in rows {
        let id: Uuid = row.try_get("folder_id").map_err(map_statement_error)?;
        let parent: Option<Uuid> = row
            .try_get("parent_folder_id")
            .map_err(map_statement_error)?;
        let name: String = row.try_get("name").map_err(map_statement_error)?;
        folders.insert(id, (parent, name));
    }
    let mut findings = BTreeSet::new();
    let roots: Vec<_> = folders
        .iter()
        .filter_map(|(id, (parent, _))| parent.is_none().then_some(*id))
        .collect();
    if roots.len() != 1 {
        for id in &roots {
            findings.insert((*id, FolderPreflightCategory::MultipleRoots));
        }
    }
    if folders
        .get(&SYSTEM_ROOT_FOLDER_ID)
        .is_none_or(|(parent, _)| parent.is_some())
    {
        findings.insert((
            SYSTEM_ROOT_FOLDER_ID,
            FolderPreflightCategory::UnexpectedRoot,
        ));
    }
    for id in &roots {
        if *id != SYSTEM_ROOT_FOLDER_ID {
            findings.insert((*id, FolderPreflightCategory::UnexpectedRoot));
        }
    }
    let mut names: BTreeMap<(Uuid, String), Vec<Uuid>> = BTreeMap::new();
    for (id, (parent, name)) in &folders {
        match normalize_folder_name(name) {
            Ok(normalized) => {
                if normalized != *name {
                    findings.insert((*id, FolderPreflightCategory::NonNormalizedName));
                }
                if let Some(parent) = parent {
                    names.entry((*parent, normalized)).or_default().push(*id);
                }
            }
            Err(_) => {
                findings.insert((*id, FolderPreflightCategory::InvalidName));
            }
        }
        if let Some(parent) = parent
            && !folders.contains_key(parent)
        {
            findings.insert((*id, FolderPreflightCategory::MissingParent));
        }
    }
    for colliders in names.values().filter(|ids| ids.len() > 1) {
        for id in colliders {
            findings.insert((*id, FolderPreflightCategory::NormalizationCollision));
        }
    }
    for id in folders.keys() {
        let mut seen = HashSet::new();
        let mut current = *id;
        loop {
            if !seen.insert(current) {
                findings.insert((*id, FolderPreflightCategory::Cycle));
                break;
            }
            let Some((parent, _)) = folders.get(&current) else {
                findings.insert((*id, FolderPreflightCategory::Unrooted));
                break;
            };
            match parent {
                Some(parent) => current = *parent,
                None if current == SYSTEM_ROOT_FOLDER_ID => break,
                None => {
                    findings.insert((*id, FolderPreflightCategory::Unrooted));
                    break;
                }
            }
        }
    }
    Ok(FolderPreflightReport {
        findings: findings
            .into_iter()
            .map(|(folder_id, category)| FolderPreflightFinding {
                folder_id,
                category,
            })
            .collect(),
    })
}
