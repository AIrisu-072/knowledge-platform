//! Process configuration from the environment, in the Document server's
//! `KP_*` vocabulary. Secret values (the database URL, the token) are kept
//! out of every error message.

use std::net::SocketAddr;
use std::path::PathBuf;

use http_local_guard::LocalOriginPolicy;

use crate::identity::PocProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Serve,
    Migrate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    RuntimeMode,
    IdentityProfile,
    Missing(&'static str),
    Invalid(&'static str),
    NonLoopback,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeMode => f.write_str("KP_RUNTIME_MODE must be poc"),
            Self::IdentityProfile => f.write_str("KP_IDENTITY_PROFILE must be poc-human or poc-agent"),
            Self::Missing(name) => write!(f, "{name} is required"),
            Self::Invalid(name) => write!(f, "{name} is invalid"),
            Self::NonLoopback => f.write_str(
                "a non-loopback KP_BIND needs KP_POC_ALLOW_NON_LOOPBACK=true; the PoC identity is not production authentication",
            ),
        }
    }
}

/// Where configuration values come from; the process environment in production.
pub trait ConfigSource {
    fn get(&self, key: &str) -> Option<String>;
}

pub struct ProcessEnvironment;

impl ConfigSource for ProcessEnvironment {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// The Discover retriever profile the server applies to every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrieverProfileName {
    Exploratory,
    Capability,
    Knowledge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeConfig {
    pub profile: PocProfile,
    pub bind: SocketAddr,
    /// File holding the shared Bearer token of this process (secret).
    pub token_file: PathBuf,
    /// The Search worker's Source file; both processes read the same one.
    pub source_config: PathBuf,
    pub retriever_profile: RetrieverProfileName,
    /// Requests hold a connection for each access check.
    pub pool_size: u32,
    pub non_loopback_warning: bool,
    /// `KP_ALLOWED_HOSTS`: when set, the local origin guard wraps every route.
    pub allowed_hosts: Option<LocalOriginPolicy>,
}

pub struct ServerConfig {
    pub command: Command,
    database_url: String,
    pub serve: Option<ServeConfig>,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("command", &self.command)
            .field("database_url", &"<redacted>")
            .field("serve", &self.serve)
            .finish()
    }
}

impl ServerConfig {
    pub fn from_env(source: &impl ConfigSource, command: Command) -> Result<Self, ConfigError> {
        if source.get("KP_RUNTIME_MODE").as_deref() != Some("poc") {
            return Err(ConfigError::RuntimeMode);
        }
        let database_url = required(source, "KP_DATABASE_URL")?;
        if !(database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")) {
            return Err(ConfigError::Invalid("KP_DATABASE_URL"));
        }
        let serve = match command {
            Command::Migrate => None,
            Command::Serve => Some(serve_config(source)?),
        };
        Ok(Self {
            command,
            database_url,
            serve,
        })
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

fn serve_config(source: &impl ConfigSource) -> Result<ServeConfig, ConfigError> {
    let profile = match source.get("KP_IDENTITY_PROFILE").as_deref() {
        Some("poc-human") => PocProfile::Human,
        Some("poc-agent") => PocProfile::Agent,
        _ => return Err(ConfigError::IdentityProfile),
    };
    let default_bind = match profile {
        PocProfile::Human => "127.0.0.1:8090",
        PocProfile::Agent => "127.0.0.1:8091",
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
    let retriever_profile = match source.get("KP_SEARCH_RETRIEVER_PROFILE").as_deref() {
        None | Some("exploratory") => RetrieverProfileName::Exploratory,
        Some("capability") => RetrieverProfileName::Capability,
        Some("knowledge") => RetrieverProfileName::Knowledge,
        _ => return Err(ConfigError::Invalid("KP_SEARCH_RETRIEVER_PROFILE")),
    };
    let pool_size = match source.get("KP_SEARCH_POOL_SIZE") {
        None => 16,
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|size| (1..=256).contains(size))
            .ok_or(ConfigError::Invalid("KP_SEARCH_POOL_SIZE"))?,
    };
    Ok(ServeConfig {
        profile,
        bind,
        token_file: path(source, "KP_SEARCH_TOKEN_FILE")?,
        source_config: path(source, "SEARCH_WORKER_CONFIG")?,
        retriever_profile,
        pool_size,
        non_loopback_warning: !bind.ip().is_loopback(),
        allowed_hosts: source
            .get("KP_ALLOWED_HOSTS")
            .map(|list| LocalOriginPolicy::parse(&list))
            .transpose()
            .map_err(|_| ConfigError::Invalid("KP_ALLOWED_HOSTS"))?,
    })
}

fn required(source: &impl ConfigSource, name: &'static str) -> Result<String, ConfigError> {
    source
        .get(name)
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn path(source: &impl ConfigSource, name: &'static str) -> Result<PathBuf, ConfigError> {
    required(source, name).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Map(BTreeMap<&'static str, &'static str>);

    impl ConfigSource for Map {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).map(|value| (*value).to_owned())
        }
    }

    fn serve_env(extra: &[(&'static str, &'static str)]) -> Map {
        let mut map = BTreeMap::from([
            ("KP_RUNTIME_MODE", "poc"),
            ("KP_DATABASE_URL", "postgres://u:p@127.0.0.1/db"),
            ("KP_IDENTITY_PROFILE", "poc-agent"),
            ("KP_SEARCH_TOKEN_FILE", "/run/secrets/search-agent-token"),
            ("SEARCH_WORKER_CONFIG", "/etc/search/source.json"),
        ]);
        map.extend(extra.iter().copied());
        Map(map)
    }

    #[test]
    fn serve_defaults_follow_the_profile() {
        let config = ServerConfig::from_env(&serve_env(&[]), Command::Serve).unwrap();
        let serve = config.serve.unwrap();
        assert_eq!(serve.profile, PocProfile::Agent);
        assert_eq!(serve.bind, "127.0.0.1:8091".parse().unwrap());
        assert_eq!(serve.retriever_profile, RetrieverProfileName::Exploratory);
        assert_eq!(serve.pool_size, 16);
        assert!(!serve.non_loopback_warning);
        assert!(serve.allowed_hosts.is_none());
    }

    #[test]
    fn allowed_hosts_are_parsed_or_rejected() {
        let env = serve_env(&[("KP_ALLOWED_HOSTS", "127.0.0.1:8091,localhost:8091")]);
        let serve = ServerConfig::from_env(&env, Command::Serve)
            .unwrap()
            .serve
            .unwrap();
        assert_eq!(
            serve.allowed_hosts.unwrap().hosts(),
            ["127.0.0.1:8091", "localhost:8091"]
        );
        let env = serve_env(&[("KP_ALLOWED_HOSTS", "http://evil.example")]);
        assert_eq!(
            ServerConfig::from_env(&env, Command::Serve).unwrap_err(),
            ConfigError::Invalid("KP_ALLOWED_HOSTS")
        );
    }

    #[test]
    fn non_loopback_needs_the_explicit_opt_in() {
        let env = serve_env(&[("KP_BIND", "0.0.0.0:8090")]);
        assert_eq!(
            ServerConfig::from_env(&env, Command::Serve).unwrap_err(),
            ConfigError::NonLoopback
        );
        let env = serve_env(&[
            ("KP_BIND", "0.0.0.0:8090"),
            ("KP_POC_ALLOW_NON_LOOPBACK", "true"),
        ]);
        let serve = ServerConfig::from_env(&env, Command::Serve)
            .unwrap()
            .serve
            .unwrap();
        assert!(serve.non_loopback_warning);
    }

    #[test]
    fn missing_or_bad_values_are_rejected_without_echoing_them() {
        let mut env = serve_env(&[]);
        env.0.remove("KP_SEARCH_TOKEN_FILE");
        assert_eq!(
            ServerConfig::from_env(&env, Command::Serve).unwrap_err(),
            ConfigError::Missing("KP_SEARCH_TOKEN_FILE")
        );
        let env = serve_env(&[("KP_IDENTITY_PROFILE", "admin")]);
        assert_eq!(
            ServerConfig::from_env(&env, Command::Serve).unwrap_err(),
            ConfigError::IdentityProfile
        );
        let env = serve_env(&[("KP_DATABASE_URL", "mysql://secret@host/db")]);
        let error = ServerConfig::from_env(&env, Command::Serve).unwrap_err();
        assert_eq!(error, ConfigError::Invalid("KP_DATABASE_URL"));
        assert!(!error.to_string().contains("secret"));
        let env = serve_env(&[("KP_RUNTIME_MODE", "production")]);
        assert_eq!(
            ServerConfig::from_env(&env, Command::Serve).unwrap_err(),
            ConfigError::RuntimeMode
        );
    }

    #[test]
    fn migrate_needs_only_the_database() {
        let env = Map(BTreeMap::from([
            ("KP_RUNTIME_MODE", "poc"),
            ("KP_DATABASE_URL", "postgresql://u:p@127.0.0.1/db"),
        ]));
        let config = ServerConfig::from_env(&env, Command::Migrate).unwrap();
        assert!(config.serve.is_none());
        assert!(!format!("{config:?}").contains("u:p"));
    }
}
