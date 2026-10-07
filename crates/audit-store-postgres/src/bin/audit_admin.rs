//! `audit-admin`: operator CLI for the Audit Store (design §8–§11).
//!
//! Connections: `AUDIT_STORE_DATABASE_URL` (the operator's own Store login;
//! superuser and audit_store_owner sessions are refused except for
//! bootstrap-admin/bind/unbind, which require an owner member) and
//! `AUDIT_STORE_MIGRATE_DATABASE_URL` (migrate only). Output is JSON on
//! stdout; errors carry codes only. Files are written with mode 0600.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use audit_store_postgres::admin::{AccessChange, AccessOperation, AuditAdmin, parse_utc_text};
use audit_store_postgres::files::{
    CheckpointFile, ExportRequest, export_identity_chain_recovery, export_to_dir, write_checkpoint,
};
use audit_store_postgres::migrate;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const USAGE: &str = "usage: audit-admin <command> [--flag value]...
commands:
  migrate
  status | posture
  bootstrap-admin --db-role R --issuer I --principal P
  bind --db-role R --issuer I --principal P | unbind --db-role R
  grant|revoke --issuer I --principal P --capability C
  investigate [--filter JSON] [--page-size N] [--max-pages N]
  export --dir D [--operation export|verify|identity_chain] [--filter JSON]
         [--page-size N] [--max-pages N] [--checkpoint FILE]
  verify [--from N] [--to N] | verify-recovery
  checkpoint --out FILE
  export-identity-chain-recovery --dir D [--checkpoint FILE]
  set-retention --policy-id ID --selector JSON --retain-days N|none
  expire --policy-id ID --expected-revision N --cutoff YYYY-MM-DDTHH:MM:SS.ffffffZ --limit N
  purge-body --event-id UUID --reason-code CODE
  begin-recovery-epoch --checkpoint FILE --relay-max-seq N
  rebind-fingerprint";

/// A database URL that never appears in Debug output or logs.
struct SecretUrl(String);

impl fmt::Debug for SecretUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug)]
struct Config {
    store_url: Option<SecretUrl>,
    migrate_url: Option<SecretUrl>,
}

impl Config {
    fn from_env() -> Self {
        let read = |name| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .map(SecretUrl)
        };
        Self {
            store_url: read("AUDIT_STORE_DATABASE_URL"),
            migrate_url: read("AUDIT_STORE_MIGRATE_DATABASE_URL"),
        }
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Failed(String),
    Violations,
}

impl<E: fmt::Display> From<E> for CliError {
    fn from(error: E) -> Self {
        Self::Failed(error.to_string())
    }
}

struct Args {
    command: String,
    flags: BTreeMap<String, String>,
}

impl Args {
    fn parse(mut raw: impl Iterator<Item = String>) -> Result<Self, CliError> {
        let command = raw
            .next()
            .ok_or_else(|| CliError::Usage("missing command".into()))?;
        let mut flags = BTreeMap::new();
        while let Some(flag) = raw.next() {
            let name = flag
                .strip_prefix("--")
                .ok_or_else(|| CliError::Usage(format!("unexpected argument {flag}")))?;
            let value = raw
                .next()
                .ok_or_else(|| CliError::Usage(format!("--{name} needs a value")))?;
            if flags.insert(name.to_owned(), value).is_some() {
                return Err(CliError::Usage(format!("--{name} given twice")));
            }
        }
        Ok(Self { command, flags })
    }

    fn get(&self, name: &str) -> Result<&str, CliError> {
        self.flags
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| CliError::Usage(format!("--{name} is required")))
    }

    fn optional(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: Option<T>) -> Result<T, CliError> {
        match (self.optional(name), default) {
            (Some(text), _) => text
                .parse()
                .map_err(|_| CliError::Usage(format!("--{name} must be a number"))),
            (None, Some(value)) => Ok(value),
            (None, None) => Err(CliError::Usage(format!("--{name} is required"))),
        }
    }

    fn json(&self, name: &str, default: Value) -> Result<Value, CliError> {
        match self.optional(name) {
            Some(text) => serde_json::from_str(text)
                .map_err(|_| CliError::Usage(format!("--{name} must be JSON"))),
            None => Ok(default),
        }
    }
}

fn print(value: &impl Serialize) -> Result<(), CliError> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

async fn pool(url: Option<&SecretUrl>, name: &str) -> Result<PgPool, CliError> {
    let url = url.ok_or_else(|| CliError::Usage(format!("{name} is not set")))?;
    Ok(PgPoolOptions::new()
        .max_connections(2)
        .connect(&url.0)
        .await?)
}

async fn operator(config: &Config) -> Result<AuditAdmin, CliError> {
    let pool = pool(config.store_url.as_ref(), "AUDIT_STORE_DATABASE_URL").await?;
    Ok(AuditAdmin::connect(pool).await?)
}

async fn owner(config: &Config) -> Result<AuditAdmin, CliError> {
    let pool = pool(config.store_url.as_ref(), "AUDIT_STORE_DATABASE_URL").await?;
    Ok(AuditAdmin::connect_owner(pool).await?)
}

fn operation(name: &str) -> Result<AccessOperation, CliError> {
    match name {
        "export" => Ok(AccessOperation::Export),
        "verify" => Ok(AccessOperation::Verify),
        "identity_chain" => Ok(AccessOperation::IdentityChain),
        _ => Err(CliError::Usage(
            "--operation must be export|verify|identity_chain".into(),
        )),
    }
}

fn checkpoint_arg(args: &Args) -> Result<Option<audit_core::Checkpoint>, CliError> {
    args.optional("checkpoint")
        .map(|path| {
            CheckpointFile::read(&PathBuf::from(path))?
                .checkpoint()
                .ok_or_else(|| CliError::Failed("invalid checkpoint file".into()))
        })
        .transpose()
}

async fn run(args: Args, config: Config) -> Result<(), CliError> {
    match args.command.as_str() {
        "migrate" => {
            let pool = pool(
                config.migrate_url.as_ref(),
                "AUDIT_STORE_MIGRATE_DATABASE_URL",
            )
            .await?;
            migrate(&pool).await?;
            print(&json!({"status": "migrated"}))
        }
        "status" => print(&operator(&config).await?.store_status().await?),
        "posture" => {
            let violations = operator(&config).await?.posture().await?;
            for violation in &violations {
                print(violation)?;
            }
            if violations.is_empty() {
                print(&json!({"status": "clean"}))
            } else {
                Err(CliError::Violations)
            }
        }
        "bootstrap-admin" => print(
            &owner(&config)
                .await?
                .bootstrap_administrator(
                    args.get("db-role")?,
                    args.get("issuer")?,
                    args.get("principal")?,
                )
                .await?,
        ),
        "bind" => print(
            &owner(&config)
                .await?
                .bind_principal(
                    args.get("db-role")?,
                    args.get("issuer")?,
                    args.get("principal")?,
                )
                .await?,
        ),
        "unbind" => print(
            &owner(&config)
                .await?
                .unbind_principal(args.get("db-role")?)
                .await?,
        ),
        "grant" | "revoke" => {
            let change = if args.command == "grant" {
                AccessChange::Grant
            } else {
                AccessChange::Revoke
            };
            print(
                &operator(&config)
                    .await?
                    .change_access(
                        args.get("issuer")?,
                        args.get("principal")?,
                        args.get("capability")?,
                        change,
                    )
                    .await?,
            )
        }
        "investigate" => {
            let admin = operator(&config).await?;
            let filter = args.json("filter", json!({}))?;
            let token = admin
                .open_access(
                    AccessOperation::Investigate,
                    &filter,
                    args.number("page-size", Some(50))?,
                    args.number("max-pages", Some(1))?,
                )
                .await?;
            let pages = admin.read_all(&token).await?;
            let mut count = 0_i64;
            for line in pages.iter().flatten() {
                println!("{}", line.line);
                count += 1;
            }
            let digests: Vec<String> = pages
                .iter()
                .map(|p| audit_store_postgres::files::page_digest(p))
                .collect();
            admin.close_access(token.secret(), count, &digests).await?;
            Ok(())
        }
        "export" => {
            let admin = operator(&config).await?;
            let request = ExportRequest {
                operation: operation(args.optional("operation").unwrap_or("export"))?,
                filter: args.json("filter", json!({}))?,
                page_size: args.number("page-size", Some(1000))?,
                max_pages: args.number("max-pages", Some(100))?,
                checkpoint: checkpoint_arg(&args)?,
            };
            let outcome = export_to_dir(&admin, &request, &PathBuf::from(args.get("dir")?)).await?;
            print(&outcome.manifest)
        }
        "verify" => print(
            &operator(&config)
                .await?
                .verify(
                    args.number("from", Some(1))?.into(),
                    args.optional("to")
                        .map(str::parse)
                        .transpose()
                        .map_err(|_| CliError::Usage("--to must be a number".into()))?,
                )
                .await?,
        ),
        "verify-recovery" => print(&operator(&config).await?.verify_recovery().await?),
        "checkpoint" => {
            let record = operator(&config).await?.checkpoint().await?;
            let file = write_checkpoint(&record, &PathBuf::from(args.get("out")?))?;
            print(&file)
        }
        "export-identity-chain-recovery" => {
            let admin = operator(&config).await?;
            let outcome = export_identity_chain_recovery(
                &admin,
                checkpoint_arg(&args)?,
                &PathBuf::from(args.get("dir")?),
            )
            .await?;
            print(&outcome.manifest)
        }
        "set-retention" => {
            let retain_days = match args.get("retain-days")? {
                "none" => None,
                text => Some(
                    text.parse()
                        .map_err(|_| CliError::Usage("--retain-days must be N or none".into()))?,
                ),
            };
            print(
                &operator(&config)
                    .await?
                    .set_retention_policy(
                        args.get("policy-id")?,
                        &args.json("selector", Value::Null)?,
                        retain_days,
                    )
                    .await?,
            )
        }
        "expire" => {
            let cutoff = parse_utc_text(args.get("cutoff")?).ok_or_else(|| {
                CliError::Usage("--cutoff must be YYYY-MM-DDTHH:MM:SS.ffffffZ".into())
            })?;
            print(
                &operator(&config)
                    .await?
                    .expire(
                        args.get("policy-id")?,
                        args.number("expected-revision", None)?,
                        cutoff,
                        args.number("limit", Some(100))?,
                    )
                    .await?,
            )
        }
        "purge-body" => {
            let event_id = Uuid::parse_str(args.get("event-id")?)
                .map_err(|_| CliError::Usage("--event-id must be a UUID".into()))?;
            print(
                &operator(&config)
                    .await?
                    .purge_body(event_id, args.get("reason-code")?)
                    .await?,
            )
        }
        "begin-recovery-epoch" => {
            let checkpoint = checkpoint_arg(&args)?
                .ok_or_else(|| CliError::Usage("--checkpoint is required".into()))?;
            print(
                &operator(&config)
                    .await?
                    .begin_recovery_epoch(&checkpoint, args.number("relay-max-seq", None)?)
                    .await?,
            )
        }
        "rebind-fingerprint" => print(&operator(&config).await?.rebind_fingerprint().await?),
        other => Err(CliError::Usage(format!("unknown command {other}"))),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
        Err(_) => return ExitCode::from(2),
    };
    match run(args, Config::from_env()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Violations) => {
            eprintln!("posture violations found");
            ExitCode::from(3)
        }
        Err(CliError::Failed(message)) => {
            eprintln!("audit-admin: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_debug_redacts_urls() {
        let config = Config {
            store_url: Some(SecretUrl("postgres://operator:secret@db/audit".into())),
            migrate_url: Some(SecretUrl("postgres://migrator:secret@db/audit".into())),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("postgres://"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn args_parse_flags_and_refuse_duplicates() {
        let args =
            Args::parse(["verify", "--from", "3"].into_iter().map(String::from)).expect("parses");
        assert_eq!(args.command, "verify");
        assert_eq!(args.number::<i64>("from", None).expect("number"), 3);
        assert!(Args::parse(["x", "--a", "1", "--a", "2"].into_iter().map(String::from)).is_err());
        assert!(Args::parse(["x", "--a"].into_iter().map(String::from)).is_err());
        assert!(Args::parse(["x", "loose"].into_iter().map(String::from)).is_err());
    }
}
