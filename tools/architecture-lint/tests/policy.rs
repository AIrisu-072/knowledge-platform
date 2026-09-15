use architecture_lint::{check_repository, Config};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn valid() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let this = Self { dir };
        for path in [
            "spec/architecture/architecture-contract-v0.md",
            "spec/architecture/development-container-ci-architecture-v0.md",
            "spec/architecture/development-assurance-architecture-v0.md",
            "spec/operations/error-handling-resilience-requirements-v0.md",
            "spec/operations/observability-audit-requirements-v0.md",
            "spec/requirements/frontend-ux-requirements-v0.md",
            "spec/selection/library-tool-selection-v0.md",
            "mise.toml",
            "rust-toolchain.toml",
        ] {
            this.write(path, "fixture\n");
        }
        this
    }

    fn root(&self) -> &Path { self.dir.path() }

    fn write(&self, path: &str, content: &str) {
        let path = self.dir.path().join(path);
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).unwrap(); }
        fs::write(path, content).unwrap();
    }

    fn with_file(self, path: &str, content: &str) -> Self {
        self.write(path, content);
        self
    }
}

fn config() -> Config {
    Config::for_fixture()
}

#[test]
fn valid_bootstrap_repository_passes() {
    let fixture = Fixture::valid();
    let report = check_repository(fixture.root(), &config()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn self_hosted_runner_is_rejected() {
    let fixture = Fixture::valid().with_file(
        ".github/workflows/ci.yml",
        "permissions:\n  contents: read\njobs:\n  test:\n    runs-on: self-hosted\n    steps:\n      - run: mise run verify:fast\n",
    );
    let report = check_repository(fixture.root(), &config()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_CI_SELF_HOSTED"));
}

#[test]
fn native_windows_runner_is_rejected() {
    let fixture = Fixture::valid().with_file(
        ".github/workflows/ci.yml",
        "permissions:\n  contents: read\njobs:\n  test:\n    runs-on: windows-latest\n    steps:\n      - run: mise run verify:fast\n",
    );
    let report = check_repository(fixture.root(), &config()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_CI_NATIVE_WINDOWS"));
}

#[test]
fn dockerfile_variants_are_rejected() {
    let fixture = Fixture::valid().with_file("Dockerfile.prod", "FROM scratch\n");
    let report = check_repository(fixture.root(), &config()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_DOCKERFILE_VARIANT"));
}
