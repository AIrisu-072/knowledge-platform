use architecture_lint::{Config, check_repository};
use serde_json::json;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("architecture-lint: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => {}
        Some("--help" | "-h") | None => {
            println!("usage: architecture-lint check [--format human|json]");
            return Ok(ExitCode::SUCCESS);
        }
        Some(other) => return Err(format!("unknown command: {other}").into()),
    }

    let mut format = "human".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => format = args.next().ok_or("--format requires a value")?,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    let root = env::current_dir()?;
    let config = Config::load(&root)?;
    let report = check_repository(&root, &config)?;

    match format.as_str() {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": report.status(),
                "findings": report.findings,
            }))?
        ),
        "human" => {
            if report.findings.is_empty() {
                println!("architecture: pass");
            } else {
                for finding in &report.findings {
                    eprintln!("{} {}: {}", finding.code, finding.path, finding.message);
                }
            }
        }
        other => return Err(format!("unsupported format: {other}").into()),
    }

    Ok(if report.findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
