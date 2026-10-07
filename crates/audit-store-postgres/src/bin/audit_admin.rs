//! `audit-admin`: operator CLI for the Audit Store (design §8–§11).
//!
//! Connections: `AUDIT_STORE_DATABASE_URL` (the operator's own Store login;
//! superuser and audit_store_owner sessions are refused except for the
//! owner-only commands bootstrap-admin / bind / unbind /
//! register-source-service, which require an owner member) and
//! `AUDIT_STORE_MIGRATE_DATABASE_URL` (migrate only). URLs carrying
//! `options` are refused and every session must report
//! `SHOW synchronous_commit = on`. Output is JSON on stdout; errors carry
//! codes only. Files are written with mode 0600.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use audit_store_postgres::admin::{
    AccessChange, AccessOperation, AuditAdmin, RecoveryExpectation, parse_utc_text,
};
use audit_store_postgres::files::{
    CheckpointFile, ExportOutcome, ExportRequest, FileError, export_identity_chain_recovery,
    export_to_dir, write_checkpoint,
};
use audit_store_postgres::migrate;
use audit_store_postgres::session::{check_url, require_synchronous_commit};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const USAGE: &str = "usage: audit-admin <command> [--flag value]...
commands:
  migrate
  status | posture
  bootstrap-admin --db-role R --issuer I --principal P          (owner member)
  bind --db-role R --issuer I --principal P | unbind --db-role R  (owner member)
  register-source-service --issuer I --principal P --source S   (owner member)
  grant|revoke --issuer I --principal P --capability C
  record-access-reapplied | confirm-retention-reapplied
  investigate [--filter JSON] [--page-size N] [--max-pages N]
  export --dir D [--operation export|verify|identity_chain] [--identity-chain]
         [--recovery] [--filter JSON] [--seq-after N] [--seq-through N]
         [--page-size N] [--max-pages N] [--checkpoint FILE]
  verify [--from N] [--to N] [--recovery] | verify-recovery
  checkpoint --out FILE
  export-identity-chain-recovery --dir D [--checkpoint FILE]
  set-retention --policy-id ID --selector JSON --retain-days N|none
  expire --policy-id ID --expected-revision N --cutoff YYYY-MM-DDTHH:MM:SS.ffffffZ --limit N
  purge-body --event-id UUID --reason-code CODE
  declare-recovery-pending --incident-code CODE
  begin-recovery-epoch [--checkpoint FILE] [--relay-max-seq N]
         (--preview | --expect-old-epoch N --expect-head-seq N
          --expect-head-chain HEX --expect-lost-upper N)";

/// Flags that take no value.
const SWITCHES: [&str; 3] = ["identity-chain", "recovery", "preview"];

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
            let value = if SWITCHES.contains(&name) {
                "true".to_owned()
            } else {
                raw.next()
                    .ok_or_else(|| CliError::Usage(format!("--{name} needs a value")))?
            };
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

    fn switch(&self, name: &str) -> bool {
        self.flags.contains_key(name)
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

    fn optional_number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, CliError> {
        self.optional(name)
            .map(|text| {
                text.parse()
                    .map_err(|_| CliError::Usage(format!("--{name} must be a number")))
            })
            .transpose()
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

/// Prints the manifest of a written export (its `chain_integrity` is
/// `intact` or `unanchored`). A chain that fails offline verification writes
/// no files: the verdict `broken` and the first offending line are printed
/// and the command fails.
fn exported(result: Result<ExportOutcome, FileError>) -> Result<(), CliError> {
    match result {
        Ok(outcome) => print(&outcome.manifest),
        Err(FileError::Verification(error)) => {
            print(&json!({
                "chain_integrity": audit_core::ChainIntegrity::Broken(error.clone()).as_str(),
                "error": error.to_string(),
            }))?;
            Err(CliError::Failed(format!(
                "export verification failed: {error}"
            )))
        }
        Err(other) => Err(other.into()),
    }
}

/// Connects after refusing a URL with `options`, then requires
/// `synchronous_commit = on` for the session.
async fn pool(url: Option<&SecretUrl>, name: &str) -> Result<PgPool, CliError> {
    let url = url.ok_or_else(|| CliError::Usage(format!("{name} is not set")))?;
    check_url(&url.0)?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url.0)
        .await
        .map_err(|error| CliError::Failed(redacted(&error)))?;
    require_synchronous_commit(&pool).await?;
    Ok(pool)
}

/// A driver error without its text (which could echo the URL).
fn redacted(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(db) => format!(
            "database error {}",
            db.code().map(|c| c.into_owned()).unwrap_or_default()
        ),
        _ => "cannot connect to the audit store".to_owned(),
    }
}

async fn operator(config: &Config) -> Result<AuditAdmin, CliError> {
    let pool = pool(config.store_url.as_ref(), "AUDIT_STORE_DATABASE_URL").await?;
    Ok(AuditAdmin::connect(pool).await?)
}

async fn owner(config: &Config) -> Result<AuditAdmin, CliError> {
    let pool = pool(config.store_url.as_ref(), "AUDIT_STORE_DATABASE_URL").await?;
    Ok(AuditAdmin::connect_owner(pool).await?)
}

fn operation(args: &Args) -> Result<AccessOperation, CliError> {
    if args.switch("identity-chain") {
        return match args.optional("operation") {
            None | Some("identity_chain") => Ok(AccessOperation::IdentityChain),
            Some(_) => Err(CliError::Usage(
                "--identity-chain conflicts with --operation".into(),
            )),
        };
    }
    match args.optional("operation").unwrap_or("export") {
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

/// The out-of-band recovery record the Store must confirm before an epoch
/// starts (`--expect-*`; take the values from `--preview`).
fn recovery_expectation(args: &Args) -> Result<RecoveryExpectation, CliError> {
    let chain = args.get("expect-head-chain")?;
    Ok(RecoveryExpectation {
        old_epoch: args.number("expect-old-epoch", None)?,
        restored_head_seq: args.number("expect-head-seq", None)?,
        restored_head_chain: audit_store_postgres::hex::decode32(chain).ok_or_else(|| {
            CliError::Usage("--expect-head-chain must be 64 lowercase hex digits".into())
        })?,
        lost_upper: args.number("expect-lost-upper", None)?,
    })
}

/// The export filter: `--filter` plus the `--seq-after` / `--seq-through`
/// shorthands.
fn export_filter(args: &Args) -> Result<Value, CliError> {
    let mut filter = args.json("filter", json!({}))?;
    let object = filter
        .as_object_mut()
        .ok_or_else(|| CliError::Usage("--filter must be a JSON object".into()))?;
    for (flag, key) in [("seq-after", "seq_after"), ("seq-through", "seq_through")] {
        if let Some(value) = args.optional_number::<i64>(flag)? {
            object.insert(key.to_owned(), json!(value));
        }
    }
    Ok(filter)
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
        "register-source-service" => print(
            &owner(&config)
                .await?
                .register_source_service(
                    args.get("issuer")?,
                    args.get("principal")?,
                    args.get("source")?,
                )
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
        "record-access-reapplied" => {
            print(&operator(&config).await?.record_access_reapplied().await?)
        }
        "confirm-retention-reapplied" => print(
            &operator(&config)
                .await?
                .confirm_retention_reapplied()
                .await?,
        ),
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
            let operation = operation(&args)?;
            let dir = PathBuf::from(args.get("dir")?);
            if args.switch("recovery") {
                if operation != AccessOperation::IdentityChain {
                    return Err(CliError::Usage(
                        "--recovery exports the identity chain only".into(),
                    ));
                }
                return exported(
                    export_identity_chain_recovery(&admin, checkpoint_arg(&args)?, &dir).await,
                );
            }
            let request = ExportRequest {
                operation,
                filter: export_filter(&args)?,
                page_size: args.number("page-size", Some(1000))?,
                max_pages: args.number("max-pages", Some(100))?,
                checkpoint: checkpoint_arg(&args)?,
            };
            exported(export_to_dir(&admin, &request, &dir).await)
        }
        "verify" if args.switch("recovery") => {
            print(&operator(&config).await?.verify_recovery().await?)
        }
        "verify" => {
            let from = args.optional_number("from")?;
            let to = args.optional_number("to")?;
            print(&operator(&config).await?.verify(from, to).await?)
        }
        "verify-recovery" => print(&operator(&config).await?.verify_recovery().await?),
        "checkpoint" => {
            let record = operator(&config).await?.checkpoint().await?;
            let file = write_checkpoint(&record, &PathBuf::from(args.get("out")?))?;
            print(&file)
        }
        "export-identity-chain-recovery" => {
            let admin = operator(&config).await?;
            exported(
                export_identity_chain_recovery(
                    &admin,
                    checkpoint_arg(&args)?,
                    &PathBuf::from(args.get("dir")?),
                )
                .await,
            )
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
        "declare-recovery-pending" => print(
            &operator(&config)
                .await?
                .declare_recovery_pending(args.get("incident-code")?)
                .await?,
        ),
        "begin-recovery-epoch" => {
            let checkpoint = checkpoint_arg(&args)?;
            let relay_max_seq = args.optional_number("relay-max-seq")?;
            let admin = operator(&config).await?;
            if args.switch("preview") {
                return print(
                    &admin
                        .preview_recovery_epoch(checkpoint.as_ref(), relay_max_seq)
                        .await?,
                );
            }
            print(
                &admin
                    .begin_recovery_epoch(
                        checkpoint.as_ref(),
                        relay_max_seq,
                        &recovery_expectation(&args)?,
                    )
                    .await?,
            )
        }
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
    fn args_parse_flags_switches_and_refuse_duplicates() {
        let args =
            Args::parse(["verify", "--from", "3"].into_iter().map(String::from)).expect("parses");
        assert_eq!(args.command, "verify");
        assert_eq!(args.number::<i64>("from", None).expect("number"), 3);
        let args = Args::parse(
            ["export", "--identity-chain", "--dir", "d", "--recovery"]
                .into_iter()
                .map(String::from),
        )
        .expect("switches");
        assert!(args.switch("identity-chain") && args.switch("recovery"));
        assert_eq!(args.get("dir").expect("dir"), "d");
        assert!(Args::parse(["x", "--a", "1", "--a", "2"].into_iter().map(String::from)).is_err());
        assert!(Args::parse(["x", "--a"].into_iter().map(String::from)).is_err());
        assert!(Args::parse(["x", "loose"].into_iter().map(String::from)).is_err());
    }

    #[test]
    fn seq_shorthands_extend_the_filter() {
        let args = Args::parse(
            ["export", "--seq-after", "5", "--seq-through", "9"]
                .into_iter()
                .map(String::from),
        )
        .expect("args");
        assert_eq!(
            export_filter(&args).expect("filter"),
            json!({"seq_after": 5, "seq_through": 9})
        );
    }
}
