"""Three-candidate P3 semantic correctness run (selection/measurement remain separate)."""

import argparse
import copy
import hashlib
import json
import time
from pathlib import Path

from refinement import IntegrityFailure, SourceAuthority, canonical_digest, checked_relation, traverse
from refinement_backends import RefinedNeo, RefinedPg, RefinedRedb


HERE = Path(__file__).resolve().parent
FULL_ID = "00000000-0000-0000-0000-000000000066"


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def file_hashes():
    names = ["bench.py", "refinement.py", "refinement_backends.py", "run_refinement.py",
             "make_refinement_fixture.py", "src/main.rs", "src/refinement_oracle.rs",
             "src/bin/refinement_redb.rs", "Cargo.lock"]
    return {name: sha(HERE / name) for name in names}


def audit(backend, generation, authority, full_override=None):
    source = generation["source_id"]
    gid = full_override or generation["generation_id"]
    expected_resources = {r["resource_id"]: dict(r, generation_id=gid)
                          for r in generation["resources"]}
    expected_relations = {r["relation_id"]: r for r in generation["relations"]}
    resource_ids, relation_ids = backend.all_ids(source, gid)
    if set(resource_ids) != set(expected_resources) or set(relation_ids) != set(expected_relations):
        raise IntegrityFailure("persisted complete ID set disagrees with fixture")
    observed_resources = []
    for rid in resource_ids:
        row = backend.resource(source, gid, rid)
        if row != expected_resources[rid]:
            raise IntegrityFailure(f"resource roundtrip mismatch: {rid}")
        observed_resources.append(row)
    authority.validate_stage(source, observed_resources, "PersistentDiscoveryMetadata")
    observed_relations = []
    for rid in relation_ids:
        relation = checked_relation(backend, source, gid, rid)
        if relation != expected_relations[rid]:
            raise IntegrityFailure(f"canonical relation payload mismatch: {rid}")
        observed_relations.append(relation)
    expected_incidence = sorted((relation["relation_id"], ordinal, member["role"],
                                 member["resource_ref"], source, gid)
                                for relation in expected_relations.values()
                                for ordinal, member in enumerate(relation["participants"]))
    raw_incidence = sorted(tuple(row) for row in backend.incidence_rows(source, gid))
    if raw_incidence != expected_incidence:
        raise IntegrityFailure("complete persisted raw incidence set disagrees with fixture")
    expected_digest = canonical_digest(source, list(expected_resources.values()),
                                       list(expected_relations.values()))
    actual_digest = canonical_digest(source, observed_resources, observed_relations)
    if actual_digest != expected_digest:
        raise IntegrityFailure("full logical digest mismatch")
    return {"resource_count": len(observed_resources), "relation_count": len(observed_relations),
            "raw_incidence_count": len(raw_incidence), "logical_digest": actual_digest,
            "all_participants_checked": True}


def expect_negative(label, operation, transcript):
    start = time.perf_counter_ns()
    try:
        operation()
    except IntegrityFailure as error:
        outcome = {"case": label, "outcome": "RED", "error_class": type(error).__name__,
                   "error": str(error), "duration_ns": time.perf_counter_ns() - start}
        transcript(outcome)
        return outcome
    raise AssertionError(f"{label}: corruption/spoof was accepted")


def preflight(fixture, transcript):
    generations = fixture["generations"]
    authority = SourceAuthority(fixture["authority"])
    for generation in generations:
        authority.validate_stage(generation["source_id"], generation["resources"], fixture["retention"])
    sample = copy.deepcopy(generations[0]["resources"][0])
    sample["mapping"]["document_id"] = generations[0]["resources"][4]["resource_id"]
    negatives = [
        expect_negative("owner-mapping-spoof-before-write", lambda: authority.validate_stage(
            sample["source_id"], [sample], fixture["retention"]), transcript),
        expect_negative("session-only-before-write", lambda: authority.validate_stage(
            generations[0]["source_id"], generations[0]["resources"], "SessionOnly"), transcript),
        expect_negative("no-retention-before-write", lambda: authority.validate_stage(
            generations[0]["source_id"], generations[0]["resources"], "NoRetention"), transcript),
    ]
    return negatives


def run(backend, fixture, transcript, negatives):
    generations = fixture["generations"]
    authority = SourceAuthority(fixture["authority"])
    if not isinstance(backend, RefinedRedb):
        for stage_name, operation in [
            ("baseline-full", lambda: backend.stage_full(generations[0], generations[0]["generation_id"])),
            ("updated-incremental", lambda: backend.stage_incremental(generations[0], generations[1])),
            ("updated-independent-full", lambda: backend.stage_full(generations[1], FULL_ID)),
            ("cross-source-full", lambda: backend.stage_full(generations[2], generations[2]["generation_id"])),
        ]:
            start = time.perf_counter_ns()
            operation()
            outcome = "per-chunk-committed" if isinstance(backend, RefinedNeo) else "committed"
            transcript({"case": stage_name, "outcome": outcome, "duration_ns": time.perf_counter_ns() - start})
    else:
        transcript({"case": "redb-stage", "outcome": "committed", **backend.stage_receipt})

    audits = {}
    for label, generation, override in [
        ("baseline", generations[0], None), ("updated-incremental", generations[1], None),
        ("updated-independent-full", generations[1], FULL_ID), ("cross-source", generations[2], None),
    ]:
        start = time.perf_counter_ns()
        audits[label] = audit(backend, generation, authority, override)
        transcript({"case": f"audit:{label}", "outcome": "PASS",
                    "duration_ns": time.perf_counter_ns() - start, **audits[label]})
    if audits["updated-incremental"]["logical_digest"] != audits["updated-independent-full"]["logical_digest"]:
        raise IntegrityFailure("incremental/full logical digest mismatch")
    # A same-ID participant replacement must retire its old role/resource key.
    source, updated_gid = generations[1]["source_id"], generations[1]["generation_id"]
    changed_relation = "00000000-0000-0000-0000-0000000f4240"
    removed_relation = "00000000-0000-0000-0000-0000000f4241"
    old_product = "00000000-0000-0000-0000-000000002711"
    new_product = "00000000-0000-0000-0000-000000002721"
    if changed_relation in backend.neighbors(source, updated_gid, old_product, "product"):
        raise IntegrityFailure("same-ID old participant incidence survived delta")
    if changed_relation not in backend.neighbors(source, updated_gid, new_product, "product"):
        raise IntegrityFailure("same-ID new participant incidence missing")
    if removed_relation in backend.neighbors(source, updated_gid,
                                             "00000000-0000-0000-0000-000000002714", "borrower"):
        raise IntegrityFailure("deleted relation incidence survived delta")
    transcript({"case": "same-id-and-delete-old-incidence", "outcome": "PASS"})
    cases = []
    for scenario in fixture["scenarios"]:
        authority.calls.clear()
        authority.set_current_revision(scenario["source_id"], scenario["revision"])
        start = time.perf_counter_ns()
        actual = traverse(backend, scenario["source_id"], scenario["generation_id"],
                          scenario["plan"], authority)
        if scenario["expected_status"] != "ok" or actual != scenario["expected"]:
            raise AssertionError(f"{backend.name} {scenario['name']} oracle mismatch expected={scenario['expected']} actual={actual}")
        case = {"case": scenario["name"], "outcome": "PASS",
                "duration_ns": time.perf_counter_ns() - start, "path_hits": len(actual),
                "source_id": scenario["source_id"], "generation_id": scenario["generation_id"],
                "revision": scenario["revision"],
                "current_access_calls": sorted({(s, r, rev) for s, r, rev in authority.calls})}
        transcript(case)
        cases.append(case)

    first = generations[0]
    s, gid = first["source_id"], first["generation_id"]
    seed = first["resources"][0]["resource_id"]
    if backend.neighbors("00000000-0000-0000-0000-000000000003", gid, seed, "borrower"):
        raise IntegrityFailure("wrong Source returned incidences")
    if backend.neighbors(s, "00000000-0000-0000-0000-000000000099", seed, "borrower"):
        raise IntegrityFailure("wrong generation returned incidences")
    transcript({"case": "wrong-scope-query", "outcome": "PASS"})

    relation_id = first["relations"][0]["relation_id"]
    backend.corrupt_participant(s, gid, relation_id, 3)
    negatives.append(expect_negative("corrupt-participant-persisted", lambda: checked_relation(
        backend, s, gid, relation_id), transcript))
    reverse_id = first["relations"][1]["relation_id"]
    backend.corrupt_reverse_incidence(s, gid, reverse_id, 0)
    negatives.append(expect_negative("corrupt-reverse-incidence-persisted", lambda: checked_relation(
        backend, s, gid, reverse_id), transcript))
    temporal_id = first["resources"][17]["resource_id"]
    backend.corrupt_temporal(s, gid, temporal_id)
    negatives.append(expect_negative("corrupt-temporal-persisted", lambda: audit(
        backend, first, authority), transcript))
    backend.corrupt_mapping(s, gid, seed)
    authority.set_current_revision(s, "1")
    negatives.append(expect_negative("corrupt-source-mapping-query", lambda: traverse(
        backend, s, gid, fixture["scenarios"][0]["plan"], authority), transcript))
    return {"audits": audits, "cases": cases, "negative_proofs": negatives,
            "selection": "Blocked", "remaining_gates": [
                "200 samples per query class with cold/warm 1/2/4/8 readers and writer",
                "predeclared crash/restart/restore/fault schedule for each backend",
                "PostgreSQL shared publication/pin/GC/build-guard disposable probe",
                "independent candidate selection review"]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=["pg", "neo", "redb"], required=True)
    parser.add_argument("--port", type=int)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--oracle-binary", type=Path)
    parser.add_argument("--db", type=Path)
    parser.add_argument("--fixture", type=Path, default=HERE / "refinement-fixture.json")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--image-id", default=None)
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    if any(case["expected_status"] is None for case in fixture["scenarios"]):
        raise ValueError("real memory oracle expectations are missing")
    transcript_path = args.output.with_suffix(".jsonl")
    with transcript_path.open("w") as log:
        def emit(value):
            log.write(json.dumps(value, sort_keys=True) + "\n")
            log.flush()
        negatives = preflight(fixture, emit)
        if args.backend == "pg":
            backend = RefinedPg(args.port)
        elif args.backend == "neo":
            backend = RefinedNeo(args.port)
        else:
            backend = RefinedRedb(args.binary, args.fixture, args.db)
        try:
            result = run(backend, fixture, emit, negatives)
        finally:
            backend.close()
    report = {"phase": "p3-refinement-correctness", "backend": backend.name,
              "transport": backend.transport, "fixture_sha256": sha(args.fixture),
              "base_fixture_sha256": fixture["base_fixture_sha256"],
              "source_sha256": file_hashes(), "image_id": args.image_id,
              "oracle_binary_sha256": sha(args.oracle_binary) if args.oracle_binary else None,
              "adapter_binary_sha256": sha(args.binary) if args.binary else None,
              "persisted_state": ("relation/resource nodes present; no atomic READY marker"
                                  if isinstance(backend, RefinedNeo) else "READY generation marker"),
              "transaction_shape": ("batched independent Query API HTTP commits"
                                    if isinstance(backend, RefinedNeo)
                                    else "one committed transaction per staged generation"),
              "transcript_sha256": sha(transcript_path), "result": result}
    args.output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"backend": backend.name, "cases": len(result["cases"]),
                      "selection": result["selection"], "report": str(args.output),
                      "sha256": sha(args.output)}))


if __name__ == "__main__":
    main()
