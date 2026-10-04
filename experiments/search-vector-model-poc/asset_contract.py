"""Offline P2-01 pin checks. This module does not load or run model weights."""

from __future__ import annotations

from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import re
import math
import subprocess
import sys
import tomllib


HEX_256 = re.compile(r"[0-9a-f]{64}\Z")
HEX_160 = re.compile(r"[0-9a-f]{40}\Z")
DOC_ID = re.compile(r"[0-9]+#[0-9]+\Z")
ARM_ROUTES = {
    "L": ["Lexical"],
    "LG": ["Lexical", "HyperGraph"],
    "D": ["Vector"],
    "LD": ["Lexical", "Vector"],
    "LDG": ["Lexical", "Vector", "HyperGraph"],
}
CODE_DIGESTS = {
    "baseline_harness_sha256", "path_crates_sha256", "planner_executor_sha256",
    "model_adapter_sha256", "runtime_build_sha256", "index_engine_sha256",
    "isolated_lock_sha256",
}
CODE_CATEGORIES = {name.removesuffix("_sha256") for name in CODE_DIGESTS}
DATA_DIGESTS = {
    "corpus_text_sha256", "query_text_sha256", "label_split_sha256",
    "source_snapshot_sha256", "current_read_sha256", "filter_sha256",
    "eligible_parent_set_sha256", "unit_binding_sha256", "judgments_sha256",
}
DATA_SCHEMAS = {
    "corpus_text_sha256": "p2-corpus-rows-v1",
    "query_text_sha256": "p2-query-rows-v1",
    "label_split_sha256": "p2-label-split-v1",
    "source_snapshot_sha256": "p2-source-snapshot-v1",
    "current_read_sha256": "p2-current-read-v1",
    "filter_sha256": "p2-filter-v1",
    "eligible_parent_set_sha256": "p2-eligible-qrels-v1",
    "unit_binding_sha256": "p2-unit-bindings-v1",
    "judgments_sha256": "p2-judgments-v1",
}
RUN_INPUT_KEYS = {
    "schema", "state", "assets_manifest_sha256", "public_slice_manifest_sha256",
    "execution", "execution_guard", "measurement", "parity", "pre_run_inputs",
    "public_japanese", "resource_ceiling", "resource_scenarios", "synthetic_fixture",
}


class ContractError(ValueError):
    """An unpinned, changed, or incomplete input cannot be used."""


def load_json(path: Path) -> dict:
    with path.open(encoding="utf-8") as stream:
        return json.load(stream)


def _required(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def _digest(value: object, name: str) -> None:
    _required(isinstance(value, str) and HEX_256.fullmatch(value) is not None, f"{name}: missing SHA-256")


def _revision(value: object, name: str) -> None:
    _required(isinstance(value, str) and HEX_160.fullmatch(value) is not None, f"{name}: mutable or missing revision")


def _exact_keys(value: object, expected: set[str], name: str) -> None:
    _required(isinstance(value, dict) and set(value) == expected, f"{name}: missing or unsupported fields")


def _utc(value: object, name: str) -> datetime:
    _required(isinstance(value, str), f"{name}: missing UTC timestamp")
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError as error:
        raise ContractError(f"{name}: invalid timestamp") from error
    _required(parsed.utcoffset() is not None, f"{name}: timezone required")
    return parsed


def _canonical_sha(value: object, domain: bytes) -> str:
    encoded = json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()
    return sha256(domain + b"\0" + encoded).hexdigest()


def _source_set_digest(files: dict[str, str]) -> str:
    records = "".join(f"{path}\0{files[path]}\n" for path in sorted(files))
    return sha256(records.encode()).hexdigest()


def _verify_source_files(source_files: dict, workspace_root: Path) -> None:
    """Rehash every declared repo or owned PoC build file before sealing."""
    root = workspace_root.resolve()
    for category, files in source_files.items():
        for relative, expected in files.items():
            path = _safe_file(root, relative)
            _required(path.is_file(), f"{category}: missing code/build file {relative}")
            actual = sha256()
            with path.open("rb") as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    actual.update(chunk)
            _required(actual.hexdigest() == expected, f"{category}: code/build bytes changed {relative}")


def _toml(path: Path) -> dict:
    try:
        with path.open("rb") as stream:
            return tomllib.load(stream)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ContractError(f"invalid or missing Cargo manifest: {path}") from error


def _workspace_manifest(manifest: Path, root: Path) -> tuple[Path, dict] | None:
    for parent in (manifest.parent, *manifest.parent.parents):
        if not parent.is_relative_to(root):
            break
        candidate = parent / "Cargo.toml"
        if candidate.is_file():
            parsed = _toml(candidate)
            if "workspace" in parsed:
                return candidate, parsed
    return None


def derive_source_inventory(workspace_root: Path) -> dict[str, str]:
    """Conservatively hash full trees of both PoCs and every local Cargo dependency.

    This deliberately does not infer a crate from seven caller-supplied categories.
    Cargo target directories, downloaded model assets, and the mutable run/receipt
    documents are excluded; model bytes are separately verified by verify_assets.
    """
    root = workspace_root.resolve()
    entrypoints = [root / "experiments/search-vector-model-poc/Cargo.toml",
                   root / "experiments/search-vector-poc/Cargo.toml"]
    for manifest in entrypoints:
        _required(manifest.is_file(), f"Rust scaffold/baseline Cargo manifest missing: {manifest}")
        _required((manifest.parent / "Cargo.lock").is_file(), f"Cargo.lock missing: {manifest.parent}")
    pending = entrypoints[:]
    seen: set[Path] = set()
    crate_dirs: set[Path] = set()
    ancillary: set[Path] = set()
    while pending:
        manifest = pending.pop()
        _required(manifest.is_relative_to(root) and manifest.name == "Cargo.toml" and
                  manifest.is_file() and not manifest.is_symlink(),
                  f"missing or escaping local Cargo dependency: {manifest}")
        if manifest in seen:
            continue
        seen.add(manifest)
        crate_dirs.add(manifest.parent)
        parsed = _toml(manifest)
        workspace = _workspace_manifest(manifest, root)
        if workspace is not None:
            ancillary.add(workspace[0])
            lock = workspace[0].parent / "Cargo.lock"
            _required(lock.is_file(), f"workspace Cargo.lock missing: {lock}")
            ancillary.add(lock)
        if (manifest.parent / "Cargo.lock").is_file():
            ancillary.add(manifest.parent / "Cargo.lock")
        # Search dependency, target, build and patch tables. Optional local path
        # dependencies are included because selected features may enable them.
        def visit(node: object, key: str | None = None, base: Path = manifest.parent) -> None:
            if isinstance(node, dict):
                if isinstance(node.get("path"), str):
                    raw_target = Path(os.path.abspath(base / node["path"]))
                    _required(not any(part.is_symlink() for part in (raw_target, *raw_target.parents)
                                      if part.is_relative_to(root)),
                              f"symlinked local Cargo path: {raw_target}")
                    target = raw_target.resolve()
                    _required(target.is_relative_to(root), f"local Cargo path escaped workspace: {target}")
                    pending.append(target / "Cargo.toml")
                elif node.get("workspace") is True and key is not None:
                    _required(workspace is not None, f"workspace dependency without workspace: {key}")
                    dependency = workspace[1].get("workspace", {}).get("dependencies", {}).get(key)
                    _required(dependency is not None, f"unresolved workspace dependency: {key}")
                    visit(dependency, key, workspace[0].parent)
                for child_key, child in node.items():
                    if child_key not in ("path", "workspace"):
                        visit(child, child_key, base)
            elif isinstance(node, list):
                for child in node:
                    visit(child, key, base)
        visit(parsed)
        if workspace is not None and workspace[0] != manifest:
            visit(workspace[1].get("patch", {}), base=workspace[0].parent)
    for crate in crate_dirs:
        for parent in (crate, *crate.parents):
            if not parent.is_relative_to(root):
                break
            for name in ("rust-toolchain", "rust-toolchain.toml"):
                candidate = parent / name
                if candidate.exists():
                    ancillary.add(candidate)
            for name in ("config", "config.toml"):
                candidate = parent / ".cargo" / name
                if candidate.exists():
                    ancillary.add(candidate)
    inventory: dict[str, str] = {}
    rust_sources: list[Path] = []
    for crate in sorted(crate_dirs):
        for directory, subdirs, filenames in os.walk(crate, followlinks=False):
            location = Path(directory)
            for name in subdirs[:]:
                child = location / name
                _required(not child.is_symlink(), f"symlink in Cargo source tree: {child}")
                if name in ("target", ".git", "__pycache__") or (
                    crate == entrypoints[0].parent and name in ("assets", "metadata")
                ):
                    subdirs.remove(name)
            for name in filenames:
                path = location / name
                _required(not path.is_symlink(), f"symlink in Cargo source tree: {path}")
                _required(path.is_file(), f"nonregular Cargo source input: {path}")
                if crate == entrypoints[0].parent and name in (
                    "run-manifest.json", "assets-manifest.json", "public-ja-slice.json",
                    "protocol-receipt.md", "protocol-fix-receipt.md", "protocol-hardening-receipt.md",
                ):
                    continue
                inventory[path.relative_to(root).as_posix()] = sha256(path.read_bytes()).hexdigest()
                if path.suffix == ".rs":
                    rust_sources.append(path)
    include_macro = re.compile(r"include(?:_str|_bytes)?!\s*\(")
    literal = re.compile(r'\s*"([^"\n]+)"\s*\)', re.S)
    scanned: set[Path] = set()
    while rust_sources:
        source = rust_sources.pop()
        if source in scanned:
            continue
        scanned.add(source)
        try:
            source_text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            raise ContractError(f"unreadable Rust source include owner: {source}") from error
        for macro in include_macro.finditer(source_text):
            match = literal.match(source_text, macro.end())
            _required(match is not None, f"dynamic Rust include cannot be pinned statically: {source}")
            raw_include = Path(os.path.abspath(source.parent / match.group(1)))
            _required(raw_include.is_relative_to(root) and
                      not any(part.is_symlink() for part in (raw_include, *raw_include.parents)
                              if part.is_relative_to(root)) and raw_include.is_file(),
                      f"missing, escaping or symlinked local Rust include: {raw_include}")
            relative = raw_include.relative_to(root).as_posix()
            inventory[relative] = sha256(raw_include.read_bytes()).hexdigest()
            if raw_include.suffix == ".rs":
                rust_sources.append(raw_include)
    for path in ancillary:
        _required(not path.is_symlink() and path.is_file() and path.is_relative_to(root),
                  f"missing, escaping or symlinked Cargo build input: {path}")
        inventory[path.relative_to(root).as_posix()] = sha256(path.read_bytes()).hexdigest()
    return dict(sorted(inventory.items()))


def capture_source_pins(workspace_root: Path) -> tuple[dict[str, dict[str, str]], dict[str, str]]:
    """Derive the exact seven code categories and their digests from real paths."""
    inventory = derive_source_inventory(workspace_root)
    categories: dict[str, dict[str, str]] = {name: {} for name in CODE_CATEGORIES}
    for path, digest in inventory.items():
        assigned = False
        if path.startswith("experiments/search-vector-poc/"):
            categories["baseline_harness"][path] = digest
            assigned = True
        if path.startswith("experiments/search-vector-model-poc/"):
            categories["model_adapter"][path] = digest
            assigned = True
        if path.startswith(("crates/", "third_party/")):
            categories["path_crates"][path] = digest
            assigned = True
        if path.startswith(("third_party/", "crates/search-tantivy/")):
            categories["index_engine"][path] = digest
        if path.startswith(("experiments/search-vector-poc/src/", "crates/search-application/",
                            "crates/search-core/", "crates/search-graph-memory/")):
            categories["planner_executor"][path] = digest
        if path.endswith(("Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml")) or \
                "/.cargo/config" in path:
            categories["runtime_build"][path] = digest
            assigned = True
        if path.endswith("Cargo.lock"):
            categories["isolated_lock"][path] = digest
        if not assigned:
            # A literal include may live outside its crate tree, but still
            # inside the workspace. Keep it in the build closure.
            categories["runtime_build"][path] = digest
    _required(all(categories.values()), "derived Cargo closure has an empty required code category")
    return categories, {name + "_sha256": _source_set_digest(files) for name, files in categories.items()}


def _verify_complete_source_closure(source_files: dict, workspace_root: Path) -> None:
    derived, _ = capture_source_pins(workspace_root)
    _required(source_files == derived, "reachable Cargo/source closure has missing, extra or changed files")


BUILD_ENV_KEYS = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET",
                  "CARGO_TARGET_DIR", "CARGO_HOME", "RUSTUP_TOOLCHAIN", "RUSTC_WRAPPER",
                  "RUSTUP_HOME", "RUSTC", "RUSTC_WORKSPACE_WRAPPER", "RUSTDOCFLAGS",
                  "CARGO_BUILD_JOBS", "CARGO_INCREMENTAL", "CARGO_NET_OFFLINE",
                  "CC", "CXX", "CFLAGS", "LDFLAGS", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET")


def _observed_build_env() -> dict[str, str]:
    return {key: value for key, value in os.environ.items()
            if key in BUILD_ENV_KEYS or key.startswith("CARGO_PROFILE_")}


def _verify_build_identity(identity: dict, source_files: dict, assets: dict, candidate_id: str,
                           workspace_root: Path) -> None:
    _exact_keys(identity, {"schema", "rustc_path", "rustc_sha256", "rustc_verbose",
                           "cargo_path", "cargo_sha256", "target_triple", "features_by_package",
                           "build_env", "native_files", "arm_executables",
                           "baseline_export_executable"}, "build identity")
    _required(identity["schema"] == "p2-build-identity-v1", "build identity schema mismatch")
    for name in ("rustc", "cargo"):
        path = Path(identity[f"{name}_path"])
        _digest(identity[f"{name}_sha256"], f"{name} executable")
        _required(path.is_absolute() and path.is_file() and not path.is_symlink() and
                  sha256(path.read_bytes()).hexdigest() == identity[f"{name}_sha256"],
                  f"{name} executable bytes missing or changed")
    try:
        actual_rustc = subprocess.run([identity["rustc_path"], "-vV"], capture_output=True,
                                      text=True, check=True, timeout=10).stdout.strip()
    except (OSError, subprocess.SubprocessError) as error:
        raise ContractError("rustc compiler identity unavailable") from error
    _required(identity["rustc_verbose"] == actual_rustc, "rustc version/host differs from executable")
    _required(isinstance(identity["target_triple"], str) and identity["target_triple"] and
              (not os.environ.get("CARGO_BUILD_TARGET") or
               identity["target_triple"] == os.environ["CARGO_BUILD_TARGET"]),
              "target triple differs from build environment")
    observed_env = _observed_build_env()
    _required(identity["build_env"] == observed_env, "Cargo/compiler build environment changed")
    cargo_home = observed_env.get("CARGO_HOME")
    _required(isinstance(cargo_home, str) and Path(cargo_home).is_dir(),
              "isolated CARGO_HOME required for complete build configuration")
    _required(not any((Path(cargo_home) / name).exists() for name in ("config", "config.toml")),
              "external Cargo home config is outside frozen workspace closure")
    _required(not any((parent / ".cargo" / name).exists()
                      for parent in workspace_root.parents for name in ("config", "config.toml")),
              "ancestor Cargo config is outside frozen workspace closure")
    _required(observed_env.get("CARGO_NET_OFFLINE") == "true", "Cargo offline build guard missing")
    if "RUSTC" in observed_env:
        _required(Path(observed_env["RUSTC"]).resolve() == Path(identity["rustc_path"]).resolve(),
                  "RUSTC override differs from pinned compiler")
    packages = {}
    for files in source_files.values():
        for relative in files:
            if relative.endswith("/Cargo.toml"):
                manifest = _toml(workspace_root / relative)
                name = manifest.get("package", {}).get("name")
                if name:
                    packages[name] = set(manifest.get("features", {}))
    features = identity["features_by_package"]
    _required(isinstance(features, dict) and set(features) == set(packages),
              "feature set missing reachable Cargo package")
    for name, selected in features.items():
        _required(isinstance(selected, list) and len(selected) == len(set(selected)) and
                  set(selected) <= packages[name], f"unknown or duplicate feature for {name}")
    candidate = next(item for item in assets["candidates"] if item["id"] == candidate_id)
    natives = identity["native_files"]
    _required(isinstance(natives, list), "native binary inventory missing")
    if candidate["runtime_id"] == "ort_cpu":
        native = assets["runtimes"]["ort_cpu"]["native_archive"]
        _required(len(natives) == 1 and natives[0] == {
            "local_path": native["extracted_library_path"],
            "bytes": native["extracted_library_bytes"],
            "sha256": native["extracted_library_sha256"],
        }, "native runtime binary differs from pinned extracted library")
    else:
        _required(not natives, "unexpected native runtime binary")
    for entry in natives:
        _check_bytes(workspace_root / "experiments/search-vector-model-poc", entry)
    _exact_keys(identity["arm_executables"], set(ARM_ROUTES), "arm executable inventory")
    for arm, entry in identity["arm_executables"].items():
        _exact_keys(entry, {"local_path", "sha256", "bytes"}, f"{arm} executable")
        _check_bytes(workspace_root, entry)
    baseline_export = identity["baseline_export_executable"]
    _exact_keys(baseline_export, {"local_path", "sha256", "bytes"}, "baseline export executable")
    _required(baseline_export == identity["arm_executables"]["L"] ==
              identity["arm_executables"]["LG"],
              "baseline export and L/LG must use the same executable bytes")
    _check_bytes(workspace_root, baseline_export)


def _available_memory_bytes() -> tuple[int, str]:
    if sys.platform == "darwin":
        try:
            output = subprocess.run(["vm_stat"], capture_output=True, text=True,
                                    check=True, timeout=10).stdout
        except (OSError, subprocess.SubprocessError) as error:
            raise ContractError("actual available RAM unavailable") from error
        page_size = int(re.search(r"page size of (\d+) bytes", output).group(1))
        counts = {match.group(1): int(match.group(2)) for match in
                  re.finditer(r"^Pages (free|inactive|speculative):\s+([0-9]+)\.", output, re.M)}
        _required("free" in counts and "inactive" in counts, "vm_stat free/inactive unavailable")
        # Conservative reclaimable estimate; excludes active/wired/compressed pages.
        return page_size * (counts["free"] + counts["inactive"]), "vm_stat free+inactive"
    if sys.platform.startswith("linux"):
        try:
            text = Path("/proc/meminfo").read_text()
        except OSError as error:
            raise ContractError("actual available RAM unavailable") from error
        match = re.search(r"^MemAvailable:\s+(\d+) kB$", text, re.M)
        _required(match is not None, "MemAvailable unavailable")
        return int(match.group(1)) * 1024, "/proc/meminfo MemAvailable"
    raise ContractError("available RAM probe unsupported on this host")


def capture_capacity_receipt(run: dict, local_root: Path) -> dict:
    """Read OS capacity now. Call again immediately before each acquire/build."""
    scenario_id = run["pre_run_inputs"]["resource_scenario_id"]
    scenario = run["resource_scenarios"][scenario_id]
    stats = os.statvfs(local_root)
    free_disk = stats.f_bavail * stats.f_frsize
    available, source = _available_memory_bytes()
    return {
        "schema": "p2-capacity-admission-v1",
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "scenario_id": scenario_id,
        "scenario_sha256": _canonical_sha(scenario, b"p2-capacity-projection-v1"),
        "filesystem_device": os.stat(local_root).st_dev,
        "free_disk_bytes": free_disk,
        "available_memory_bytes": available,
        "memory_source": source,
        "verdict": "ADMITTED" if (free_disk >= scenario["required_free_with_reserve_bytes"] and
                                  available >= run["resource_ceiling"]["per_process_rss_max_bytes"]) else
                   "ENVIRONMENT_UNAVAILABLE",
    }


def _verify_capacity_admission(run: dict, local_root: Path, receipt: dict | None = None,
                               *, initial: bool = True) -> None:
    pinned = run["pre_run_inputs"]["capacity_admission"] if receipt is None else receipt
    _exact_keys(pinned, {"schema", "captured_at_utc", "scenario_id", "scenario_sha256",
                         "filesystem_device", "free_disk_bytes", "available_memory_bytes",
                         "memory_source", "verdict"}, "capacity admission")
    _required(pinned["schema"] == "p2-capacity-admission-v1", "capacity receipt schema mismatch")
    moment = _utc(pinned["captured_at_utc"], "capacity admission")
    age = (datetime.now(timezone.utc) - moment).total_seconds()
    _required(0 <= age <= 60, "capacity observation stale or future")
    scenario_id = run["pre_run_inputs"]["resource_scenario_id"]
    scenario = run["resource_scenarios"][scenario_id]
    _required(pinned["scenario_id"] == scenario_id and
              pinned["scenario_sha256"] == _canonical_sha(scenario, b"p2-capacity-projection-v1"),
              "capacity receipt projection differs from selected immutable scenario")
    _required(pinned["filesystem_device"] == os.stat(local_root).st_dev, "capacity filesystem changed")
    live = capture_capacity_receipt(run, local_root)
    _required(pinned["verdict"] == "ADMITTED" and live["verdict"] == "ADMITTED" and
              pinned["free_disk_bytes"] <= live["free_disk_bytes"] and
              pinned["available_memory_bytes"] <= live["available_memory_bytes"],
              "current disk/available RAM does not support admission")
    if initial:
        _required(pinned["free_disk_bytes"] == run["resource_ceiling"]["current_free_bytes"] and
                  pinned["available_memory_bytes"] == run["pre_run_inputs"]["host_hardware"]["free_memory_bytes_at_admission"] and
                  pinned["free_disk_bytes"] == run["execution"]["observed_free_disk_kib"] * 1024,
                  "capacity fields differ from measured OS receipt")


def verify_capacity_before_action(run: dict, assets: dict, public: dict, local_root: Path,
                                  receipt: dict, action: str, *, evaluation_lane: str = "synthetic") -> None:
    _required(action in ("acquire", "build"), "unsupported capacity action")
    validate_assets(assets)
    validate_public_slice(public)
    _validate_run_inputs(run, assets, public, evaluation_lane)
    _verify_capacity_admission(run, local_root, receipt, initial=False)


def _asset(entry: dict, name: str) -> None:
    _required(isinstance(entry, dict), f"{name}: missing asset declaration")
    _digest(entry.get("sha256"), name)
    _required(type(entry.get("bytes")) is int and entry["bytes"] > 0, f"{name}: missing byte length")
    for field in ("remote_path", "local_path"):
        path = entry.get(field)
        _required(isinstance(path, str) and path and not Path(path).is_absolute(), f"{name}: invalid {field}")
        _required(".." not in Path(path).parts and "\\" not in path, f"{name}: unsafe {field}")


def validate_assets(manifest: dict) -> None:
    _required(manifest.get("schema") == "p2-model-assets-v1", "unsupported asset schema")
    models = manifest.get("models")
    _required(isinstance(models, list) and len(models) == 2, "exactly two real model candidates required")
    by_id = {}
    for model in models:
        ident = model.get("id")
        _required(ident in ("e5", "minilm") and ident not in by_id, "duplicate or unknown model")
        by_id[ident] = model
        _revision(model.get("revision"), f"{ident} revision")
        _required(model.get("license") in ("mit", "apache-2.0"), f"{ident}: missing model license")
        _required(model["revision"] in model.get("license_source_url", ""), f"{ident}: unpinned license source")
        metadata = model.get("metadata", {})
        for name in ("config.json", "tokenizer_config.json", "special_tokens_map.json", "sentence_bert_config.json", "modules.json", "1_Pooling/config.json"):
            _asset(metadata.get(name), f"{ident} metadata {name}")
            _required(metadata[name]["remote_path"] == name, f"{ident}: metadata path mismatch")
        artifacts = model.get("artifacts", {})
        for name, remote in (("safetensors", "model.safetensors"), ("onnx", "onnx/model.onnx"), ("tokenizer_json", "tokenizer.json"), ("sentencepiece", "sentencepiece.bpe.model")):
            _asset(artifacts.get(name), f"{ident} {name}")
            _required(artifacts[name]["remote_path"] == remote, f"{ident}: artifact path mismatch")
        embed = model.get("embedding", {})
        _required(embed.get("architecture") == "BertModel" and embed.get("model_type") == "bert", f"{ident}: unverified architecture")
        _required(embed.get("hidden_size") == 384 and embed.get("layers") == 12, f"{ident}: dimension or layer mismatch")
        _required(embed.get("tokenizer_class") == ("XLMRobertaTokenizer" if ident == "e5" else "PreTrainedTokenizerFast"), f"{ident}: tokenizer class mismatch")
        _required(embed.get("max_seq_length") == (512 if ident == "e5" else 128), f"{ident}: token limit mismatch")
        _required(embed.get("precision") == "f32" and embed.get("metric") == "cosine", f"{ident}: precision/metric mismatch")
        _required(embed.get("pooling") == "attention_masked_mean", f"{ident}: pooling mismatch")
        _required(embed.get("query_prefix") == ("query: " if ident == "e5" else ""), f"{ident}: query prefix mismatch")
        _required(embed.get("passage_prefix") == ("passage: " if ident == "e5" else ""), f"{ident}: passage prefix mismatch")
    _required(set(by_id) == {"e5", "minilm"}, "both model families required")

    runtimes = manifest.get("runtimes", {})
    _required(set(runtimes) == {"candle_cpu", "ort_cpu"}, "both pinned CPU runtime plans required")
    candle = runtimes["candle_cpu"]
    _required(candle.get("version") == "0.11.0" and candle.get("default_features") is False, "Candle version/features not pinned")
    _required(candle.get("features") == [] and candle.get("license"), "Candle CPU features/license missing")
    _required(set(candle.get("crates", {})) == {"candle-core", "candle-nn", "candle-transformers", "tokenizers"}, "Candle crate set incomplete")
    ort = runtimes["ort_cpu"]
    _required(ort.get("version") == "2.0.0-rc.13" and ort.get("default_features") is False, "ort version/features not pinned")
    _required("load-dynamic" in ort.get("features", []) and "api-28" in ort["features"], "ort dynamic CPU path missing")
    _required("download-binaries" in ort.get("disabled_features", []) and ort.get("license"), "ort download/license guard missing")
    _required(set(ort.get("crates", {})) == {"ort", "ort-sys"}, "ort crate set incomplete")
    for runtime_name, runtime in runtimes.items():
        for crate_name, crate in runtime["crates"].items():
            _required(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[0-9]+)?", crate.get("version", "")) is not None, f"{crate_name}: unpinned version")
            _digest(crate.get("crate_archive_sha256"), f"{crate_name} archive")
    native = ort.get("native_archive", {})
    _digest(native.get("sha256"), "ONNX Runtime archive")
    _required(native.get("bytes", 0) > 0 and native.get("license") == "MIT", "ONNX Runtime artifact license/size missing")
    _required("v1.28.0" in native.get("source_url", "") and "osx-arm64" in native.get("name", ""), "ONNX Runtime native artifact unpinned")

    candidates = manifest.get("candidates")
    _required(isinstance(candidates, list) and len(candidates) == 4 and {c.get("id") for c in candidates} == {"e5-candle-f32", "e5-ort-f32", "minilm-candle-f32", "minilm-ort-f32"}, "candidate set incomplete")
    for candidate in candidates:
        ident = candidate["id"]
        _required(candidate.get("model_id") in by_id, f"{ident}: model missing")
        _required(candidate.get("runtime_id") in runtimes, f"{ident}: runtime missing")
        _required(ident.startswith(candidate["model_id"] + "-"), f"{ident}: candidate/model binding mismatch")
        _required(("-candle-" in ident) == (candidate["runtime_id"] == "candle_cpu"), f"{ident}: candidate/runtime binding mismatch")
        expected_format = "safetensors" if candidate["runtime_id"] == "candle_cpu" else "onnx"
        _required(candidate.get("model_artifact") == expected_format, f"{ident}: format/runtime mismatch")


def _safe_file(root: Path, path_text: str) -> Path:
    root = root.resolve()
    _required(isinstance(path_text, str), "unsafe local asset path")
    relative = Path(path_text)
    _required(not relative.is_absolute() and ".." not in relative.parts, "unsafe local asset path")
    probe = root
    for component in relative.parts:
        probe = probe / component
        _required(not probe.is_symlink(), "symlink in local input path")
    path = (root / relative).resolve()
    _required(path.is_relative_to(root), "local asset escaped root")
    return path


def _check_bytes(root: Path, entry: dict) -> None:
    path = _safe_file(root, entry["local_path"])
    _required(path.is_file(), f"missing asset: {entry['local_path']}")
    _required(path.stat().st_size == entry["bytes"], f"asset byte length changed: {entry['local_path']}")
    hasher = sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    _required(hasher.hexdigest() == entry["sha256"], f"asset SHA-256 changed: {entry['local_path']}")


def _read_pinned_json(root: Path, entry: dict, schema: str) -> dict:
    _exact_keys(entry, {"local_path", "sha256", "bytes"}, f"{schema} artifact pin")
    _digest(entry["sha256"], f"{schema} artifact")
    _required(type(entry["bytes"]) is int and entry["bytes"] > 0, f"{schema}: invalid byte count")
    path = _safe_file(root, entry["local_path"])
    _required(not path.is_symlink(), f"{schema}: symlink artifact")
    _check_bytes(root, entry)
    try:
        value = load_json(path)
    except (OSError, UnicodeError, ValueError) as error:
        raise ContractError(f"{schema}: invalid JSON artifact") from error
    _required(isinstance(value, dict) and value.get("schema") == schema, f"{schema}: wrong schema")
    return value


def _pairs(value: object, fields: set[str], name: str) -> list[dict]:
    _required(isinstance(value, list), f"{name}: list missing")
    for item in value:
        _exact_keys(item, fields, name)
        _required(all(isinstance(item[field], str) and item[field] for field in fields),
                  f"{name}: empty identifier")
    _required(len(value) == len({tuple(item[field] for field in sorted(fields)) for item in value}),
              f"{name}: duplicate row")
    return value


def _executed_synthetic_input(run: dict, root: Path, scale: int) -> dict:
    """Ask the pinned L/LG binary for the in-process Corpus and planner view."""
    identity = run["pre_run_inputs"]["build_identity"]
    entry = identity.get("baseline_export_executable")
    _exact_keys(entry, {"local_path", "sha256", "bytes"}, "baseline export executable")
    _required(entry == identity.get("arm_executables", {}).get("L") ==
              identity.get("arm_executables", {}).get("LG"),
              "executed synthetic input: export and L/LG executable differ")
    _check_bytes(root, entry)
    executable = _safe_file(root, entry["local_path"])
    _required(os.access(executable, os.X_OK), "executed synthetic input: baseline executable is not runnable")
    try:
        completed = subprocess.run(
            [str(executable), "--export-synthetic-input", str(scale),
             str(run["execution"]["window"])],
            capture_output=True, text=True, check=True, timeout=60,
        )
        exported = json.loads(completed.stdout)
    except (OSError, subprocess.SubprocessError, UnicodeError, ValueError) as error:
        raise ContractError("executed synthetic input: pinned L/LG export failed") from error
    _exact_keys(exported, {"schema", "seed", "scale", "window", "actor", "artifacts"},
                "executed synthetic input")
    _required(exported["schema"] == "p2-synthetic-executed-input-v1" and
              exported["seed"] == run["execution"]["seed"] and
              exported["scale"] == scale and
              exported["window"] == run["execution"]["window"] and
              exported["actor"] == run["execution"]["actor"],
              "executed synthetic input: seed/scale/window/actor differ")
    _exact_keys(exported["artifacts"], DATA_DIGESTS | {"planner_output"},
                "executed synthetic input artifacts")
    return exported["artifacts"]


def _verify_data_artifacts(run: dict, public: dict, workspace_root: Path, lane: str) -> dict:
    """Bind data digests to actual rows and recompute eligible parent judgments."""
    inputs = run["pre_run_inputs"]
    artifacts = inputs["data_artifacts"]
    _exact_keys(artifacts, DATA_DIGESTS | {"planner_output"}, "data artifact paths")
    data = {}
    for name, schema in DATA_SCHEMAS.items():
        entry = artifacts[name]
        _required(entry["sha256"] == inputs["data"][name], f"{name}: digest differs from file pin")
        data[name] = _read_pinned_json(workspace_root, entry, schema)
    planner_entry = artifacts["planner_output"]
    _required(planner_entry["sha256"] == inputs["planner_output_sha256"], "planner digest differs from artifact")
    planner = _read_pinned_json(workspace_root, planner_entry, "p2-planner-output-v1")
    planner_fields = ({"schema", "window", "seed", "baseline_plans"}
                      if lane == "synthetic" else
                      {"schema", "route_order", "fanout_by_arm", "window", "seed"})
    _exact_keys(planner, planner_fields, "planner output")
    _required((lane == "synthetic" or
               (planner["route_order"] == run["execution"]["route_order"] and
                planner["fanout_by_arm"] == inputs["fanout_by_arm"])) and
              planner["window"] == run["execution"]["window"] and
              planner["seed"] == run["execution"]["seed"], "planner artifact differs from selected route")
    corpus = data["corpus_text_sha256"]
    queries = data["query_text_sha256"]
    judgments = data["judgments_sha256"]
    _exact_keys(corpus, {"schema", "rows"}, "corpus rows")
    _exact_keys(queries, {"schema", "queries"}, "query rows")
    _exact_keys(judgments, {"schema", "judgments"}, "judgment rows")
    row_fields = {"docid", "text", "source_id", "version_id", "part_id", "unit_id", "locator", "source_generation"}
    synthetic_text_fields = {"source_snapshot", "logical_path", "profile", "parser_build_id",
                             "representation_ref", "raw_sha256", "text_sha256"}
    synthetic_number_fields = {"part_ordinal", "unit_ordinal", "raw_size_bytes"}
    if lane == "synthetic":
        row_fields |= synthetic_text_fields | synthetic_number_fields
    rows = corpus["rows"]
    _required(isinstance(rows, list), "corpus rows missing")
    for row in rows:
        _exact_keys(row, row_fields | ({"resource_index"} if lane == "synthetic" else set()), "corpus row")
        _required(all(isinstance(row[key], str) and row[key] for key in row_fields - synthetic_number_fields),
                  "corpus row binding/text missing")
        if lane == "synthetic":
            _required(type(row["resource_index"]) is int and row["resource_index"] >= 0,
                      "synthetic resource index missing")
            _required(all(type(row[key]) is int and row[key] >= 0 for key in synthetic_number_fields) and
                      row["raw_size_bytes"] > 0 and
                      sha256(row["text"].encode()).hexdigest() == row["text_sha256"],
                      "synthetic Unit/Part/text binding invalid")
    _required(rows and len({r["docid"] for r in rows}) == len(rows) and
              len({r["unit_id"] for r in rows}) == len(rows), "corpus docid/Unit incomplete or duplicate")
    row_by_docid = {r["docid"]: r for r in rows}
    query_rows = _pairs(queries["queries"], {"query_id", "text", "split", "family"}, "query row")
    _required(query_rows and len({q["query_id"] for q in query_rows}) == len(query_rows) and
              all(q["split"] in ("development", "untouched_holdout") for q in query_rows),
              "duplicate or invalid query split")
    query_by_id = {q["query_id"]: q for q in query_rows}
    _required(inputs["sample_plan"]["query_ids"] == [q["query_id"] for q in query_rows if q["split"] == "development"],
              "sample plan differs from actual eligible development queries")
    split = data["label_split_sha256"]
    _exact_keys(split, {"schema", "development", "untouched_holdout"}, "split rows")
    _required(set(split["development"]) == {q["query_id"] for q in query_rows if q["split"] == "development"} and
              set(split["untouched_holdout"]) == {q["query_id"] for q in query_rows if q["split"] == "untouched_holdout"} and
              not set(split["development"]) & set(split["untouched_holdout"]), "split artifact/query mismatch")
    source = data["source_snapshot_sha256"]
    read = data["current_read_sha256"]
    filt = data["filter_sha256"]
    _exact_keys(source, {"schema", "source_generation", "parents"}, "Source snapshot")
    _exact_keys(read, {"schema", "actor", "allowed", "denied", "unknown"}, "current Read")
    _exact_keys(filt, {"schema", "included_unit_ids"}, "filter")
    _required(read["actor"] == run["execution"]["actor"] and
              (lane != "synthetic" or
               source["source_generation"] == run["synthetic_fixture"]["source_generation"]),
              "Source/Read identity differs from fixture")
    parent_fields = {"source_id", "version_id"}
    source_parents = source["parents"]
    _required(isinstance(source_parents, list) and source_parents, "Source parent rows missing")
    source_keys = set()
    current_keys = set()
    for parent in source_parents:
        _exact_keys(parent, parent_fields | {"current"}, "Source parent")
        _required(type(parent["current"]) is bool, "Source current flag invalid")
        key = (parent["source_id"], parent["version_id"])
        _required(key not in source_keys, "duplicate Source parent")
        source_keys.add(key)
        if parent["current"]:
            current_keys.add(key)
    read_sets = {name: {(p["source_id"], p["version_id"]) for p in _pairs(read[name], parent_fields, name)}
                 for name in ("allowed", "denied", "unknown")}
    _required(not (read_sets["allowed"] & read_sets["denied"] or
                   read_sets["allowed"] & read_sets["unknown"] or
                   read_sets["denied"] & read_sets["unknown"]) and
              set.union(*read_sets.values()) == source_keys, "Read rows do not partition Source parents")
    included = filt["included_unit_ids"]
    _required(isinstance(included, list) and len(included) == len(set(included)) and
              set(included) <= {r["unit_id"] for r in rows}, "filter Unit set invalid")
    eligible_rows = [r for r in rows if r["unit_id"] in included and
                     (r["source_id"], r["version_id"]) in current_keys & read_sets["allowed"]]
    _required(all(r["source_generation"] == source["source_generation"] for r in rows),
              "corpus Source generation differs")
    binding = data["unit_binding_sha256"]
    _exact_keys(binding, {"schema", "bindings"}, "Unit bindings")
    binding_fields = row_fields - ({"text"} if lane == "synthetic" else {"text", "source_generation"})
    expected_bindings = [{k: r[k] for k in binding_fields} for r in rows]
    _required(sorted(binding["bindings"], key=lambda r: r.get("docid", "")) ==
              sorted(expected_bindings, key=lambda r: r["docid"]), "Unit/locator/Part binding differs from corpus")
    judgment_rows = judgments["judgments"]
    _required(isinstance(judgment_rows, list) and judgment_rows, "closed judgments missing")
    seen_judgments = set()
    split_parents = {"development": set(), "untouched_holdout": set()}
    calculated: dict[str, dict[tuple[str, str], int]] = {qid: {} for qid in query_by_id}
    eligible_docids = {r["docid"] for r in eligible_rows}
    for item in judgment_rows:
        _exact_keys(item, {"query_id", "docid", "grade"}, "judgment")
        key = (item["query_id"], item["docid"])
        _required(key not in seen_judgments and item["query_id"] in query_by_id and
                  item["docid"] in row_by_docid and type(item["grade"]) is int and
                  0 <= item["grade"] <= 3, "judgment has unknown/duplicate passage or grade")
        seen_judgments.add(key)
        judged_row = row_by_docid[item["docid"]]
        split_parents[query_by_id[item["query_id"]]["split"]].add(
            (judged_row["source_id"], judged_row["version_id"]))
        if item["docid"] in eligible_docids:
            row = row_by_docid[item["docid"]]
            parent_key = (row["source_id"], row["version_id"])
            calculated[item["query_id"]][parent_key] = max(
                item["grade"], calculated[item["query_id"]].get(parent_key, 0))
    _required(not split_parents["development"] & split_parents["untouched_holdout"],
              "judged parent crosses development/holdout split")
    eligible = data["eligible_parent_set_sha256"]
    _exact_keys(eligible, {"schema", "queries"}, "eligible judgments")
    expected_eligible = [{"query_id": qid, "parents": [
        {"source_id": source_id, "version_id": version_id, "grade": grade}
        for (source_id, version_id), grade in sorted(parents.items())]}
        for qid, parents in sorted(calculated.items())]
    _required(eligible["queries"] == expected_eligible, "eligible qrels differ from Source/Read/filter/judgments")
    if lane == "public_ja":
        _verify_public_rows(run, public, workspace_root, rows, query_rows, judgment_rows)
    else:
        baseline = workspace_root / "experiments/search-vector-poc"
        for field, filename in (("corpus_manifest_sha256", "corpus_manifest.json"),
                                ("queries_sha256", "queries.jsonl"), ("qrels_sha256", "qrels.jsonl")):
            path = baseline / filename
            _required(path.is_file() and sha256(path.read_bytes()).hexdigest() == run["synthetic_fixture"][field],
                      f"synthetic fixture {filename} bytes differ")
        try:
            baseline_manifest = load_json(baseline / "corpus_manifest.json")
            baseline_queries = [json.loads(line) for line in (baseline / "queries.jsonl").read_text().splitlines()]
            baseline_qrels = [json.loads(line) for line in (baseline / "qrels.jsonl").read_text().splitlines()]
        except (OSError, UnicodeError, ValueError) as error:
            raise ContractError("synthetic baseline fixture source invalid") from error
        _required(baseline_manifest.get("generation") == source["source_generation"] and
                  baseline_manifest.get("seed") == run["execution"]["seed"] and
                  len({r["resource_index"] for r in rows}) in baseline_manifest.get("scales", []) and
                  {r["resource_index"] for r in rows} == set(range(len({r["resource_index"] for r in rows}))) and
                  [(q.get("query_id"), q.get("text")) for q in baseline_queries] ==
                  [(q["query_id"], q["text"]) for q in query_rows] and
                  {q.get("query_id") for q in baseline_qrels} <= set(query_by_id) and
                  all(type(q.get("resource_index")) is int and
                      q["resource_index"] in {r["resource_index"] for r in rows}
                      for q in baseline_qrels),
                  "synthetic corpus/query/qrel artifacts differ from real baseline fixture")
        baseline_judgments = {(q["query_id"], q["resource_index"], q["grade"])
                              for q in baseline_qrels}
        actual_judgments = {(item["query_id"], row_by_docid[item["docid"]]["resource_index"],
                             item["grade"]) for item in judgment_rows}
        _required(actual_judgments == baseline_judgments,
                  "synthetic eligible judgment grades differ from baseline qrels bytes")
        executed = _executed_synthetic_input(run, workspace_root, len({r["resource_index"] for r in rows}))
        for name in DATA_DIGESTS:
            _required(data[name] == executed[name], f"executed synthetic input differs: {name}")
        _required(planner == executed["planner_output"],
                  "executed synthetic input differs: planner_output")
    return {"queries": query_by_id, "eligible": calculated, "rows": eligible_rows}


def _verify_public_rows(run: dict, public: dict, root: Path, rows: list[dict],
                        queries: list[dict], judgments: list[dict]) -> None:
    public_run = run["public_japanese"]
    entry = public_run.get("selected_rows_artifact")
    _required(isinstance(entry, dict), "selected public row artifact missing")
    _exact_keys(entry, {"local_path", "bytes", "sha256"}, "selected public row pin")
    _required(entry["sha256"] == public["corpus"]["selected_rows_sha256"], "selected public row SHA differs")
    _check_bytes(root, entry)
    raw = _safe_file(root, entry["local_path"]).read_bytes()
    try:
        selected = [json.loads(line) for line in raw.splitlines() if line]
    except (UnicodeError, ValueError) as error:
        raise ContractError("selected public row JSONL invalid") from error
    _required(all(isinstance(r, dict) and set(r) == {"docid", "text"} for r in selected),
              "selected public row schema invalid")
    _required({r["docid"]: r["text"] for r in selected} == {r["docid"]: r["text"] for r in rows} and
              len(selected) == len(rows), "public corpus differs from selected row bytes")
    mapping = public["corpus"]["selected_row_to_source_version_part_unit_map"]
    fields = {"source_id", "version_id", "part_id", "unit_id", "locator"}
    _required(isinstance(mapping, dict) and set(mapping) == {r["docid"] for r in rows} and
              all(isinstance(binding, dict) and set(binding) == fields for binding in mapping.values()),
              "public row to Source/Version/Part/Unit/locator mapping incomplete")
    for row in rows:
        _required(mapping[row["docid"]] == {field: row[field] for field in fields},
                  "public row binding differs from real corpus")
    source_files = public["topics_qrels"]
    for kind, pin_name in (("topics", "topics_sha256"), ("qrels", "qrels_sha256")):
        pinned = public_run.get(f"{kind}_artifact")
        _required(isinstance(pinned, dict) and pinned.get("sha256") == source_files[pin_name],
                  f"public {kind} pinned source artifact missing")
        _exact_keys(pinned, {"local_path", "sha256", "bytes"}, f"public {kind} source artifact")
        _check_bytes(root, pinned)
    try:
        topic_lines = _safe_file(root, public_run["topics_artifact"]["local_path"]).read_text().splitlines()
        qrel_lines = _safe_file(root, public_run["qrels_artifact"]["local_path"]).read_text().splitlines()
        topics = dict(line.split("\t", 1) for line in topic_lines)
        parsed_qrels = {(qid, docid, int(grade)) for qid, _, docid, grade in
                        (line.split() for line in qrel_lines)}
    except (OSError, UnicodeError, ValueError) as error:
        raise ContractError("public topic/qrel source schema invalid") from error
    _required(all(topics.get(q["query_id"]) == q["text"] for q in queries),
              "public query text differs from pinned topics bytes")
    _required({(j["query_id"], j["docid"], j["grade"]) for j in judgments} ==
              {item for item in parsed_qrels if item[0] in {q["query_id"] for q in queries}},
              "public judgments differ from pinned qrels bytes")
    _required({q["query_id"] for q in queries} == {q["query_id"] for q in public["queries"]} and
              {(j["query_id"], j["docid"], j["grade"]) for j in judgments} ==
              {(q["query_id"], j["docid"], j["grade"]) for q in public["queries"] for j in q["judgments"]},
              "public query/closed qrels differ from pinned manifest")
    _required([q["query_id"] for q in queries if q["split"] == "untouched_holdout"] ==
              run["public_japanese"]["holdout_ids"] and run["public_japanese"]["holdout_rank_look_count"] == 0,
              "public holdout was opened or split changed")


def verify_assets(manifest: dict, local_root: Path, *, full: bool, candidate_ids: list[str] | None = None) -> None:
    """Verify small metadata now, or selected complete candidates after acquisition."""
    validate_assets(manifest)
    for model in manifest["models"]:
        for entry in model["metadata"].values():
            _check_bytes(local_root, entry)
        alias = model["id"]
        config = load_json(_safe_file(local_root, model["metadata"]["config.json"]["local_path"]))
        tokenizer = load_json(_safe_file(local_root, model["metadata"]["tokenizer_config.json"]["local_path"]))
        special = load_json(_safe_file(local_root, model["metadata"]["special_tokens_map.json"]["local_path"]))
        sentence = load_json(_safe_file(local_root, model["metadata"]["sentence_bert_config.json"]["local_path"]))
        pooling = load_json(_safe_file(local_root, model["metadata"]["1_Pooling/config.json"]["local_path"]))
        modules = load_json(_safe_file(local_root, model["metadata"]["modules.json"]["local_path"]))
        _required(config.get("architectures") == ["BertModel"] and config.get("model_type") == "bert", f"{alias}: actual config architecture mismatch")
        _required(config.get("hidden_size") == 384 and config.get("num_hidden_layers") == 12, f"{alias}: actual config shape mismatch")
        _required(tokenizer.get("tokenizer_class") == model["embedding"]["tokenizer_class"], f"{alias}: actual tokenizer class mismatch")
        _required(tokenizer.get("model_max_length") == 512, f"{alias}: actual tokenizer file limit mismatch")
        _required(all(special.get(key) == value for key, value in (("cls_token", "<s>"), ("sep_token", "</s>"), ("pad_token", "<pad>"))), f"{alias}: actual special tokens mismatch")
        _required(sentence.get("max_seq_length") == model["embedding"]["max_seq_length"], f"{alias}: actual sequence limit mismatch")
        _required(pooling.get("pooling_mode_mean_tokens") is True and pooling.get("pooling_mode_cls_token") is False, f"{alias}: actual pooling mismatch")
        module_types = [item["type"] for item in modules]
        _required(("sentence_transformers.models.Normalize" in module_types) == (alias == "e5"), f"{alias}: normalization module mismatch")
    if not full:
        return
    candidates = {item["id"]: item for item in manifest["candidates"]}
    selected = candidate_ids or list(candidates)
    _required(bool(selected) and all(item in candidates for item in selected), "unknown full-verify candidate")
    by_model = {model["id"]: model for model in manifest["models"]}
    seen_paths = set()
    for candidate_id in selected:
        candidate = candidates[candidate_id]
        artifacts = by_model[candidate["model_id"]]["artifacts"]
        for name in (candidate["model_artifact"], "tokenizer_json", "sentencepiece"):
            entry = artifacts[name]
            if entry["local_path"] not in seen_paths:
                _check_bytes(local_root, entry)
                seen_paths.add(entry["local_path"])
        if candidate["runtime_id"] == "ort_cpu":
            native = manifest["runtimes"]["ort_cpu"]["native_archive"]
            _check_bytes(local_root, native)
            _digest(native.get("extracted_library_sha256"), "extracted ONNX Runtime shared library")
            _check_bytes(local_root, {"local_path": native.get("extracted_library_path"), "bytes": native.get("extracted_library_bytes"), "sha256": native["extracted_library_sha256"]})


def model_id(manifest: dict, candidate_id: str) -> str:
    """Preflight identity only; a production ID also needs built code/native hashes."""
    validate_assets(manifest)
    candidates = {item["id"]: item for item in manifest["candidates"]}
    _required(candidate_id in candidates, "unknown candidate")
    candidate = candidates[candidate_id]
    model = next(item for item in manifest["models"] if item["id"] == candidate["model_id"])
    runtime = manifest["runtimes"][candidate["runtime_id"]]
    identity = {
        "model_repository": model["repository"],
        "model_revision": model["revision"],
        "metadata_sha256": {name: entry["sha256"] for name, entry in sorted(model["metadata"].items())},
        "model_weight_sha256": model["artifacts"][candidate["model_artifact"]]["sha256"],
        "tokenizer_sha256": model["artifacts"]["tokenizer_json"]["sha256"],
        "sentencepiece_sha256": model["artifacts"]["sentencepiece"]["sha256"],
        "embedding": model["embedding"],
        "runtime": runtime,
    }
    encoded = json.dumps(identity, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
    return "preflight-sha256:" + sha256(encoded).hexdigest()


def validate_public_slice(public: dict) -> None:
    _required(public.get("schema") == "p2-public-ja-closed-pool-v1", "public slice schema mismatch")
    _revision(public.get("topics_qrels", {}).get("revision"), "topics/qrels revision")
    _revision(public.get("corpus", {}).get("revision"), "corpus revision")
    for name in ("topics_sha256", "qrels_sha256"):
        _digest(public["topics_qrels"].get(name), name)
    _required(public["topics_qrels"].get("license") == "apache-2.0", "topics/qrels license missing")
    _required(public["corpus"].get("underlying_wikipedia_text_license", "").startswith("CC BY-SA 3.0"), "2019 Wikipedia text license version missing")
    _required(public.get("grading", {}).get("unjudged_policy") == "unknown_not_grade_zero", "unjudged grade policy missing")
    for shard in public["corpus"].get("shards", []):
        _digest(shard.get("sha256"), "corpus shard")
        _required(shard.get("bytes", 0) > 0, "corpus shard size missing")
    _required(len(public["corpus"].get("shards", [])) == 14, "Japanese corpus shard pin incomplete")
    queries = public.get("queries", [])
    _required(len(queries) == 6, "public query set changed")
    seen_queries = set()
    parents = {"development": set(), "untouched_holdout": set()}
    for query in queries:
        qid = query.get("query_id")
        split = query.get("split")
        _required(qid not in seen_queries and split in parents and query.get("family"), "duplicate or unsplit query")
        seen_queries.add(qid)
        _digest(query.get("topic_sha256"), f"topic {qid}")
        judgments = query.get("judgments", [])
        _required(bool(judgments), f"{qid}: no closed judgments")
        docids = [item.get("docid") for item in judgments]
        _required(len(docids) == len(set(docids)) and all(isinstance(docid, str) and DOC_ID.fullmatch(docid) for docid in docids), f"{qid}: duplicate or invalid passage ID")
        _required(all(type(item.get("grade")) is int and item["grade"] in (0, 1) for item in judgments), f"{qid}: qrel grade changed")
        canonical = "".join(f"{qid} Q0 {item['docid']} {item['grade']}\n" for item in sorted(judgments, key=lambda value: value["docid"]))
        _required(sha256(canonical.encode()).hexdigest() == query.get("judgments_canonical_sha256"), f"{qid}: closed judgment set changed")
        parents[split].update(docid.split("#")[0] for docid in docids)
    _required(len(parents["development"]) > 0 and len(parents["untouched_holdout"]) > 0, "empty public split")
    _required(not (parents["development"] & parents["untouched_holdout"]), "development/holdout parent overlap")


def _manifest_digest(value: dict) -> str:
    data = (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()
    return sha256(data).hexdigest()


def _reject_postrun_fields(value: object) -> None:
    """The input document must not masquerade as a measurement receipt."""
    if isinstance(value, dict):
        for key, item in value.items():
            _required(not any(word in key.lower() for word in ("result", "receipt", "measured")),
                      f"post-run field in pre-run inputs: {key}")
            _reject_postrun_fields(item)
    elif isinstance(value, list):
        for item in value:
            _reject_postrun_fields(item)


def _validate_run_inputs(run: dict, assets: dict, public: dict, evaluation_lane: str) -> dict:
    _exact_keys(run, RUN_INPUT_KEYS | {"historical_baseline", "observed_at"}, "run protocol")
    _required(run["schema"] == "p2-model-run-protocol-v2", "run schema mismatch")
    _required(run["state"] == "PRE_RUN_READY_UNRUN", "pre-run inputs not ready")
    _required(run["assets_manifest_sha256"] == _manifest_digest(assets), "asset manifest digest changed")
    _required(run["public_slice_manifest_sha256"] == _manifest_digest(public), "public slice manifest digest changed")
    _required(evaluation_lane in ("synthetic", "public_ja"), "unknown evaluation lane")

    inputs = run["pre_run_inputs"]
    _exact_keys(inputs, {"state", "code", "source_files", "data", "data_artifacts", "build_identity",
                         "sample_plan",
                         "capacity_admission", "host_hardware", "embedding_policy",
                         "planner_output_sha256", "fanout_by_arm", "resource_scenario_id",
                         "deadline_utc"}, "pre-run inputs")
    _required(inputs["state"] == "EXACT_INPUTS_CAPTURED", "pre-run inputs not captured")
    _exact_keys(inputs["code"], CODE_DIGESTS, "code pins")
    _exact_keys(inputs["source_files"], CODE_CATEGORIES, "code path categories")
    _exact_keys(inputs["data"], DATA_DIGESTS, "data pins")
    for name, value in (*inputs["code"].items(), *inputs["data"].items()):
        _digest(value, name)
    _exact_keys(inputs["data_artifacts"], DATA_DIGESTS | {"planner_output"}, "data artifact inventory")
    for name, entry in inputs["data_artifacts"].items():
        _exact_keys(entry, {"local_path", "sha256", "bytes"}, f"{name} artifact")
        _digest(entry["sha256"], f"{name} artifact")
        _required(type(entry["bytes"]) is int and entry["bytes"] > 0,
                  f"{name}: artifact byte count missing")
        _required(entry["sha256"] == (inputs["planner_output_sha256"] if name == "planner_output"
                                         else inputs["data"][name]), f"{name}: data pin differs from artifact")
    for category, files in inputs["source_files"].items():
        _required(isinstance(files, dict) and bool(files), f"{category}: exact file set missing")
        for relative, digest in files.items():
            _required(isinstance(relative, str) and relative and not Path(relative).is_absolute() and
                      ".." not in Path(relative).parts and "\\" not in relative,
                      f"{category}: unsafe source path")
            _digest(digest, f"{category} {relative}")
        _required(inputs["code"][category + "_sha256"] == _source_set_digest(files),
                  f"{category}: source file set digest mismatch")
    _digest(inputs["planner_output_sha256"], "actual planner output")
    _exact_keys(inputs["sample_plan"], {"query_ids", "repetitions_per_query"}, "sample plan")
    _required(isinstance(inputs["sample_plan"]["query_ids"], list) and
              bool(inputs["sample_plan"]["query_ids"]) and
              len(inputs["sample_plan"]["query_ids"]) == len(set(inputs["sample_plan"]["query_ids"])) and
              type(inputs["sample_plan"]["repetitions_per_query"]) is int and
              inputs["sample_plan"]["repetitions_per_query"] > 0, "sample plan invalid")
    _utc(inputs["deadline_utc"], "deadline")
    hardware = inputs["host_hardware"]
    _exact_keys(hardware, {"fingerprint_sha256", "cpu_model", "free_memory_bytes_at_admission"}, "host hardware")
    _digest(hardware["fingerprint_sha256"], "host fingerprint")
    _required(isinstance(hardware["cpu_model"], str) and bool(hardware["cpu_model"].strip()), "CPU model missing")
    _required(type(hardware["free_memory_bytes_at_admission"]) is int and hardware["free_memory_bytes_at_admission"] > 0,
              "admission free memory missing")

    execution = run["execution"]
    _exact_keys(execution, {"actor", "dedup", "fusion", "host", "observation_method",
                            "observed_free_disk_kib", "observed_memory_free_percent",
                            "physical_memory_bytes", "route_order", "route_order_state",
                            "rustc", "score_policy", "seed", "selected_candidate_id",
                            "window", "window_max"}, "execution")
    _required(execution["fusion"] == "S1 PriorityConcat after Source-current eligibility", "unsupported fusion policy")
    _required(execution["score_policy"] == "raw scores within retriever trace only; no cross-retriever arithmetic",
              "raw-score addition or unsupported score policy")
    _required(execution["dedup"] == "Unit hit folded to parent Version ResourceId before first reported rank; one parent once",
              "unsupported parent folding policy")
    _required(execution["route_order_state"] == "BASELINE_L_LG_PLANNER_VERIFIED_DENSE_UNRUN",
              "baseline L/LG planner order not verified")
    _required(execution["route_order"] == ARM_ROUTES, "invalid priority or route order")
    _required(type(execution["window"]) is int and execution["window"] == 20,
              "unsupported retrieval window")
    _required(type(execution["window_max"]) is int and execution["window_max"] >= execution["window"],
              "invalid window maximum")
    _required(type(execution["seed"]) is int and execution["seed"] >= 0, "invalid seed")
    _required(isinstance(execution["host"], str) and bool(execution["host"]), "host missing")
    _required(type(execution["physical_memory_bytes"]) is int and execution["physical_memory_bytes"] > 0,
              "physical memory missing")
    _exact_keys(inputs["fanout_by_arm"], set(ARM_ROUTES), "arm fanout")
    for arm, route in ARM_ROUTES.items():
        fanout = inputs["fanout_by_arm"][arm]
        _required(type(fanout) is int and fanout >= len(route) and fanout <= execution["window_max"],
                  f"{arm}: fanout cannot reach all routed retrievers")

    candidate = next((item for item in assets["candidates"] if item["id"] == execution["selected_candidate_id"]), None)
    _required(candidate is not None, "selected candidate not pinned")
    model = next(item for item in assets["models"] if item["id"] == candidate["model_id"])
    embedding = model["embedding"]
    expected_embedding = {
        "tokenizer_sha256": model["artifacts"]["tokenizer_json"]["sha256"],
        "pooling": embedding["pooling"],
        "mask": "attention_mask",
        "normalization": embedding["retrieval_normalization"],
        "truncation": embedding["truncation"],
        "metric": embedding["metric"],
        "precision": embedding["precision"],
    }
    _required(inputs["embedding_policy"] == expected_embedding, "embedding/tokenizer policy differs from pinned candidate")

    fixture = run["synthetic_fixture"]
    _exact_keys(fixture, {"corpus_manifest_sha256", "queries_sha256", "qrels_sha256", "source_generation",
                          "quality_use", "required_strata"}, "synthetic fixture")
    for name in ("corpus_manifest_sha256", "queries_sha256", "qrels_sha256"):
        _digest(fixture.get(name), f"synthetic {name}")
    _required(isinstance(fixture.get("source_generation"), str) and fixture["source_generation"],
              "Source generation missing")
    public_run = run["public_japanese"]
    _exact_keys(public_run, {"adoption_quality_status", "corpus_rows_ready", "development_ids",
                             "holdout_ids", "holdout_rank_look_count", "slice_file", "status",
                             "selected_rows_artifact", "topics_artifact", "qrels_artifact"}, "public lane")
    _required(public_run.get("development_ids") == ["101", "1059", "1076"] and
              public_run.get("holdout_ids") == ["0", "102", "1043"] and
              public_run.get("holdout_rank_look_count") == 0,
              "public split or untouched holdout changed")
    if evaluation_lane == "public_ja":
        _required(public_run.get("corpus_rows_ready") is True and public.get("state") == "ROWS_FROZEN",
                  "public corpus rows not frozen")
        _digest(public["corpus"].get("selected_rows_sha256"), "selected public row bytes")
        _required(bool(public["corpus"].get("selected_row_to_source_version_part_unit_map")),
                  "selected public row binding missing")

    guard = run["execution_guard"]
    _exact_keys(guard, {"cargo_build", "cleanup", "current_capacity_verdict", "network_after_pin",
                        "pytorch_bin", "weight_download"}, "execution guard")
    _required(guard.get("current_capacity_verdict") == "ADMITTED", "resource admission required")
    _required(guard.get("network_after_pin") ==
              "block outbound at OS/process boundary and repeat load/query/rebuild; Python preflight offline test does not prove runtime offline",
              "unsupported offline policy")
    ceiling = run["resource_ceiling"]
    _exact_keys(ceiling, {"current_free_bytes", "currently_admitted_scenarios", "free_disk_observations",
                          "index_cache_max_bytes", "native_unpack_max_bytes", "paired_run_wall_seconds",
                          "per_process_rss_max_bytes", "recheck_before_each_acquire_or_build", "reserve_free_bytes",
                          "rust_target_max_additional_bytes", "single_model_wall_seconds", "temporary_max_bytes"},
                "resource ceiling")
    for name in ("current_free_bytes", "reserve_free_bytes", "per_process_rss_max_bytes",
                 "paired_run_wall_seconds", "single_model_wall_seconds", "index_cache_max_bytes",
                 "native_unpack_max_bytes", "rust_target_max_additional_bytes", "temporary_max_bytes"):
        _required(type(ceiling.get(name)) is int and ceiling[name] > 0, f"{name}: resource limit missing")
    _required(type(execution["observed_free_disk_kib"]) is int and
              execution["observed_free_disk_kib"] * 1024 == ceiling["current_free_bytes"],
              "disk observation and admission bytes differ")
    _required(ceiling["per_process_rss_max_bytes"] <= execution["physical_memory_bytes"] and
              ceiling["single_model_wall_seconds"] <= ceiling["paired_run_wall_seconds"],
              "RSS or wall limit exceeds host/paired budget")
    scenarios = run["resource_scenarios"]
    _required(set(scenarios) == {"e5_candle_only", "e5_ort_only", "e5_candle_ort_overlap_one_model",
                                 "minilm_candle_only", "minilm_ort_only", "minilm_candle_ort_overlap_one_model"},
              "resource scenario set changed")
    for name, scenario in scenarios.items():
        _exact_keys(scenario, {"components_bytes", "owned_peak_additional_bytes",
                               "required_free_with_reserve_bytes"}, f"{name} scenario")
        components = scenario.get("components_bytes")
        _required(isinstance(components, dict) and components and
                  all(type(value) is int and value > 0 for value in components.values()),
                  f"{name}: invalid capacity components")
        peak = sum(components.values())
        _required(scenario.get("owned_peak_additional_bytes") == peak and
                  scenario.get("required_free_with_reserve_bytes") == peak + ceiling["reserve_free_bytes"],
                  f"{name}: capacity arithmetic changed")
        model_name, variant = name.split("_", 1)
        pinned_model = next(model for model in assets["models"] if model["id"] == model_name)
        artifacts = pinned_model["artifacts"]
        expected_components = {
            "index_and_cache": ceiling["index_cache_max_bytes"],
            "temporary_conversion_or_download": ceiling["temporary_max_bytes"],
            "temporary_target": ceiling["rust_target_max_additional_bytes"],
            "tokenizer_and_sentencepiece": artifacts["tokenizer_json"]["bytes"] + artifacts["sentencepiece"]["bytes"],
        }
        if variant == "candle_only":
            expected_components["model_artifact"] = artifacts["safetensors"]["bytes"]
        elif variant == "ort_only":
            expected_components.update({
                "model_artifact": artifacts["onnx"]["bytes"],
                "native_archive": assets["runtimes"]["ort_cpu"]["native_archive"]["bytes"],
                "native_unpacked_ceiling": ceiling["native_unpack_max_bytes"],
            })
        else:
            _required(variant == "candle_ort_overlap_one_model", f"unsupported scenario: {name}")
            expected_components.update({
                "safetensors": artifacts["safetensors"]["bytes"],
                "onnx": artifacts["onnx"]["bytes"],
                "native_archive": assets["runtimes"]["ort_cpu"]["native_archive"]["bytes"],
                "native_unpacked_ceiling": ceiling["native_unpack_max_bytes"],
            })
        _required(components == expected_components, f"{name}: components differ from selected immutable assets/build/index ceilings")
    expected_scenario = candidate["model_id"] + ("_candle_only" if candidate["runtime_id"] == "candle_cpu" else "_ort_only")
    _required(inputs["resource_scenario_id"] == expected_scenario, "scenario does not match selected candidate")
    _required(ceiling["current_free_bytes"] >= scenarios[expected_scenario]["required_free_with_reserve_bytes"],
              "disk capacity below selected scenario")
    _required(ceiling["per_process_rss_max_bytes"] <= hardware["free_memory_bytes_at_admission"],
              "RSS ceiling exceeds available RAM at admission")
    parity = run["parity"]
    _exact_keys(parity, {"dimension", "finite_nonzero_required", "max_component_absolute_error",
                         "max_cosine_absolute_error", "maximum_norm_after_l2", "minimum_norm_after_l2",
                         "rank_reversal", "reference_vs_rust", "token_ids", "vector_reference"}, "parity policy")
    _required(parity.get("dimension") == 384 and parity.get("max_component_absolute_error") == 0.002 and
              parity.get("max_cosine_absolute_error") == 0.0001 and parity.get("minimum_norm_after_l2") == 0.9999 and
              parity.get("maximum_norm_after_l2") == 1.0001 and parity.get("finite_nonzero_required") is True,
              "unsupported numerical parity policy")
    _exact_keys(run["measurement"], {"ann_oracle", "critical_checks", "decision_sheet", "disk",
                                     "latency_samples", "memory", "no_positive", "scale_claim",
                                     "semantic_metrics", "sizes", "timing", "unjudged"}, "measurement policy")

    semantic = {name: run[name] for name in sorted(RUN_INPUT_KEYS)}
    _reject_postrun_fields(semantic)
    return json.loads(json.dumps(semantic, ensure_ascii=False, allow_nan=False))


def freeze_run(
    run: dict,
    assets: dict,
    public: dict,
    local_root: Path | None = None,
    *,
    evaluation_lane: str = "synthetic",
) -> dict:
    """Seal exact pre-run inputs. No arm result is consumed or emitted here."""
    validate_assets(assets)
    validate_public_slice(public)
    semantic = _validate_run_inputs(run, assets, public, evaluation_lane)
    _required(local_root is not None, "full local asset verification required")
    verify_assets(assets, local_root, full=True, candidate_ids=[run["execution"]["selected_candidate_id"]])
    _verify_source_files(run["pre_run_inputs"]["source_files"], local_root.parents[1])
    _verify_complete_source_closure(run["pre_run_inputs"]["source_files"], local_root.parents[1])
    _verify_build_identity(run["pre_run_inputs"]["build_identity"], run["pre_run_inputs"]["source_files"],
                           assets, run["execution"]["selected_candidate_id"], local_root.parents[1])
    _verify_data_artifacts(run, public, local_root.parents[1], evaluation_lane)
    _verify_capacity_admission(run, local_root)
    payload = {"evaluation_lane": evaluation_lane, "inputs": semantic}
    return {"schema": "p2-run-pin-v2", "sha256": _canonical_sha(payload, b"p2-run-pin-v2"), **payload}


def _validate_pin(pin: dict) -> None:
    _exact_keys(pin, {"schema", "sha256", "evaluation_lane", "inputs"}, "RunPin")
    _required(pin["schema"] == "p2-run-pin-v2", "RunPin schema mismatch")
    _digest(pin["sha256"], "RunPin")
    _required(pin["evaluation_lane"] in ("synthetic", "public_ja"), "RunPin lane invalid")
    _exact_keys(pin["inputs"], RUN_INPUT_KEYS, "RunPin inputs")
    _reject_postrun_fields(pin["inputs"])
    _required(pin["inputs"]["execution"]["route_order"] == ARM_ROUTES and
              pin["inputs"]["execution"]["fusion"] == "S1 PriorityConcat after Source-current eligibility" and
              pin["inputs"]["execution"]["score_policy"] ==
              "raw scores within retriever trace only; no cross-retriever arithmetic", "RunPin route policy invalid")
    _required(pin["sha256"] == _canonical_sha({"evaluation_lane": pin["evaluation_lane"], "inputs": pin["inputs"]},
                                              b"p2-run-pin-v2"), "RunPin mutated after freeze")


def verify_frozen_run(pin: dict, run: dict, assets: dict, public: dict, local_root: Path,
                      capacity_receipt: dict | None = None) -> None:
    """Fail before an arm if any declared semantic input changed after freeze."""
    _validate_pin(pin)
    validate_assets(assets)
    validate_public_slice(public)
    semantic = _validate_run_inputs(run, assets, public, pin["evaluation_lane"])
    _required(_canonical_sha({"evaluation_lane": pin["evaluation_lane"], "inputs": semantic},
                            b"p2-run-pin-v2") == pin["sha256"], "post-freeze input mutation")
    verify_assets(assets, local_root, full=True, candidate_ids=[run["execution"]["selected_candidate_id"]])
    root = local_root.parents[1]
    _verify_source_files(run["pre_run_inputs"]["source_files"], root)
    _verify_complete_source_closure(run["pre_run_inputs"]["source_files"], root)
    _verify_build_identity(run["pre_run_inputs"]["build_identity"], run["pre_run_inputs"]["source_files"],
                           assets, run["execution"]["selected_candidate_id"], root)
    _verify_data_artifacts(run, public, root, pin["evaluation_lane"])
    receipt = capacity_receipt if capacity_receipt is not None else capture_capacity_receipt(run, local_root)
    _verify_capacity_admission(run, local_root, receipt, initial=False)


def seal_run(pin: dict, seal_path: Path, run: dict, assets: dict, public: dict, local_root: Path) -> str:
    """Persist the pre-run pin exactly once, before any L/LG arm starts."""
    verify_frozen_run(pin, run, assets, public, local_root)
    seal = {
        "schema": "p2-run-seal-v1",
        "run_pin_sha256": pin["sha256"],
        "sealed_at_utc": datetime.now(timezone.utc).isoformat(),
        "pin": pin,
    }
    encoded = (json.dumps(seal, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()
    try:
        fd = os.open(seal_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o444)
    except FileExistsError as error:
        raise ContractError("pre-run seal already exists; inspect before retry") from error
    with os.fdopen(fd, "wb") as stream:
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    return sha256(encoded).hexdigest()


def _validate_seal(pin: dict, seal_path: Path, expected_sha256: str) -> datetime:
    _digest(expected_sha256, "pre-run seal")
    _required(seal_path.is_file(), "pre-run seal missing")
    encoded = seal_path.read_bytes()
    _required(sha256(encoded).hexdigest() == expected_sha256, "pre-run seal bytes changed")
    try:
        seal = json.loads(encoded)
    except (UnicodeError, ValueError) as error:
        raise ContractError("pre-run seal JSON invalid") from error
    _exact_keys(seal, {"schema", "run_pin_sha256", "sealed_at_utc", "pin"}, "pre-run seal")
    _required(seal["schema"] == "p2-run-seal-v1" and seal["run_pin_sha256"] == pin["sha256"] and
              seal["pin"] == pin, "pre-run seal RunPin mismatch")
    sealed_at = _utc(seal["sealed_at_utc"], "pre-run seal")
    _required(sealed_at <= datetime.now(timezone.utc), "pre-run seal is in the future")
    return sealed_at


def _read_arm_artifact(root: Path, entry: dict, pin: dict, arm: str, kind: str) -> dict:
    _exact_keys(entry, {"local_path", "sha256", "bytes"}, f"{arm} {kind} artifact")
    _digest(entry["sha256"], f"{arm} {kind} artifact")
    _required(type(entry["bytes"]) is int and entry["bytes"] > 0, f"{arm} {kind} artifact length missing")
    _check_bytes(root, entry)
    try:
        data = load_json(_safe_file(root, entry["local_path"]))
    except (OSError, UnicodeError, ValueError) as error:
        raise ContractError(f"{arm}: invalid {kind} artifact JSON") from error
    _required(isinstance(data, dict) and data.get("schema") == f"p2-arm-{kind}-v1" and
              data.get("run_pin_sha256") == pin["sha256"] and data.get("arm") == arm and
              data.get("route_order") == pin["inputs"]["execution"]["route_order"][arm],
              f"{arm}: {kind} artifact differs from RunPin")
    return data


def begin_arm(pin: dict, arm: str, run: dict, assets: dict, public: dict, local_root: Path,
              evidence_root: Path, capacity_receipt: dict | None = None) -> dict:
    """Reverify frozen inputs immediately before one arm and persist its preflight.

    This is a structural receipt, not a signature proving that inference ran.
    """
    _required(arm in ARM_ROUTES, "unknown arm")
    verify_frozen_run(pin, run, assets, public, local_root, capacity_receipt)
    executable = pin["inputs"]["pre_run_inputs"]["build_identity"]["arm_executables"][arm]
    preflight = {
        "schema": "p2-arm-preflight-v1", "run_pin_sha256": pin["sha256"], "arm": arm,
        "route_order": ARM_ROUTES[arm], "checked_at_utc": datetime.now(timezone.utc).isoformat(),
        "executable_sha256": executable["sha256"],
    }
    encoded = (json.dumps(preflight, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    path = evidence_root / arm / "preflight.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o444)
    except FileExistsError as error:
        raise ContractError(f"{arm}: preflight already exists; inspect before retry") from error
    with os.fdopen(fd, "wb") as stream:
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    return {"local_path": f"{arm}/preflight.json", "bytes": len(encoded),
            "sha256": sha256(encoded).hexdigest()}


def _verified_pin_data(pin: dict, workspace_root: Path) -> dict:
    public_path = workspace_root / "experiments/search-vector-model-poc/public-ja-slice.json"
    _required(public_path.is_file(), "pinned public slice manifest missing")
    public = load_json(public_path)
    _required(_manifest_digest(public) == pin["inputs"]["public_slice_manifest_sha256"],
              "public slice manifest changed after RunPin")
    return _verify_data_artifacts(pin["inputs"], public, workspace_root, pin["evaluation_lane"])


def _validate_arm_receipt(pin: dict, arm: str, receipt: dict, evidence_root: Path,
                          workspace_root: Path, sealed_at: datetime) -> tuple[datetime, datetime]:
    _exact_keys(receipt, {"arm", "run_pin_sha256", "route_order", "status", "started_at_utc",
                          "finished_at_utc", "measurements", "trace", "preflight", "sample_count"},
                f"{arm} receipt")
    _required(receipt["arm"] == arm and receipt["run_pin_sha256"] == pin["sha256"],
              f"{arm}: wrong RunPin or arm")
    _required(receipt["route_order"] == pin["inputs"]["execution"]["route_order"][arm],
              f"{arm}: actual route differs from RunPin")
    _required(receipt["status"] == "MEASURED", f"{arm}: arm unmeasured")
    plan = pin["inputs"]["pre_run_inputs"]["sample_plan"]
    expected_count = len(plan["query_ids"]) * plan["repetitions_per_query"]
    _required(type(receipt["sample_count"]) is int and receipt["sample_count"] == expected_count,
              f"{arm}: sample count differs from pinned query plan")
    preflight = _read_arm_artifact(evidence_root, receipt["preflight"], pin, arm, "preflight")
    _exact_keys(preflight, {"schema", "run_pin_sha256", "arm", "route_order", "checked_at_utc",
                            "executable_sha256"}, f"{arm} preflight")
    executable = pin["inputs"]["pre_run_inputs"]["build_identity"]["arm_executables"][arm]
    _required(preflight["executable_sha256"] == executable["sha256"],
              f"{arm}: preflight executable differs from RunPin")
    _check_bytes(workspace_root, executable)
    measurements = _read_arm_artifact(evidence_root, receipt["measurements"], pin, arm, "measurements")
    trace = _read_arm_artifact(evidence_root, receipt["trace"], pin, arm, "trace")
    _exact_keys(measurements, {"schema", "run_pin_sha256", "arm", "route_order", "sample_count",
                                "samples", "metrics"}, f"{arm} measurements")
    _exact_keys(trace, {"schema", "run_pin_sha256", "arm", "route_order", "events"}, f"{arm} trace")
    _required(measurements["sample_count"] == expected_count and
              isinstance(measurements["samples"], list) and len(measurements["samples"]) == expected_count and
              isinstance(trace["events"], list) and len(trace["events"]) == expected_count,
              f"{arm}: actual sample/trace count differs from pinned plan")
    evidence = _verified_pin_data(pin, workspace_root)
    eligible_rows = {row["unit_id"]: row for row in evidence["rows"]}
    query_ids = plan["query_ids"]
    seen_sample_ids = set()
    trace_by_id = {}
    for event in trace["events"]:
        _exact_keys(event, {"sample_id", "query_id", "route_order", "scored_results"}, f"{arm} trace event")
        _required(event["sample_id"] not in trace_by_id, f"{arm}: duplicate trace sample")
        trace_by_id[event["sample_id"]] = event
    recalls = []
    visible_false_positive = 0
    unjudged_count = 0
    for sample in measurements["samples"]:
        _exact_keys(sample, {"sample_id", "query_id", "route_order", "planner_output_sha256",
                             "latency_ms", "scored_results"}, f"{arm} sample")
        sample_id = sample["sample_id"]
        qid = sample["query_id"]
        _required(isinstance(sample_id, str) and sample_id not in seen_sample_ids and
                  qid in query_ids and sample["route_order"] == ARM_ROUTES[arm] and
                  sample["planner_output_sha256"] == pin["inputs"]["pre_run_inputs"]["planner_output_sha256"] and
                  type(sample["latency_ms"]) in (int, float) and math.isfinite(sample["latency_ms"]) and
                  sample["latency_ms"] >= 0, f"{arm}: sample/query/planner/latency invalid")
        seen_sample_ids.add(sample_id)
        _required(sample_id in trace_by_id and trace_by_id[sample_id] == {
            "sample_id": sample_id, "query_id": qid, "route_order": ARM_ROUTES[arm],
            "scored_results": sample["scored_results"]}, f"{arm}: trace differs from scored outputs")
        scored = sample["scored_results"]
        _required(isinstance(scored, list) and len(scored) <= pin["inputs"]["execution"]["window"],
                  f"{arm}: scored result window invalid")
        parents_seen = set()
        positive = {key for key, grade in evidence["eligible"][qid].items() if grade > 0}
        found = set()
        for rank, result in enumerate(scored, start=1):
            _exact_keys(result, {"rank", "route", "score", "unit_id", "source_id",
                                 "version_id", "part_id", "locator"}, f"{arm} scored result")
            unit_id = result["unit_id"]
            row = eligible_rows.get(unit_id)
            _required(row is not None and result["rank"] == rank and
                      result["route"] in ARM_ROUTES[arm] and
                      type(result["score"]) in (int, float) and math.isfinite(result["score"]) and
                      all(result[field] == row[field] for field in
                          ("source_id", "version_id", "part_id", "locator")),
                      f"{arm}: scored result route/eligibility/Unit binding invalid")
            parent = (row["source_id"], row["version_id"])
            _required(parent not in parents_seen, f"{arm}: parent not folded before rank")
            parents_seen.add(parent)
            found.add(parent)
            if parent not in evidence["eligible"][qid]:
                unjudged_count += 1  # Unknown remains unknown, never grade zero.
            elif evidence["eligible"][qid][parent] == 0:
                visible_false_positive += 1
        if positive:
            recalls.append(len(positive & found) / len(positive))
    _required(set(trace_by_id) == seen_sample_ids and
              {qid: sum(1 for sample in measurements["samples"] if sample["query_id"] == qid)
               for qid in query_ids} == {qid: plan["repetitions_per_query"] for qid in query_ids},
              f"{arm}: sample plan not fully observed")
    expected_metrics = {
        "parent_recall_at_20": sum(recalls) / len(recalls) if recalls else None,
        "visible_false_positive": visible_false_positive,
        "unjudged_count": unjudged_count,
    }
    _required(measurements["metrics"] == expected_metrics,
              f"{arm}: numeric metrics differ from scored output/judgments")
    started = _utc(receipt["started_at_utc"], f"{arm} started")
    finished = _utc(receipt["finished_at_utc"], f"{arm} finished")
    _required(started < finished <= _utc(pin["inputs"]["pre_run_inputs"]["deadline_utc"], "RunPin deadline"),
              f"{arm}: invalid time or deadline exceeded")
    _required(finished <= datetime.now(timezone.utc), f"{arm}: measured arm is in the future")
    checked = _utc(preflight["checked_at_utc"], f"{arm} preflight")
    _required(sealed_at <= checked <= started, f"{arm}: missing immediate post-seal pre-execution verification")
    _required((started - checked).total_seconds() <= 60, f"{arm}: stale pre-execution verification")
    return started, finished


def validate_baseline_receipt(pin: dict, receipt: dict, evidence_root: Path, seal_path: Path,
                              workspace_root: Path) -> str:
    """Validate real same-pin L/LG arm evidence after the pre-run freeze."""
    _validate_pin(pin)
    _exact_keys(receipt, {"schema", "run_pin_sha256", "seal_receipt_sha256", "arms"}, "baseline receipt")
    _required(receipt["schema"] == "p2-postrun-baseline-v1" and receipt["run_pin_sha256"] == pin["sha256"],
              "baseline receipt pin mismatch")
    sealed_at = _validate_seal(pin, seal_path, receipt["seal_receipt_sha256"])
    _exact_keys(receipt["arms"], {"L", "LG"}, "baseline arms")
    for arm in ("L", "LG"):
        started, _ = _validate_arm_receipt(pin, arm, receipt["arms"][arm], evidence_root,
                                           workspace_root, sealed_at)
        _required(started >= sealed_at, f"{arm}: baseline arm predates pre-run seal")
    return _canonical_sha(receipt, b"p2-postrun-baseline-v1")


def validate_paired_receipt(pin: dict, baseline: dict, dense: dict, evidence_root: Path, seal_path: Path,
                            workspace_root: Path) -> str:
    """Only D/LD/LDG measured after same-pin L/LG may form a paired receipt."""
    baseline_sha256 = validate_baseline_receipt(pin, baseline, evidence_root, seal_path, workspace_root)
    _exact_keys(dense, {"schema", "run_pin_sha256", "baseline_receipt_sha256", "arms"}, "dense receipt")
    _required(dense["schema"] == "p2-postrun-dense-v1" and dense["run_pin_sha256"] == pin["sha256"],
              "dense receipt pin mismatch")
    _required(dense["baseline_receipt_sha256"] == baseline_sha256, "dense receipt did not consume same-pin L/LG")
    _exact_keys(dense["arms"], {"D", "LD", "LDG"}, "dense arms")
    baseline_finished = max(_utc(item["finished_at_utc"], "baseline finish") for item in baseline["arms"].values())
    for arm in ("D", "LD", "LDG"):
        started, _ = _validate_arm_receipt(pin, arm, dense["arms"][arm], evidence_root,
                                           workspace_root, _validate_seal(pin, seal_path, baseline["seal_receipt_sha256"]))
        _required(started >= baseline_finished, f"{arm}: dense arm predates same-pin L/LG")
    return _canonical_sha(dense, b"p2-postrun-dense-v1")
