use crate::{ErrorCode, FormatId, InspectionProfile, PocError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FixtureClass {
    Base,
    Semantic,
    Noise,
    Editorial,
    Hostile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedOutcome {
    Success,
    SameAs { case_id: String },
    DifferentFrom { case_id: String },
    Error { code: ErrorCode },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureCase {
    pub id: String,
    pub class: FixtureClass,
    pub path: String,
    pub format: FormatId,
    pub sha256: String,
    pub size: u64,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub delimiter: Option<String>,
    #[serde(default)]
    pub text_encoding: Option<String>,
    #[serde(default)]
    pub script_required: bool,
    pub expected: ExpectedOutcome,
}

impl FixtureCase {
    pub fn inspection_profile(&self) -> Result<InspectionProfile, PocError> {
        let csv_delimiter = match self.delimiter.as_deref() {
            Some(value) => {
                let bytes = value.as_bytes();
                if bytes.len() != 1 || !bytes[0].is_ascii() {
                    return Err(PocError::InvalidManifest(format!(
                        "{} delimiter must be exactly one ASCII byte",
                        self.id
                    )));
                }
                Some(bytes[0])
            }
            None => None,
        };

        Ok(InspectionProfile {
            id: self.profile.clone(),
            csv_delimiter,
            text_encoding: self.text_encoding.clone(),
            html_script_required: self.script_required,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub cases: Vec<FixtureCase>,
}

impl FixtureManifest {
    pub fn from_json(json: &str) -> Result<Self, PocError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|error| PocError::InvalidManifest(error.to_string()))?;

        let manifest = if value.is_array() {
            let cases: Vec<FixtureCase> = serde_json::from_value(value)
                .map_err(|error| PocError::InvalidManifest(error.to_string()))?;
            Self {
                schema_version: 1,
                cases,
            }
        } else {
            serde_json::from_value(value)
                .map_err(|error| PocError::InvalidManifest(error.to_string()))?
        };

        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_path(path: &Path) -> Result<Self, PocError> {
        let json = fs::read_to_string(path).map_err(|error| {
            PocError::InvalidManifest(format!("cannot read {}: {error}", path.display()))
        })?;
        Self::from_json(&json)
    }

    pub fn validate(&self) -> Result<(), PocError> {
        if self.schema_version != 1 {
            return Err(PocError::InvalidManifest(format!(
                "unsupported schema_version {}",
                self.schema_version
            )));
        }

        let mut ids = BTreeSet::new();
        let mut classes = BTreeMap::new();

        for case in &self.cases {
            if case.id.trim().is_empty() {
                return Err(PocError::InvalidManifest("case id must not be empty".into()));
            }
            if !ids.insert(case.id.clone()) {
                return Err(PocError::InvalidManifest(format!(
                    "duplicate case id {}",
                    case.id
                )));
            }
            validate_relative_path(&case.path)?;
            validate_sha256(&case.sha256)?;
            case.inspection_profile()?;
            classes.insert(case.id.clone(), case.class);
        }

        for case in &self.cases {
            let reference = match &case.expected {
                ExpectedOutcome::SameAs { case_id } | ExpectedOutcome::DifferentFrom { case_id } => {
                    Some(case_id)
                }
                ExpectedOutcome::Success | ExpectedOutcome::Error { .. } => None,
            };

            if let Some(reference) = reference {
                let Some(class) = classes.get(reference) else {
                    return Err(PocError::InvalidManifest(format!(
                        "{} references missing BASE case {}",
                        case.id, reference
                    )));
                };
                if *class != FixtureClass::Base {
                    return Err(PocError::InvalidManifest(format!(
                        "{} references non-BASE case {}",
                        case.id, reference
                    )));
                }
            }
        }

        Ok(())
    }
}

pub fn fixture_case(path: &str, sha256: String, size: u64) -> FixtureCase {
    FixtureCase {
        id: "fixture".to_owned(),
        class: FixtureClass::Base,
        path: path.to_owned(),
        format: FormatId::Txt,
        sha256,
        size,
        profile: default_profile(),
        delimiter: None,
        text_encoding: None,
        script_required: false,
        expected: ExpectedOutcome::Success,
    }
}

fn default_profile() -> String {
    "dsi-v0".to_owned()
}

fn validate_relative_path(path: &str) -> Result<(), PocError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(PocError::InvalidManifest(format!(
            "fixture path must be relative and traversal-free: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), PocError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PocError::InvalidManifest(
            "fixture sha256 must be exactly 64 hexadecimal characters".into(),
        ));
    }
    Ok(())
}
