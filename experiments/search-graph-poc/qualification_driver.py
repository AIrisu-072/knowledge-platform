#!/usr/bin/env python3
"""Isolated PostgreSQL/redb/Neo4j native qualification with raw samples."""

import argparse
import copy
import gzip
import hashlib
import json
import math
import os
import platform
import resource
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import psycopg

from qualification import (
    BENCHMARK_CLASSES, MIN_SAMPLES, REQUIRED_READERS,
    REQUIRED_TEMPERATURES, NativeSourceAuthority, PreparedBackend, audit_bulk_native,
    make_qualification_fixture, run_cell, select_backend, sha256,
    summarize_samples,
)
from qualification_native import (
    NeoReader, PgReader, neo_incremental_writer, pg_incremental_writer,
    prepare_writer_generation, stage_source_authority,
)
from qualification_redb_client import RedbMux, RedbReader
from qualification_runtime import (MIB, machine_inventory, machine_sample,
                                   owned_container, stop_on_disk_floor, enforce_candidate_budget)
from refinement import traverse
from refinement_backends import RefinedNeo, RefinedPg


HERE = Path(__file__).resolve().parent
FULL_ID = "00000000-0000-0000-0000-000000000066"


def canonical_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def disk_free():
    return shutil.disk_usage(HERE).free


def directory_bytes(path):
    if path is None:
        return None
    path = Path(path)
    if path.is_file():
        return path.stat().st_size
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file())


def process_rss_peak():
    # macOS ru_maxrss is bytes; Linux reports KiB.
    value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return value if sys.platform == "darwin" else value * 1024


def write_report(path, report):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    temporary.replace(path)


def duration_summary(rows):
    values = sorted(row["duration_ns"] for row in rows)
    if not values:
        raise ValueError("no committed writer timings")
    return {"n": len(values), "p50_ns": values[math.ceil(.50 * len(values)) - 1],
            "p95_ns": values[math.ceil(.95 * len(values)) - 1],
            "p99_ns": values[math.ceil(.99 * len(values)) - 1],
            "min_ns": values[0], "max_ns": values[-1]}


def source_hashes():
    names = ("qualification.py", "qualification_native.py", "qualification_driver.py",
             "qualification-method.md", "qualification-license.md",
             "refinement.py", "refinement_backends.py",
             "run_refinement.py", "make_refinement_fixture.py", "bench.py",
             "src/bin/refinement_redb.rs", "src/bin/qualification_redb.rs",
             "qualification_redb_client.py", "qualification_runtime.py",
             "qualification_recovery.py", "src/refinement_oracle.rs",
             "Cargo.toml", "Cargo.lock")
    return {name: sha256(HERE / name) for name in names}


def generate(groups, output, oracle_binary):
    fixture = make_qualification_fixture(groups)
    output.parent.mkdir(parents=True, exist_ok=True)
    raw = canonical_bytes(fixture)
    if output.exists() and output.read_bytes() != raw:
        raise FileExistsError("fixed fixture exists with different bytes")
    output.write_bytes(raw)
    verify_memory_oracle(oracle_binary, fixture, output.parent)
    return {"profile": groups, "fixture_sha256": sha256(output),
            "oracle_binary_sha256": sha256(oracle_binary),
            "actual_memory_oracle": "PASS"}


def verify_memory_oracle(oracle_binary, fixture, folder):
    if oracle_binary is None:
        raise ValueError("actual MemoryGraphRetriever oracle binary required")
    with tempfile.NamedTemporaryFile(prefix="p3-oracle-", suffix=".json", dir=folder,
                                     delete=False) as temp:
        oracle_path = Path(temp.name)
        temp.write(canonical_bytes(fixture))
    try:
        subprocess.run([str(oracle_binary), "refine-oracle", str(oracle_path)],
                       check=True, timeout=120, capture_output=True, text=True)
        observed = json.loads(oracle_path.read_text())
        if observed != fixture:
            raise ValueError("scaled actual Memory oracle diverges from pinned path expectations")
    finally:
        oracle_path.unlink(missing_ok=True)


def _staging(stager, fixture, backend, emit):
    gens = fixture["generations"]
    operations = [
        ("baseline-full", lambda: stager.stage_full(gens[0], gens[0]["generation_id"])),
        ("updated-incremental", lambda: stager.stage_incremental(gens[0], gens[1])),
        ("updated-independent-full", lambda: stager.stage_full(gens[1], FULL_ID)),
        ("cross-source-full", lambda: stager.stage_full(gens[2], gens[2]["generation_id"])),
    ]
    times = {}
    for name, operation in operations:
        start = time.perf_counter_ns()
        operation()
        duration = time.perf_counter_ns() - start
        times[name] = duration
        emit({"phase": "build", "case": name, "backend": backend,
              "outcome": "committed-per-HTTP-chunk" if backend == "neo" else "committed",
              "duration_ns": duration})
    return times


def _audit_all(reader, fixture, emit):
    output = {}
    gens = fixture["generations"]
    for label, generation, full in (
            ("baseline", gens[0], False), ("updated-incremental", gens[1], False),
            ("updated-independent-full", gens[1], True), ("cross-source", gens[2], False)):
        expected = copy.deepcopy(generation)
        if full:
            expected["generation_id"] = FULL_ID
            for row in expected["resources"]:
                row["generation_id"] = FULL_ID
        start = time.perf_counter_ns()
        actual = reader.bulk_rows(expected["source_id"], expected["generation_id"])
        result = audit_bulk_native(expected, *actual, fixture["authority"], fixture["retention"])
        result["duration_ns"] = time.perf_counter_ns() - start
        output[label] = result
        emit({"phase": "audit", "case": label, "outcome": "PASS", **result})
    if output["updated-incremental"]["logical_digest"] != output[
            "updated-independent-full"]["logical_digest"]:
        raise ValueError("full/incremental digest inequivalence")
    source, gid = gens[1]["source_id"], gens[1]["generation_id"]
    changed = "00000000-0000-0000-0000-0000000f4240"
    deleted = "00000000-0000-0000-0000-0000000f4241"
    if (changed in reader.neighbors(source, gid,
                                   "00000000-0000-0000-0000-000000002711", "product") or
            changed not in reader.neighbors(source, gid,
                                            "00000000-0000-0000-0000-000000002721", "product") or
            deleted in reader.neighbors(source, gid,
                                        "00000000-0000-0000-0000-000000002714", "borrower")):
        raise ValueError("same-ID replacement/deletion left incorrect native incidence")
    emit({"phase": "audit", "case": "same-id-delete-native-incidence", "outcome": "PASS"})
    return output


def _audit_source(reader, fixture, emit):
    policies, revisions = reader.bulk_authority()
    expected_policies = sorted((source, resource, policy)
                               for source, entries in fixture["authority"].items()
                               for resource, policy in entries.items())
    expected_revisions = sorted((case["source_id"], case["name"], case["revision"])
                                for case in fixture["scenarios"])
    if sorted(tuple(row) for row in policies) != expected_policies:
        raise ValueError("native Source policy row set/payload mismatch")
    if sorted(tuple(row) for row in revisions) != expected_revisions:
        raise ValueError("native Source revision row set mismatch")
    row = {"phase": "audit", "case": "native-current-Source", "outcome": "PASS",
           "policy_rows": len(policies), "revision_rows": len(revisions),
           "policy_sha256": hashlib.sha256(canonical_bytes(policies)).hexdigest(),
           "revision_sha256": hashlib.sha256(canonical_bytes(revisions)).hexdigest()}
    emit(row)
    return row


def _oracle_paths(reader, fixture, emit):
    outcomes = []
    for scenario in fixture["scenarios"]:
        source, gid = scenario["source_id"], scenario["generation_id"]
        prepared = PreparedBackend(reader, source, gid, scenario["plan"])
        authority = NativeSourceAuthority(reader, scenario)
        actual = traverse(prepared, source, gid, scenario["plan"], authority)
        authority.verify_before_return()
        if actual != scenario["expected"] or scenario["expected_status"] != "ok":
            raise ValueError(f"actual native oracle mismatch: {scenario['name']}")
        row = {"phase": "oracle", "case": scenario["name"], "outcome": "PASS",
               "path_sha256": hashlib.sha256(canonical_bytes(actual)).hexdigest(),
               "current_source_calls": sorted(set(authority.calls))}
        emit(row)
        outcomes.append(row)
    return outcomes


def measure(args):
    if args.profile not in (100, 1000, 3000) or args.backend not in ("pg", "neo", "redb"):
        raise ValueError("unsupported profile/backend")
    if args.backend == "redb" and (not args.redb_binary or not args.db_path):
        raise ValueError("redb needs an exact binary and new disposable DB path")
    if args.backend != "redb" and args.port is None:
        raise ValueError("container candidate needs its mapped port")
    if args.backend != "redb":
        if not args.container_name or not args.image_id or not args.data_dir:
            raise ValueError("container needs owned name, exact cached image ID and data dir")
        owned_container(args.container_name, args.image_id, args.data_dir)
    fixture = json.loads(args.fixture.read_text())
    if fixture != make_qualification_fixture(args.profile):
        raise ValueError("fixture differs from fixed generator or Memory oracle")
    verify_memory_oracle(args.oracle_binary, fixture, args.fixture.parent)
    if args.profile > 100:
        if not args.admission or not args.admission.exists():
            raise ValueError("measured peak admission is required before larger run")
        admission = json.loads(args.admission.read_text())
        if admission.get("profile") != args.profile or admission.get("status") != "accepted":
            raise ValueError("larger-profile measured peak admission does not match")
    if disk_free() < 2 * 1024**3:
        raise RuntimeError("disk admission failed: less than 2 GiB free")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    raw_path = args.output.with_suffix(".jsonl.gz")
    candidate_path = args.db_path if args.backend == "redb" else args.data_dir
    if args.resume:
        if not args.output.exists() or not raw_path.exists():
            raise FileNotFoundError("resume needs a prior report and raw log")
        report = json.loads(args.output.read_text())
        if (report.get("backend"), report.get("profile"), report.get("fixture_sha256"),
                report.get("oracle_binary_sha256")) != (
                    args.backend, args.profile, sha256(args.fixture), sha256(args.oracle_binary)):
            raise ValueError("resume input pin differs from saved measurement")
        current_hashes = source_hashes()
        if current_hashes != report["source_sha256"]:
            if not args.amendment or not args.amendment.exists():
                raise ValueError("changed source needs a written before/after amendment")
            report.setdefault("source_amendments", []).append({
                "prior": report["source_sha256"], "current": current_hashes,
                "amendment_sha256": sha256(args.amendment),
                "amendment_text": args.amendment.read_text()})
            report["source_sha256"] = current_hashes
        report["resume_count"] = report.get("resume_count", 0) + 1
        report["status"] = "RUNNING"
        report.pop("error", None)
    else:
        if args.output.exists() or raw_path.exists():
            raise FileExistsError("refuse to overwrite previous qualification receipts")
        before = {"disk_free_bytes": disk_free(), "data_dir_bytes": directory_bytes(candidate_path),
                  "rss_peak_bytes": process_rss_peak(),
                  "machine": machine_sample(args.container_name if args.backend != "redb" else None,
                                            candidate_path)}
        report = {"phase": "p3-native-qualification", "backend": args.backend,
              "profile": args.profile, "fixture_sha256": sha256(args.fixture),
              "oracle_binary_sha256": sha256(args.oracle_binary),
              "source_sha256": source_hashes(),
              "transport": {"pg": "psycopg/PostgreSQL protocol",
                            "neo": "Neo4j Query API v2 persistent HTTP (not Bolt)",
                            "redb": "multiplexed local JSON-lines and parallel native redb read transactions"}[args.backend],
              "environment": {"platform": platform.platform(), "machine": platform.machine(),
                              "python": sys.version, "psycopg": psycopg.__version__,
                              "cargo": subprocess.run(["cargo", "--version"],
                                                      capture_output=True, text=True,
                                                      timeout=5, check=True).stdout.strip(),
                              "cpu_count": os.cpu_count(),
                              "image_id": args.image_id if args.backend != "redb" else None,
                              "native_reader_handles": "one independent connection per reader" if args.backend != "redb" else
                              "separate native read transaction per request on shared Arc<Database>",
                              "cold_definition": "fresh host reader per sample; native/OS cache not flushed" if args.backend == "redb" else
                              "fresh host connection per sample; OS/database cache not flushed",
                              "warm_definition": "reused native connection; per-sample frontier not cached"},
              "hardware_toolchain": machine_inventory(),
              "before": before, "build": {}, "audits": {}, "oracle_cases": [],
              "cells": {}, "after": None, "gates": {gate: "UNMEASURED" for gate in
              ("semantic", "current_source", "retention", "restart", "restore",
               "faults", "publication", "capacity", "license", "measurements")},
              "selection": select_backend({}), "status": "RUNNING",
              "resume_count": 0, "source_amendments": [], "cell_attempts": {}}
    started = time.monotonic()
    mux = None
    try:
        with gzip.open(raw_path, "at" if args.resume else "wt") as raw:
            def emit(row):
                raw.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
                raw.flush()

            if args.backend == "redb":
                if args.db_path.exists() and not args.resume:
                    raise FileExistsError("refuse to overwrite a redb candidate")
                if not args.resume:
                    started_build = time.perf_counter_ns()
                    staged = subprocess.run([str(args.redb_binary), "stage", str(args.fixture),
                                             str(args.db_path)], check=True, capture_output=True,
                                            text=True, timeout=120)
                    receipt = json.loads(staged.stdout)
                    if receipt.get("status") != "ready":
                        raise ValueError("native redb stage did not return ready")
                    report["build"] = {"stage_wall_ns": time.perf_counter_ns() - started_build,
                                       **receipt}
                    emit({"phase": "build", "backend": "redb", "outcome": "committed",
                          **report["build"]})
                mux = RedbMux(args.redb_binary, args.db_path)
                factory = lambda: RedbReader(mux)

                def writer_fn(stop, started):
                    mux.request("writer_start")
                    # Probe a committed native update before readers enter.
                    until = time.monotonic() + 5
                    while mux.request("writer_progress") < 1:
                        if time.monotonic() > until:
                            raise RuntimeError("redb native writer never committed")
                        time.sleep(0.005)
                    started.set()
                    stop.wait()
                    return mux.request("writer_stop")
            else:
                if not args.resume:
                    stager = RefinedPg(args.port) if args.backend == "pg" else RefinedNeo(args.port)
                    try:
                        start_source = time.perf_counter_ns()
                        source_counts = stage_source_authority(stager, fixture, args.backend)
                        report["build"]["source_authority_ns"] = time.perf_counter_ns() - start_source
                        emit({"phase": "build", "case": "native-source-authority",
                              "backend": args.backend, "outcome": "committed", **source_counts,
                              "duration_ns": report["build"]["source_authority_ns"]})
                        report["build"].update(_staging(stager, fixture, args.backend, emit))
                        first_relation = prepare_writer_generation(
                            stager, fixture["generations"][0], backend=args.backend)
                    finally:
                        stager.close()
                else:
                    first_relation = fixture["generations"][0]["relations"][0]
                reader_type = PgReader if args.backend == "pg" else NeoReader
                factory = lambda: reader_type(args.port)
                writer_fn = (pg_incremental_writer if args.backend == "pg" else
                             neo_incremental_writer)(args.port, fixture["generations"][0]["source_id"],
                                                     first_relation)
            reader = factory()
            try:
                report["audits"] = _audit_all(reader, fixture, emit)
                report["source_audit"] = _audit_source(reader, fixture, emit)
                report["oracle_cases"] = _oracle_paths(reader, fixture, emit)
            finally:
                reader.close()
            report["gates"].update(semantic="PASS", current_source="PASS", retention="PASS")
            staged_metrics = machine_sample(
                args.container_name if args.backend != "redb" else None,
                candidate_path, mux.proc.pid if mux is not None else None)
            staged_metrics["driver_rss_peak_bytes"] = process_rss_peak()
            enforce_candidate_budget(staged_metrics, args.profile)
            staged_bytes = staged_metrics["candidate_data_bytes"]
            emit({"phase": "machine", "case": "post-stage", "candidate_data_bytes": staged_bytes,
                  "disk_free_bytes": disk_free()})
            if args.profile == 100 and staged_bytes is not None and staged_bytes >= 512 * MIB:
                raise RuntimeError("fixed fixture owned candidate exceeded 512 MiB")
            if disk_free() < int(1.5 * 1024**3):
                raise RuntimeError("disk floor crossed after stage")
            names = {s["name"]: s for s in fixture["scenarios"]}
            for name in BENCHMARK_CLASSES:
                for temperature in REQUIRED_TEMPERATURES:
                    for readers in REQUIRED_READERS:
                        if time.monotonic() - started > 300:
                            raise TimeoutError("five-minute candidate/profile canary exceeded")
                        cell = f"{name}/{temperature}/{readers}"
                        if cell in report["cells"]:
                            continue
                        attempt = report["cell_attempts"].get(cell, 0) + 1
                        report["cell_attempts"][cell] = attempt
                        write_report(args.output, report)
                        samples, updates = run_cell(
                            factory, writer_fn, fixture, names[name], profile=args.profile,
                            temperature=temperature, readers=readers, count=MIN_SAMPLES,
                            deadline_seconds=30,
                            on_sample=lambda row: emit({"phase": "query", "cell": cell,
                                                        "attempt": attempt, **row}))
                        summary = summarize_samples(samples, profile=args.profile,
                                                    query_class=name, temperature=temperature,
                                                    readers=readers)
                        report["cells"][cell] = {**summary,
                                                  "accepted_attempt": attempt,
                                                  "writer_commit_duration": duration_summary(updates)}
                        metrics = machine_sample(
                            args.container_name if args.backend != "redb" else None,
                            candidate_path, mux.proc.pid if mux is not None else None)
                        metrics["driver_rss_peak_bytes"] = process_rss_peak()
                        enforce_candidate_budget(metrics, args.profile)
                        report["cells"][cell]["machine_after"] = metrics
                        emit({"phase": "writer", "cell": cell, "updates": updates})
                        emit({"phase": "machine", "cell": cell, **metrics})
                        write_report(args.output, report)
                        if args.profile == 100 and metrics["candidate_data_bytes"] >= 512 * MIB:
                            raise RuntimeError("fixed fixture owned candidate exceeded 512 MiB")
                        if disk_free() < int(1.5 * 1024**3):
                            raise RuntimeError("disk floor crossed: stop owned candidate")
            if len(report["cells"]) != len(BENCHMARK_CLASSES) * len(REQUIRED_TEMPERATURES) * len(REQUIRED_READERS):
                raise ValueError("measurement matrix is incomplete")
            if mux is not None:
                report["native_concurrency"] = mux.request("server_stats")
                emit({"phase": "native-concurrency", **report["native_concurrency"]})
                if report["native_concurrency"]["max_parallel_frontier_reads"] < 2:
                    raise ValueError("redb native readers did not overlap")
            report["gates"]["measurements"] = "PASS"
        report["status"] = "MEASURED_FAULTS_PENDING"
    except Exception as error:
        report["status"] = "BLOCKED"
        report["error"] = {"class": type(error).__name__, "message": str(error)}
        raise
    finally:
        if mux is not None:
            try:
                mux.close()
            except Exception as cleanup_error:
                report["redb_server_close_error"] = str(cleanup_error)
        report["after"] = {"disk_free_bytes": disk_free(),
                           "data_dir_bytes": directory_bytes(candidate_path),
                           "rss_peak_bytes": process_rss_peak(),
                           "elapsed_seconds": time.monotonic() - started}
        if raw_path.exists():
            report["raw_sha256"] = sha256(raw_path)
        report["selection"] = select_backend({})
        if args.backend != "redb" and disk_free() < int(1.5 * 1024**3):
            try:
                report["owned_floor_cleanup"] = stop_on_disk_floor(
                    args.container_name, args.image_id, args.data_dir)
            except Exception as cleanup_error:
                report["owned_floor_cleanup_error"] = str(cleanup_error)
        write_report(args.output, report)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    generate_parser = commands.add_parser("generate")
    generate_parser.add_argument("--profile", type=int, required=True)
    generate_parser.add_argument("--output", type=Path, required=True)
    generate_parser.add_argument("--oracle-binary", type=Path, required=True)
    measure_parser = commands.add_parser("measure")
    measure_parser.add_argument("--backend", choices=("pg", "neo", "redb"), required=True)
    measure_parser.add_argument("--profile", type=int, required=True)
    measure_parser.add_argument("--fixture", type=Path, required=True)
    measure_parser.add_argument("--oracle-binary", type=Path, required=True)
    measure_parser.add_argument("--port", type=int)
    measure_parser.add_argument("--output", type=Path, required=True)
    measure_parser.add_argument("--image-id")
    measure_parser.add_argument("--container-name")
    measure_parser.add_argument("--redb-binary", type=Path)
    measure_parser.add_argument("--db-path", type=Path)
    measure_parser.add_argument("--data-dir", type=Path)
    measure_parser.add_argument("--admission", type=Path)
    measure_parser.add_argument("--resume", action="store_true")
    measure_parser.add_argument("--amendment", type=Path)
    args = parser.parse_args()
    if args.command == "generate":
        print(json.dumps(generate(args.profile, args.output, args.oracle_binary)))
    else:
        measure(args)
        print(json.dumps({"report": str(args.output), "sha256": sha256(args.output)}))


if __name__ == "__main__":
    main()
