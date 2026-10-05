#!/usr/bin/env python3
"""Run the bounded L/LG PoC only when every local Cargo input is unchanged.

This hashes bytes but never copies Source content or any secret file. A changed input
invalidates the receipt instead of silently associating a run with Git HEAD.
"""

import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


POC = Path(__file__).resolve().parents[1]
REPO = POC.parents[1]
LOG = POC / "baseline-refinement-run.log"
MANIFEST = POC / "baseline-refinement-run-manifest.json"
IGNORED_DIRS = {".git", "target", "__pycache__", ".pytest_cache"}
IGNORED_POC_FILES = {
    "baseline-refinement-run-manifest.json",
    "baseline-refinement-report.md",
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def digest_package_tree(package: Path, poc_root: Path | None) -> dict:
    """Hash every local package file except generated receipts and secret env files."""
    package = package.resolve()
    files = {}
    for path in sorted(package.rglob("*")):
        relative = path.relative_to(package)
        if any(part in IGNORED_DIRS for part in relative.parts):
            continue
        if not path.is_file() or path.is_symlink() or path.name.startswith(".env"):
            continue
        if poc_root is not None and package == poc_root:
            if path.suffix == ".log" or path.name in IGNORED_POC_FILES:
                continue
        files[relative.as_posix()] = sha256(path.read_bytes())
    if "Cargo.toml" not in files:
        raise RuntimeError(f"local package has no Cargo.toml: {package}")
    canonical = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {"tree_sha256": sha256(canonical), "file_count": len(files), "files": files}


def command_output(argv, env):
    result = subprocess.run(argv, cwd=REPO, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {' '.join(argv)}\n"
                           + result.stdout.decode(errors="replace")[-3000:])
    return result.stdout.decode(errors="replace")


def snapshot_inputs(env):
    metadata = json.loads(command_output([
        "cargo", "metadata", "--manifest-path", str(POC / "Cargo.toml"),
        "--offline", "--locked", "--format-version", "1",
    ], env))
    package_by_id = {item["id"]: item for item in metadata["packages"]}
    node_by_id = {item["id"]: item for item in metadata["resolve"]["nodes"]}
    pending = [metadata["resolve"]["root"]]
    reached = set()
    while pending:
        package_id = pending.pop()
        if package_id in reached:
            continue
        reached.add(package_id)
        pending.extend(dep["pkg"] for dep in node_by_id[package_id]["deps"])
    packages = {}
    for package_id in sorted(reached):
        package = package_by_id[package_id]
        if package["source"] is not None:
            continue
        directory = Path(package["manifest_path"]).resolve().parent
        try:
            name = directory.relative_to(REPO).as_posix()
        except ValueError:
            name = str(directory)
        packages[name] = {
            "name": package["name"],
            "version": package["version"],
            **digest_package_tree(directory, POC),
        }
    patched_tantivy = "third_party/search/tantivy-0.26.2"
    if patched_tantivy not in packages or packages[patched_tantivy]["name"] != "tantivy":
        raise RuntimeError("expected patched Tantivy is absent from actual Cargo resolve graph")
    for required in ["experiments/search-vector-poc", "crates/search-core",
                     "crates/search-application", "crates/search-graph-memory",
                     "crates/search-tantivy"]:
        if required not in packages:
            raise RuntimeError(f"required actual path package absent: {required}")
    host_inputs = {}
    # Path packages inherit edition, rust-version, and dependency declarations here.
    workspace_manifest = REPO / "Cargo.toml"
    host_inputs[str(workspace_manifest)] = sha256(workspace_manifest.read_bytes())
    for directory in [REPO, *POC.parents[:1], POC]:
        for name in [".cargo/config", ".cargo/config.toml", "rust-toolchain", "rust-toolchain.toml"]:
            path = directory / name
            if path.is_file() and not path.name.startswith(".env"):
                host_inputs[str(path)] = sha256(path.read_bytes())
    identity = {
        "packages": packages,
        "host_inputs": host_inputs,
        "cargo_version": command_output(["cargo", "--version"], env).strip(),
        "rustc_verbose": command_output(["rustc", "-vV"], env).strip(),
    }
    identity["input_sha256"] = sha256(json.dumps(identity, sort_keys=True,
                                                  separators=(",", ":")).encode())
    return identity


def main():
    env = os.environ.copy()
    forced = {
        "CARGO_TARGET_DIR": str(REPO / "target"),
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0",
        "CARGO_BUILD_JOBS": "2",
    }
    env.update(forced)
    # A caller-provided override could change compilation without changing source files.
    for variable in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
        env.pop(variable, None)
    commands = [
        ("focused_tests", ["cargo", "test", "--manifest-path", str(POC / "Cargo.toml"),
                           "--offline", "--locked", "--tests"]),
        ("baseline", ["cargo", "run", "--manifest-path", str(POC / "Cargo.toml"),
                      "--offline", "--locked", "--quiet"]),
        ("focused_clippy", ["cargo", "clippy", "--manifest-path", str(POC / "Cargo.toml"),
                            "--offline", "--locked", "-p", "search-vector-poc", "--tests",
                            "--", "-D", "warnings"]),
    ]
    manifest = {
        "schema": "p2-l-lg-pinned-inputs-v1",
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "status": "REJECTED",
        "git_head": command_output(["git", "rev-parse", "HEAD"], env).strip(),
        "git_branch": command_output(["git", "branch", "--show-current"], env).strip(),
        "forced_environment": forced,
        "unset_environment": ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"],
        "phases": [],
    }
    first = None
    try:
        with LOG.open("w", encoding="utf-8") as log:
            for name, argv in commands:
                before = snapshot_inputs(env)
                if first is None:
                    first = before
                if before != first:
                    raise RuntimeError(f"input snapshot changed before {name}")
                start = time.monotonic()
                result = subprocess.run(argv, cwd=REPO, env=env, stdout=subprocess.PIPE,
                                        stderr=subprocess.STDOUT, check=False)
                log.write(f"=== {name}: {' '.join(argv)} ===\n")
                log.write(result.stdout.decode(errors="replace"))
                log.flush()
                after = snapshot_inputs(env)
                phase = {
                    "name": name,
                    "command": argv,
                    "exit_code": result.returncode,
                    "seconds": round(time.monotonic() - start, 3),
                    "before_sha256": before["input_sha256"],
                    "after_sha256": after["input_sha256"],
                }
                manifest["phases"].append(phase)
                if before != after:
                    raise RuntimeError(f"input snapshot changed during {name}")
                if result.returncode != 0:
                    raise RuntimeError(f"{name} failed with exit code {result.returncode}")
            manifest["status"] = "ACCEPTED"
        manifest["input_snapshot"] = first
    except Exception as error:
        manifest["error"] = str(error)
    manifest["finished_utc"] = datetime.now(timezone.utc).isoformat()
    manifest["log_sha256"] = sha256(LOG.read_bytes()) if LOG.exists() else None
    MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
                        encoding="utf-8")
    print(f"status={manifest['status']} input_sha256={first['input_sha256'] if first else 'UNAVAILABLE'} "
          f"log_sha256={manifest['log_sha256']} manifest={MANIFEST}")
    if manifest["status"] != "ACCEPTED":
        print(manifest.get("error", "unknown failure"), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
