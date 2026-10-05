#!/usr/bin/env python3
"""Exact native redb restart, offline-copy restore and fault receipts.

Container restart/restore probes run separately after their owned data stores
and image IDs have been admitted; this command never touches Docker state.
"""

import argparse
import copy
import json
import shutil
import tempfile
import time
from pathlib import Path

from qualification import NativeSourceAuthority, PreparedBackend, audit_bulk_native, sha256
from qualification_driver import _audit_all, _audit_source, _oracle_paths
from qualification_redb_client import RedbMux, RedbReader
from qualification_native import NeoReader, PgReader
from qualification_runtime import (GIB, QUAL_DATA, OWNER_LABEL, PREFIX,
                                   command, owned_container, machine_sample)
from refinement import IntegrityFailure, traverse


def expect_fault(case, operation, emit, expected_fragment=None):
    start = time.perf_counter_ns()
    try:
        operation()
    except Exception as error:
        if expected_fragment and expected_fragment not in str(error):
            raise AssertionError(f"{case}: unexpected failure: {error}") from error
        receipt = {"phase": "fault", "case": case, "outcome": "FAIL_CLOSED",
                   "error_class": type(error).__name__, "error": str(error),
                   "duration_ns": time.perf_counter_ns() - start}
        emit(receipt)
        return receipt
    raise AssertionError(f"{case}: corruption was accepted")


def check_native(reader, fixture, emit):
    audits = _audit_all(reader, fixture, emit)
    source = _audit_source(reader, fixture, emit)
    cases = _oracle_paths(reader, fixture, emit)
    return {"audits": audits, "source": source, "oracle_cases": len(cases)}


def query_case(reader, fixture, name):
    scenario = next(case for case in fixture["scenarios"] if case["name"] == name)
    prepared = PreparedBackend(reader, scenario["source_id"],
                               scenario["generation_id"], scenario["plan"])
    authority = NativeSourceAuthority(reader, scenario)
    paths = traverse(prepared, scenario["source_id"], scenario["generation_id"],
                     scenario["plan"], authority)
    authority.verify_before_return()
    if paths != scenario["expected"]:
        raise IntegrityFailure("native fault result differs from actual Memory oracle")
    return paths


def source_drift_fault(reader, fixture, mutate, emit):
    """Change persisted Source after first decision, before final native reread."""
    original = reader.authority_decision
    changed = False

    def injected(source, scenario, resource):
        nonlocal changed
        result = original(source, scenario, resource)
        if scenario == "source-one-revoked" and result[2] == "Denied" and not changed:
            changed = True
            mutate(source, resource)
        return result

    reader.authority_decision = injected
    try:
        return expect_fault("current-Source-drift-before-return",
                            lambda: query_case(reader, fixture, "source-one-revoked"),
                            emit, "native Source changed before return")
    finally:
        reader.authority_decision = original


def mutate_neo_source_status(reader, source, resource):
    """Mutate and retain the selected policy, never a different seed's payload."""
    key = f"{source}|{resource}"
    rows = reader.run(
        "MATCH (p:P3SourcePolicy {key:$key}) RETURN p.payload", {"key": key})
    if len(rows) != 1:
        raise RuntimeError("Neo4j Source policy absent before drift fault")
    original = rows[0][0]
    changed = json.loads(original)
    if changed["revision"]["2"] != "Denied":
        raise RuntimeError("Source drift injection requires a Denied policy")
    changed["revision"]["2"] = "Allowed"
    reader.run("MATCH (p:P3SourcePolicy {key:$key}) SET p.payload=$payload",
               {"key": key, "payload": json.dumps(changed, sort_keys=True)})
    return key, original


def _fault_on_copy(binary, original, case, mutate, verify, emit, expected_fragment):
    with tempfile.TemporaryDirectory(prefix="p3-redb-fault-", dir=original.parent) as folder:
        copied = Path(folder) / "candidate.redb"
        shutil.copy2(original, copied)
        mux = RedbMux(binary, copied)
        try:
            reader = RedbReader(mux)
            mutate(mux, reader)
            fault = expect_fault(case, lambda: verify(reader), emit, expected_fragment)
        finally:
            mux.close()
        fault["disposable_file_sha256"] = sha256(copied)
        return fault


def recover_redb(binary, db_path, fixture, output):
    if not db_path.exists():
        raise FileNotFoundError(db_path)
    if output.exists():
        raise FileExistsError(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    raw_path = output.with_suffix(".jsonl")
    if raw_path.exists():
        raise FileExistsError(raw_path)
    report = {"backend": "redb", "fixture_sha256": fixture["sha256"],
              "binary_sha256": sha256(binary), "original_file_sha256": sha256(db_path),
              "restart": "UNMEASURED", "restore": "UNMEASURED", "faults": "UNMEASURED",
              "publication": "NO_POSTGRESQL_SOURCE_ATOMIC_PROTOCOL", "fault_cases": []}
    with raw_path.open("w") as log:
        def emit(row):
            log.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
            log.flush()

        start = time.perf_counter_ns()
        mux = RedbMux(binary, db_path)
        try:
            report["restart_check"] = check_native(RedbReader(mux), fixture["value"], emit)
        finally:
            mux.close()
        report["restart_open_audit_ns"] = time.perf_counter_ns() - start
        report["restart"] = "PASS"
        with tempfile.TemporaryDirectory(prefix="p3-redb-restore-", dir=db_path.parent) as folder:
            backup = Path(folder) / "offline-backup.redb"
            start = time.perf_counter_ns()
            shutil.copy2(db_path, backup)
            report["backup_copy_ns"] = time.perf_counter_ns() - start
            report["backup_bytes"] = backup.stat().st_size
            report["backup_sha256"] = sha256(backup)
            mux = RedbMux(binary, backup)
            try:
                report["restore_check"] = check_native(RedbReader(mux), fixture["value"], emit)
            finally:
                mux.close()
        report["restore"] = "PASS"
        generation = fixture["value"]["generations"][0]
        source, gid = generation["source_id"], generation["generation_id"]
        relation = generation["relations"][0]["relation_id"]
        seed = generation["relations"][0]["participants"][0]["resource_ref"]
        fault_specs = (
            ("missing-participant-row",
             lambda mux, _reader: mux.request("corrupt_participant", source=source,
                 generation=gid, relation=relation, ordinal=3),
             lambda reader: query_case(reader, fixture["value"], "nary-repeated-role"),
             "missing native participant row"),
            ("missing-reverse-key-at-seed",
             lambda mux, _reader: mux.request("corrupt_reverse_incidence", source=source,
                 generation=gid, relation=relation, ordinal=0),
             lambda reader: query_case(reader, fixture["value"], "nary-repeated-role"),
             "reverse incidence key"),
            ("temporal-payload-drift",
             lambda mux, _reader: mux.request("corrupt_temporal", source=source,
                 generation=gid, resource=seed),
             lambda reader: audit_bulk_native(generation, *reader.bulk_rows(source, gid),
                 fixture["value"]["authority"], fixture["value"]["retention"]),
             "complete native resource/relation payload mismatch"),
            ("Source-mapping-drift",
             lambda mux, _reader: mux.request("corrupt_mapping", source=source,
                 generation=gid, resource=seed),
             lambda reader: query_case(reader, fixture["value"], "nary-repeated-role"),
             "native Source revision or mapping drift"),
        )
        for name, mutate, verify, expected in fault_specs:
            report["fault_cases"].append(_fault_on_copy(
                binary, db_path, name, mutate, verify, emit, expected))
        with tempfile.TemporaryDirectory(prefix="p3-redb-current-drift-",
                                         dir=db_path.parent) as folder:
            copied = Path(folder) / "candidate.redb"
            shutil.copy2(db_path, copied)
            mux = RedbMux(binary, copied)
            try:
                reader = RedbReader(mux)
                drift = source_drift_fault(reader, fixture["value"],
                    lambda source, resource: mux.request(
                        "corrupt_source_status", source=source, resource=resource,
                        revision="2", status="Allowed"), emit)
            finally:
                mux.close()
            drift["disposable_file_sha256"] = sha256(copied)
            report["fault_cases"].append(drift)
        writer_generation = "00000000-0000-0000-0000-0000000000c8"
        mux = RedbMux(binary, db_path)
        try:
            reader = RedbReader(mux)
            report["fault_cases"].append(expect_fault(
                "BUILDING-generation-invisible",
                lambda: reader.fetch_frontier(source, writer_generation, [seed], "borrower"),
                emit, "generation is not READY"))
        finally:
            mux.close()
        report["faults"] = "PASS"
    report["raw_sha256"] = sha256(raw_path)
    output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    return report


class _RollbackProbe(Exception):
    pass


def _pg_fault(reader, fixture, case, mutate, verify, expected, emit):
    try:
        with reader.conn.transaction():
            mutate()
            receipt = expect_fault(case, verify, emit, expected)
            raise _RollbackProbe()
    except _RollbackProbe:
        return receipt


def recover_pg(container_name, image_id, data_dir, port, fixture, output):
    """Restart owned PostgreSQL, restore pg_dump to a different DB, then fault it."""
    owned_container(container_name, image_id, data_dir)
    if output.exists():
        raise FileExistsError(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    raw_path = output.with_suffix(".jsonl")
    if raw_path.exists():
        raise FileExistsError(raw_path)
    report = {"backend": "postgresql", "fixture_sha256": fixture["sha256"],
              "image_id": image_id, "container_name": container_name,
              "restart": "UNMEASURED", "restore": "UNMEASURED", "faults": "UNMEASURED",
              "publication": "AWAITING_ACCEPTED_P7_PHYSICAL_SCHEMA", "fault_cases": []}
    with raw_path.open("w") as log:
        def emit(row):
            log.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
            log.flush()

        before = machine_sample(container_name, data_dir)
        start = time.perf_counter_ns()
        command(["docker", "restart", container_name], timeout=90)
        # Docker may remap a dynamic localhost port after restart.
        bound = command(["docker", "port", container_name, "5432/tcp"])
        actual_port = int(bound.decode().strip().rsplit(":", 1)[1])
        deadline = time.monotonic() + 60
        while True:
            try:
                reader = PgReader(actual_port)
                reader.conn.execute("SELECT 1")
                break
            except Exception:
                if time.monotonic() > deadline:
                    raise TimeoutError("PostgreSQL failed to recover after restart")
                time.sleep(0.5)
        try:
            report["restart_check"] = check_native(reader, fixture["value"], emit)
        finally:
            reader.close()
        report["restart_ns"] = time.perf_counter_ns() - start
        report["restart_port_before"] = port
        report["restart_port_after"] = actual_port
        report["restart"] = "PASS"
        emit({"phase": "restart", "outcome": "PASS", "duration_ns": report["restart_ns"],
              "port_after": actual_port, "machine_before": before,
              "machine_after": machine_sample(container_name, data_dir)})

        backup = output.with_suffix(".pgdump")
        if backup.exists():
            raise FileExistsError(backup)
        start = time.perf_counter_ns()
        dump = command(["docker", "exec", container_name, "pg_dump", "-U", "postgres",
                        "-d", "p3poc", "-Fc"], timeout=120)
        backup.write_bytes(dump)
        report["backup_sha256"] = sha256(backup)
        report["backup_bytes"] = len(dump)
        report["backup_ns"] = time.perf_counter_ns() - start
        restored_name = "p3poc_qualification_restored"
        command(["docker", "exec", container_name, "createdb", "-U", "postgres",
                 restored_name], timeout=30)
        start = time.perf_counter_ns()
        command(["docker", "exec", "-i", container_name, "pg_restore", "-U", "postgres",
                 "-d", restored_name, "--single-transaction", "--exit-on-error"],
                timeout=120, input_bytes=dump)
        report["restore_ns"] = time.perf_counter_ns() - start
        reader = PgReader(actual_port, restored_name)
        try:
            report["restore_check"] = check_native(reader, fixture["value"], emit)
            report["restore"] = "PASS"
            generation = fixture["value"]["generations"][0]
            source, gid = generation["source_id"], generation["generation_id"]
            relation = generation["relations"][0]["relation_id"]
            seed = generation["relations"][0]["participants"][0]["resource_ref"]
            specs = (
                ("missing-participant-row",
                 lambda: reader.corrupt_participant(source, gid, relation, 3),
                 lambda: query_case(reader, fixture["value"], "nary-repeated-role"),
                 "persisted participants disagree with payload"),
                ("missing-seed-incidence",
                 lambda: reader.corrupt_reverse_incidence(source, gid, relation, 0),
                 lambda: query_case(reader, fixture["value"], "nary-repeated-role"),
                 "actual Memory oracle"),
                ("temporal-native-column-drift",
                 lambda: reader.corrupt_temporal(source, gid, seed),
                 lambda: reader.bulk_rows(source, gid),
                 "PostgreSQL native temporal/owner column mismatch"),
                ("Source-mapping-drift",
                 lambda: reader.corrupt_mapping(source, gid, seed),
                 lambda: query_case(reader, fixture["value"], "nary-repeated-role"),
                 "native Source revision or mapping drift"),
            )
            for case, mutate, verify, expected in specs:
                report["fault_cases"].append(_pg_fault(
                    reader, fixture["value"], case, mutate, verify, expected, emit))
            try:
                with reader.conn.transaction():
                    drift = source_drift_fault(reader, fixture["value"],
                        lambda source, resource: reader.conn.execute(
                            "UPDATE p3_poc.source_policy SET policy=jsonb_set(policy, "
                            "'{revision,2}', '\"Allowed\"'::jsonb) "
                            "WHERE source_id=%s AND resource_id=%s",
                            (source, resource)), emit)
                    raise _RollbackProbe()
            except _RollbackProbe:
                report["fault_cases"].append(drift)
            report["faults"] = "PASS"
        finally:
            reader.close()
    report["raw_sha256"] = sha256(raw_path)
    output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    return report


def _neo_admin(image_id, data_dir, operation, input_bytes=None):
    if operation == "dump":
        args = ["database", "dump", "--to-stdout", "neo4j"]
    elif operation == "load":
        args = ["database", "load", "--from-stdin", "neo4j",
                "--overwrite-destination=true"]
    else:
        raise ValueError("unknown Neo4j offline operation")
    # Linux bind mounts keep host ownership; the explicit neo4j user must be
    # able to write the separately owned offline directory (amendment 2026-10-05).
    Path(data_dir).chmod(0o777)
    return command(["docker", "run", "--rm", "--pull=never", "--memory=2g",
                    "--user", "neo4j",
                    "--label", f"{OWNER_LABEL}=1", "--entrypoint", "neo4j-admin",
                    "-v", f"{data_dir}:/data", image_id, *args], timeout=180,
                   input_bytes=input_bytes)


def _neo_start_restore(name, image_id, data_dir):
    command(["docker", "run", "-d", "--pull=never", "--name", name,
             "--label", f"{OWNER_LABEL}=1", "--memory=2g",
             "-e", "NEO4J_AUTH=neo4j/p3syntheticpass",
             "-e", "NEO4J_db_tx__log_preallocate=false",
             "-e", "NEO4J_server_memory_heap_initial__size=256m",
             "-e", "NEO4J_server_memory_heap_max__size=256m",
             "-e", "NEO4J_server_memory_pagecache_size=128m",
             "-p", "127.0.0.1::7474", "-v", f"{data_dir}:/data", image_id], timeout=30)
    owned_container(name, image_id, data_dir)
    bound = command(["docker", "port", name, "7474/tcp"])
    return int(bound.decode().strip().rsplit(":", 1)[1])


def recover_neo(container_name, image_id, data_dir, port, fixture, output):
    """Community offline dump/load to a separate owned data dir and runtime."""
    owned_container(container_name, image_id, data_dir)
    if output.exists():
        raise FileExistsError(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    raw_path = output.with_suffix(".jsonl")
    if raw_path.exists():
        raise FileExistsError(raw_path)
    profile = (len(fixture["value"]["generations"][0]["resources"]) - 3) // 4
    restore_name = f"{PREFIX}neo-restore-{profile}"
    restore_dir = QUAL_DATA / restore_name
    backup = output.with_suffix(".neo4j.dump")
    if restore_dir.exists() or backup.exists():
        raise FileExistsError("Neo4j restore candidate or backup already exists")
    report = {"backend": "neo4j", "fixture_sha256": fixture["sha256"],
              "image_id": image_id, "container_name": container_name,
              "restart": "UNMEASURED", "restore": "UNMEASURED", "faults": "UNMEASURED",
              "publication": "NO_POSTGRESQL_SOURCE_ATOMIC_PROTOCOL", "fault_cases": []}
    with raw_path.open("w") as log:
        def emit(row):
            log.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
            log.flush()

        start = time.perf_counter_ns()
        command(["docker", "restart", container_name], timeout=90)
        bound = command(["docker", "port", container_name, "7474/tcp"])
        actual_port = int(bound.decode().strip().rsplit(":", 1)[1])
        deadline = time.monotonic() + 90
        while True:
            try:
                reader = NeoReader(actual_port)
                if reader.run("RETURN 1") == [[1]]:
                    break
                reader.close()
            except Exception:
                if time.monotonic() > deadline:
                    raise TimeoutError("Neo4j failed to recover after restart")
                time.sleep(0.5)
        try:
            report["restart_check"] = check_native(reader, fixture["value"], emit)
        finally:
            reader.close()
        report["restart_ns"] = time.perf_counter_ns() - start
        report["restart_port_before"] = port
        report["restart_port_after"] = actual_port
        report["restart"] = "PASS"
        emit({"phase": "restart", "outcome": "PASS", "duration_ns": report["restart_ns"],
              "machine": machine_sample(container_name, data_dir)})

        # Community Edition requires the database to be offline for dump/load.
        command(["docker", "stop", "--time", "15", container_name], timeout=45)
        start = time.perf_counter_ns()
        dump = _neo_admin(image_id, data_dir, "dump")
        backup.write_bytes(dump)
        backup.chmod(0o600)
        report["backup_sha256"] = sha256(backup)
        report["backup_bytes"] = len(dump)
        report["backup_ns"] = time.perf_counter_ns() - start
        emit({"phase": "backup", "outcome": "PASS", "bytes": report["backup_bytes"],
              "sha256": report["backup_sha256"], "duration_ns": report["backup_ns"]})
        restore_dir.mkdir(parents=True)
        (restore_dir / ".owner").write_text(restore_name + "\n")
        start = time.perf_counter_ns()
        _neo_admin(image_id, restore_dir, "load", input_bytes=dump)
        if shutil.disk_usage(output.parent).free < int(1.5 * GIB):
            raise RuntimeError("disk floor crossed during Neo4j offline restore")
        restore_port = _neo_start_restore(restore_name, image_id, restore_dir)
        deadline = time.monotonic() + 90
        while True:
            try:
                restored = NeoReader(restore_port)
                if restored.run("RETURN 1") == [[1]]:
                    break
                restored.close()
            except Exception:
                if time.monotonic() > deadline:
                    raise TimeoutError("Neo4j restored runtime failed to start")
                time.sleep(0.5)
        report["restore_ns"] = time.perf_counter_ns() - start
        report["restore_port"] = restore_port
        try:
            report["restore_check"] = check_native(restored, fixture["value"], emit)
            report["restore"] = "PASS"
            # Each fault gets a complete disposable baseline generation so the
            # failed native edge cannot contaminate the next assertion.
            generation = fixture["value"]["generations"][0]
            source = generation["source_id"]
            relation = generation["relations"][0]["relation_id"]
            seed = generation["relations"][0]["participants"][0]["resource_ref"]
            cases = (
                ("missing-participant-edge", 0xd1,
                 lambda gid: restored.corrupt_participant(source, gid, relation, 3),
                 lambda f: query_case(restored, f, "nary-repeated-role"),
                 "persisted participants disagree with payload"),
                ("missing-seed-incidence", 0xd2,
                 lambda gid: restored.corrupt_reverse_incidence(source, gid, relation, 0),
                 lambda f: query_case(restored, f, "nary-repeated-role"),
                 "actual Memory oracle"),
                ("temporal-payload-drift", 0xd3,
                 lambda gid: restored.corrupt_temporal(source, gid, seed),
                 lambda f: audit_bulk_native(f["generations"][0],
                     *restored.bulk_rows(source, f["generations"][0]["generation_id"]),
                     f["authority"], f["retention"]),
                 "complete native resource/relation payload mismatch"),
                ("Source-mapping-drift", 0xd4,
                 lambda gid: restored.corrupt_mapping(source, gid, seed),
                 lambda f: query_case(restored, f, "nary-repeated-role"),
                 "native Source revision or mapping drift"),
            )
            for case, suffix, mutate, verify, expected in cases:
                fault_gid = f"00000000-0000-0000-0000-0000000000{suffix:02x}"
                copied_fixture = copy.deepcopy(fixture["value"])
                copied_fixture["generations"][0]["generation_id"] = fault_gid
                for row in copied_fixture["generations"][0]["resources"]:
                    row["generation_id"] = fault_gid
                for scenario in copied_fixture["scenarios"]:
                    if scenario["source_id"] == source and scenario["generation_id"] == generation["generation_id"]:
                        scenario["generation_id"] = fault_gid
                restored.stage_full(generation, fault_gid)
                mutate(fault_gid)
                report["fault_cases"].append(expect_fault(
                    case, lambda f=copied_fixture: verify(f), emit, expected))
                restored.run("MATCH (n) WHERE n.source=$source AND n.generation=$gid "
                             "DETACH DELETE n", {"source": source, "gid": fault_gid})
            mutated_policies = {}
            def mutate_source(source_id, resource_id):
                key, original = mutate_neo_source_status(restored, source_id, resource_id)
                mutated_policies[key] = original
            try:
                report["fault_cases"].append(source_drift_fault(
                    restored, fixture["value"], mutate_source, emit))
            finally:
                for key, original in mutated_policies.items():
                    restored.run("MATCH (p:P3SourcePolicy {key:$key}) SET p.payload=$payload",
                                 {"key": key, "payload": original})
            _audit_source(restored, fixture["value"], emit)
            report["faults"] = "PASS"
        finally:
            restored.close()
    report["raw_sha256"] = sha256(raw_path)
    output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=("redb", "pg", "neo"), required=True)
    parser.add_argument("--redb-binary", type=Path)
    parser.add_argument("--db-path", type=Path)
    parser.add_argument("--container-name")
    parser.add_argument("--image-id")
    parser.add_argument("--data-dir", type=Path)
    parser.add_argument("--port", type=int)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    fixture = {"value": json.loads(args.fixture.read_text()), "sha256": sha256(args.fixture)}
    if args.backend == "redb":
        report = recover_redb(args.redb_binary, args.db_path, fixture, args.output)
    elif args.backend == "pg":
        report = recover_pg(args.container_name, args.image_id, args.data_dir,
                            args.port, fixture, args.output)
    else:
        report = recover_neo(args.container_name, args.image_id, args.data_dir,
                             args.port, fixture, args.output)
    print(json.dumps({"report": str(args.output), "sha256": sha256(args.output),
                      "restart": report["restart"], "restore": report["restore"],
                      "faults": report["faults"]}))


if __name__ == "__main__":
    main()
