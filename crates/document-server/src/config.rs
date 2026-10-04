//! Startup-only configuration. Errors and Debug never include configuration values.
use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use sqlx::postgres::PgConnectOptions;
use thiserror::Error;

use crate::identity::PoCIdentityProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Serve,
    Migrate,
    BootstrapPoc,
}

pub trait ConfigSource {
    fn get(&self, name: &str) -> Option<String>;
}
pub struct ProcessEnvironment;
impl ConfigSource for ProcessEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    #[error("KP_RUNTIME_MODE must explicitly select poc")]
    RuntimeMode,
    #[error("KP_IDENTITY_PROFILE must select a supported fixed PoC profile")]
    IdentityProfile,
    #[error("bootstrap-poc requires the human profile")]
    HumanProfileRequired,
    #[error("required configuration variable is missing or empty: {0}")]
    Missing(&'static str),
    #[error("configuration variable is invalid: {0}")]
    Invalid(&'static str),
    #[error("non-loopback bind requires explicit PoC override")]
    NonLoopback,
}

pub struct RuntimeConfig {
    command: Command,
    database_url: String,
    profile: Option<PoCIdentityProfile>,
    serve: Option<ServeConfig>,
}

#[derive(Clone)]
pub struct ServeConfig {
    bind: SocketAddr,
    storage_root: PathBuf,
    dsi_worker: PathBuf,
    diff_worker: PathBuf,
    web_dist: Option<PathBuf>,
    pdfium_runtime_dir: Option<PathBuf>,
    non_loopback_warning: bool,
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("command", &self.command)
            .field("profile", &self.profile)
            .field("values", &"[REDACTED]")
            .finish()
    }
}
impl fmt::Debug for ServeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeConfig")
            .field("values", &"[REDACTED]")
            .finish()
    }
}

fn required(source: &impl ConfigSource, key: &'static str) -> Result<String, ConfigError> {
    source
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(key))
}
fn path(source: &impl ConfigSource, key: &'static str) -> Result<PathBuf, ConfigError> {
    let value = required(source, key)?;
    if value.chars().any(char::is_control) {
        return Err(ConfigError::Invalid(key));
    }
    Ok(PathBuf::from(value))
}

impl RuntimeConfig {
    pub fn from_env(source: &impl ConfigSource, command: Command) -> Result<Self, ConfigError> {
        if source.get("KP_RUNTIME_MODE").as_deref() != Some("poc") {
            return Err(ConfigError::RuntimeMode);
        }
        let profile = if command == Command::Migrate {
            None
        } else {
            Some(match source.get("KP_IDENTITY_PROFILE").as_deref() {
                Some("poc-human") => PoCIdentityProfile::Human,
                Some("poc-agent") => PoCIdentityProfile::Agent,
                _ => return Err(ConfigError::IdentityProfile),
            })
        };
        if command == Command::BootstrapPoc && profile != Some(PoCIdentityProfile::Human) {
            return Err(ConfigError::HumanProfileRequired);
        }
        let database_url = required(source, "KP_DATABASE_URL")?;
        if !(database_url.starts_with("postgres://") || database_url.starts_with("postgresql://"))
            || PgConnectOptions::from_str(&database_url).is_err()
        {
            return Err(ConfigError::Invalid("KP_DATABASE_URL"));
        }
        let serve = if command == Command::Serve {
            let profile = profile.ok_or(ConfigError::IdentityProfile)?;
            let default_bind = match profile {
                PoCIdentityProfile::Human => "127.0.0.1:8080",
                PoCIdentityProfile::Agent => "127.0.0.1:8081",
            };
            let bind: SocketAddr = source
                .get("KP_BIND")
                .unwrap_or_else(|| default_bind.into())
                .parse()
                .map_err(|_| ConfigError::Invalid("KP_BIND"))?;
            let allow_non_loopback = match source.get("KP_POC_ALLOW_NON_LOOPBACK").as_deref() {
                None | Some("false") => false,
                Some("true") => true,
                _ => return Err(ConfigError::Invalid("KP_POC_ALLOW_NON_LOOPBACK")),
            };
            if !bind.ip().is_loopback() && !allow_non_loopback {
                return Err(ConfigError::NonLoopback);
            }
            Some(ServeConfig {
                bind,
                storage_root: path(source, "KP_STORAGE_ROOT")?,
                dsi_worker: path(source, "KP_DSI_WORKER")?,
                diff_worker: path(source, "KP_DIFF_WORKER")?,
                web_dist: if profile == PoCIdentityProfile::Human {
                    Some(path(source, "KP_WEB_DIST")?)
                } else {
                    None
                },
                pdfium_runtime_dir: source
                    .get("KP_DSI_PDFIUM_RUNTIME_DIR")
                    .map(|_| path(source, "KP_DSI_PDFIUM_RUNTIME_DIR"))
                    .transpose()?,
                non_loopback_warning: !bind.ip().is_loopback(),
            })
        } else {
            None
        };
        Ok(Self {
            command,
            database_url,
            profile,
            serve,
        })
    }
    pub const fn command(&self) -> Command {
        self.command
    }
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
    pub const fn profile(&self) -> Option<PoCIdentityProfile> {
        self.profile
    }
    pub fn serve(&self) -> Option<&ServeConfig> {
        self.serve.as_ref()
    }
}
impl ServeConfig {
    pub(crate) fn capture_targets(mut self) -> Self {
        // Resolve once before assembling adapters; keep an absent path for a safe
        // unavailable result rather than manufacturing a usable fallback.
        for path in [
            &mut self.storage_root,
            &mut self.dsi_worker,
            &mut self.diff_worker,
        ] {
            if let Ok(canonical) = path.canonicalize() {
                *path = canonical;
            }
        }
        for path in [&mut self.web_dist, &mut self.pdfium_runtime_dir]
            .into_iter()
            .flatten()
        {
            if let Ok(canonical) = path.canonicalize() {
                *path = canonical;
            }
        }
        self
    }
    pub const fn bind(&self) -> SocketAddr {
        self.bind
    }
    pub fn storage_root(&self) -> &Path {
        &self.storage_root
    }
    pub fn dsi_worker(&self) -> &Path {
        &self.dsi_worker
    }
    pub fn diff_worker(&self) -> &Path {
        &self.diff_worker
    }
    pub fn web_dist(&self) -> Option<&Path> {
        self.web_dist.as_deref()
    }
    pub fn pdfium_runtime_dir(&self) -> Option<&Path> {
        self.pdfium_runtime_dir.as_deref()
    }
    pub const fn non_loopback_warning(&self) -> bool {
        self.non_loopback_warning
    }
}
