use assurance_cli::capability;
use assurance_cli::evidence::{self, Outcome};
use assurance_cli::graph::Graph;
use assurance_cli::planner::{self, Plan};
use assurance_cli::provider::{CommandProvider, Provider};
use assurance_cli::requirement;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("assure: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let root = env::current_dir()?;
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());

    match command.as_str() {
        "scan" => {
            let requirements = requirement::scan(&root)?;
            let graph = Graph::scan(&root)?;
            println!(
                "requirements={} nodes={} edges={}",
                requirements.len(),
                graph.node_count(),
                graph.edge_count()
            );
            Ok(ExitCode::SUCCESS)
        }
        "plan" => {
            let requirements = requirement::scan(&root)?;
            let capabilities = capability::load(&root)?;
            let graph = Graph::scan(&root)?;
            let mode = args.next().ok_or("plan requires --all or --changed-from <sha>")?;
            let plan = if mode == "--all" {
                planner::plan_all(&requirements, &capabilities)
            } else if mode == "--changed-from" {
                let sha = args.next().ok_or("--changed-from requires a git SHA")?;
                planner::plan(&graph, &requirements, &capabilities, &changed_paths(&root, &sha)?)
            } else {
                return Err(format!("unknown plan mode: {mode}").into());
            };
            print_plan(&plan)?;
            Ok(if plan.gaps.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "run" => {
            if args.next().as_deref() != Some("--all") {
                return Err("run requires --all".into());
            }
            let provider = CommandProvider::new(&root, git_head(&root)?);
            let capabilities = capability::load(&root)?;
            let mut failed = false;
            for item in capabilities.iter().filter(|item| item.provider == provider.id()) {
                let result = provider.run(item)?;
                println!(
                    "{} outcome={:?} duration_ms={}",
                    result.capability_id, result.outcome, result.duration_ms
                );
                failed |= result.outcome != Outcome::Pass;
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        "report" => {
            let evidence = evidence::read_all(&root)?;
            let failed = evidence
                .iter()
                .filter(|item| item.outcome != Outcome::Pass)
                .count();
            println!(
                "evidence={} failed={} status={}",
                evidence.len(),
                failed,
                if evidence.is_empty() {
                    "missing"
                } else if failed == 0 {
                    "pass"
                } else {
                    "fail"
                }
            );
            Ok(if !evidence.is_empty() && failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "help" | "--help" | "-h" => {
            println!("usage: assure <scan|plan --all|plan --changed-from SHA|run --all|report>");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command: {other}").into()),
    }
}

fn changed_paths(root: &Path, sha: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let range = format!("{sha}..HEAD");
    let output = Command::new("git")
        .args(["diff", "--name-only", &range])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(PathBuf::from)
        .collect())
}

fn git_head(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn print_plan(plan: &Plan) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(plan)?);
    Ok(())
}
