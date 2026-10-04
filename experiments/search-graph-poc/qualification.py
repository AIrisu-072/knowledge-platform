"""P3-P04 native-backend qualification primitives.

The measured runner and its raw receipts are separate from the audited
fixed-fixture correctness runner. Nothing in this module selects a backend
without an independent review receipt.
"""

import copy
import hashlib
import json
import math
import threading
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import make_refinement_fixture
from refinement import IntegrityFailure, SourceAuthority, canonical_digest, traverse


HERE = Path(__file__).resolve().parent
REQUIRED_PROFILES = (100, 1000, 3000)
REQUIRED_READERS = (1, 2, 4, 8)
REQUIRED_TEMPERATURES = ("cold", "warm")
MIN_SAMPLES = 200
BENCHMARK_CLASSES = (
    "nary-repeated-role",
    "two-hop",
    "hidden-degree-visible-budget",
    "negative-offset-nanosecond",
    "source-one-revoked",
    "relation-only-add",
)
BACKENDS = ("postgresql", "redb", "neo4j")
HARD_GATES = ("semantic", "current_source", "retention", "restart",
              "restore", "faults", "publication", "capacity", "license",
              "measurements")
FIXED_SHA256 = "d870c538f762e0fdaf30b8f901a48b3b8411504eac1da4bbe36766ee10fc0cf3"
BASE_SHA256 = {
    100: "489a780c5c1152b9d530c7c1134a25577a1a035c1194eb221149f03041f4ac3b",
    1000: "b7ed5b68caab488b7c1b58870943052900159ee789a1eb718097d131e94b2f91",
    3000: "f98ac755d524c15656d26c3a54a523b97236bba206e1c69effdd4f48d153e3fb",
}


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def make_qualification_fixture(groups):
    """Extend the unchanged seed-314159 base fixture with audited semantics.

    Expected paths are the 100-group actual MemoryGraphRetriever results.
    The caller must re-run `refine-oracle` against each larger output and compare
    those paths before the result may satisfy the semantic gate.
    """
    if groups not in REQUIRED_PROFILES:
        raise ValueError("unsupported fixed profile")
    base = HERE / f"fixture-{groups}.json"
    if sha256(base) != BASE_SHA256[groups]:
        raise ValueError(f"base fixture hash mismatch: {base}")
    expected = json.loads((HERE / "refinement-fixture.json").read_text())
    if sha256(HERE / "refinement-fixture.json") != FIXED_SHA256:
        raise ValueError("audited refinement fixture hash mismatch")
    raw = base.read_bytes()
    if groups != 100:
        # The historical 1,000/3,000 fixtures omitted the Source-2 canary.
        # Add only that unchanged source before applying the audited extension.
        merged = json.loads(raw)
        source_two = json.loads((HERE / "fixture-100.json").read_text())["cross_source"]
        merged["cross_source"] = source_two
        raw_for_extension = json.dumps(merged, sort_keys=True, separators=(",", ":")).encode()
    else:
        raw_for_extension = raw

    class _BaseBytes:
        def read_bytes(self):
            return raw_for_extension

    prior = make_refinement_fixture.ORIGINAL
    try:
        make_refinement_fixture.ORIGINAL = _BaseBytes()
        fixture = make_refinement_fixture.make()
    finally:
        make_refinement_fixture.ORIGINAL = prior
    fixture["base_fixture_sha256"] = hashlib.sha256(raw).hexdigest()
    expected_cases = {case["name"]: case for case in expected["scenarios"]}
    if set(expected_cases) != {case["name"] for case in fixture["scenarios"]}:
        raise ValueError("scenario set changed")
    for case in fixture["scenarios"]:
        original = expected_cases[case["name"]]
        if any(case[field] != original[field] for field in
               ("source_id", "generation_id", "revision", "plan")):
            raise ValueError(f"oracle plan changed: {case['name']}")
        case["expected"] = copy.deepcopy(original["expected"])
        case["expected_status"] = original["expected_status"]
    if fixture["seed"] != 314159:
        raise ValueError("seed changed")
    return fixture


def _percentile_ns(sorted_durations, probability):
    return sorted_durations[math.ceil(probability * len(sorted_durations)) - 1]


def summarize_samples(samples, *, profile, query_class, temperature, readers):
    if profile not in REQUIRED_PROFILES or query_class not in BENCHMARK_CLASSES:
        raise ValueError("unknown qualification cell")
    if temperature not in REQUIRED_TEMPERATURES or readers not in REQUIRED_READERS:
        raise ValueError("unknown concurrency/temperature cell")
    cell = [s for s in samples if s.get("profile") == profile and
            s.get("query_class") == query_class and
            s.get("temperature") == temperature and
            s.get("readers") == readers]
    if len(cell) < MIN_SAMPLES:
        raise ValueError(f"fewer than {MIN_SAMPLES} raw samples")
    if len({s.get("sample") for s in cell}) != len(cell):
        raise ValueError("duplicate sample index")
    if any(s.get("outcome") != "PASS" or s.get("writer_active") is not True or
           not isinstance(s.get("duration_ns"), int) or s["duration_ns"] <= 0 or
           not isinstance(s.get("started_perf_ns"), int) or
           not isinstance(s.get("finished_perf_ns"), int) or
           s["finished_perf_ns"] <= s["started_perf_ns"]
           for s in cell):
        raise ValueError("sample missing native result, active writer or duration")
    points = sorted((moment, delta) for s in cell for moment, delta in (
        (s["started_perf_ns"], 1), (s["finished_perf_ns"], -1)))
    active = maximum = 0
    for _, delta in points:
        active += delta
        maximum = max(maximum, active)
    if readers > 1 and maximum < 2:
        raise ValueError("independent reader requests never overlapped")
    ordered = sorted(s["duration_ns"] for s in cell)
    return {"n": len(ordered), "p50_ns": _percentile_ns(ordered, 0.50),
            "p95_ns": _percentile_ns(ordered, 0.95),
            "p99_ns": _percentile_ns(ordered, 0.99),
            "min_ns": ordered[0], "max_ns": ordered[-1],
            "max_overlapping_full_requests": maximum}


def select_backend(reports, *, independent_review=None):
    """A missing candidate, hard gate or independent GO never selects PG."""
    gaps = []
    for backend in BACKENDS:
        result = reports.get(backend)
        if result is None:
            gaps.append(f"{backend}:absent")
            continue
        gaps.extend(f"{backend}:{gate}={result.get(gate, 'UNMEASURED')}"
                    for gate in HARD_GATES if result.get(gate) != "PASS")
    if independent_review != "GO":
        gaps.append("independent_review:GO required")
    if gaps:
        return {"status": "Blocked", "gaps": gaps}
    # A complete independent review must record the comparative decision.
    return {"status": "Blocked", "gaps": ["comparative_selection_decision:pending"]}


class PreparedBackend:
    """Per-query frontier loaded through a candidate's native incidence API.

    A reader must return persisted relation payload plus *all* native participant
    rows or edges for every relation attached to the requested frontier. The
    common traversal then checks the typed n-ary roles, ordinals, resources,
    current Source and temporal state against the frozen Memory oracle.
    """

    def __init__(self, reader, source, generation, plan):
        self.source = source
        self.generation = generation
        self.reader = reader
        self.relations = {}
        self.incidence = defaultdict(set)
        self.resources = {}
        self.complete_frontiers = set()
        frontier = set(plan["seed_nodes"])
        resource_ids = set(frontier)
        for pattern in plan["path_patterns"]:
            role = pattern["from_role"]
            if not frontier:
                break
            packet = reader.fetch_frontier(source, generation, sorted(frontier), role)
            self.complete_frontiers.update((rid, role) for rid in frontier)
            following = set()
            for payload, stored in packet:
                rid = payload["relation_id"]
                value = (payload, sorted((int(i), r, n) for i, r, n in stored))
                if rid in self.relations and self.relations[rid] != value:
                    raise IntegrityFailure("native packet relation changed within query")
                self.relations[rid] = value
                for _, member_role, member_id in value[1]:
                    self.incidence[member_id, member_role].add(rid)
                    resource_ids.add(member_id)
                    if member_role == pattern["to_role"]:
                        following.add(member_id)
            frontier = following
        for rid in sorted(resource_ids):
            row = reader.resource(source, generation, rid)
            if row is not None:
                if (row.get("source_id"), row.get("generation_id"), row.get("resource_id")) != (
                        source, generation, rid):
                    raise IntegrityFailure("native packet resource scope mismatch")
                self.resources[rid] = row

    def neighbors(self, source, generation, resource, role):
        if (source, generation) != (self.source, self.generation):
            return []
        return sorted(self.incidence.get((resource, role), ()))

    def relation(self, source, generation, relation_id):
        if (source, generation) != (self.source, self.generation):
            return None
        return self.relations.get(relation_id)

    def resource(self, source, generation, resource):
        if (source, generation) != (self.source, self.generation):
            return None
        return self.resources.get(resource)


class NativeSourceAuthority:
    """Read the candidate's persisted Source policy and scenario revision per decision."""

    def __init__(self, reader, scenario):
        self.reader = reader
        self.scenario = scenario
        self.calls = []
        self.observed = {}

    def decision(self, source, row):
        if source != self.scenario["source_id"]:
            raise IntegrityFailure("Source scope changed")
        resource = row["resource_id"]
        revision, mapping, status = self.reader.authority_decision(
            source, self.scenario["name"], resource)
        self.calls.append((source, resource, str(revision)))
        if str(revision) != str(self.scenario["revision"]) or mapping != row.get("mapping"):
            raise IntegrityFailure("native Source revision or mapping drift")
        if status not in ("Allowed", "Denied", "Unknown", "Error"):
            raise IntegrityFailure("native Source returned unknown decision")
        snapshot = (str(revision), mapping, status)
        prior = self.observed.setdefault((source, resource), snapshot)
        if prior != snapshot:
            raise IntegrityFailure("native Source changed within traversal")
        return status == "Allowed"

    def verify_before_return(self):
        for (source, resource), previous in self.observed.items():
            revision, mapping, status = self.reader.authority_decision(
                source, self.scenario["name"], resource)
            if (str(revision), mapping, status) != previous:
                raise IntegrityFailure("native Source changed before return")


def audit_bulk_native(generation, resources, relations, incidence_rows,
                      authority_entries, retention):
    """Full native row/edge enumeration; no sampled or expected row is skipped."""
    source, gid = generation["source_id"], generation["generation_id"]
    expected_resources = {r["resource_id"]: r for r in generation["resources"]}
    expected_relations = {r["relation_id"]: r for r in generation["relations"]}
    actual_resources = {r["resource_id"]: r for r in resources}
    actual_relations = {r["relation_id"]: r for r in relations}
    if (len(actual_resources) != len(resources) or
            len(actual_relations) != len(relations) or
            actual_resources != expected_resources or
            actual_relations != expected_relations):
        raise ValueError("complete native resource/relation payload mismatch")
    SourceAuthority(authority_entries).validate_stage(source, resources, retention)
    expected_incidence = sorted((r["relation_id"], i, p["role"], p["resource_ref"],
                                 source, gid)
                                for r in relations for i, p in enumerate(r["participants"]))
    actual_incidence = sorted(tuple(row) for row in incidence_rows)
    if actual_incidence != expected_incidence:
        raise ValueError("complete native incidence set mismatch")
    for relation in relations:
        members = relation["participants"]
        if (len(members) < 2 or len(members) !=
                len({(p["role"], p["resource_ref"]) for p in members}) or
                any(p["resource_ref"] not in actual_resources for p in members)):
            raise ValueError("n-ary participant/resource integrity mismatch")
    digest = canonical_digest(source, resources, relations)
    if digest != canonical_digest(source, list(expected_resources.values()),
                                  list(expected_relations.values())):
        raise ValueError("logical Graph digest mismatch")
    return {"resource_count": len(resources), "relation_count": len(relations),
            "raw_incidence_count": len(actual_incidence), "logical_digest": digest,
            "all_participants_checked": True}


def _path_sha256(paths):
    raw = json.dumps(paths, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(raw).hexdigest()


def run_cell(reader_factory, writer_fn, fixture, scenario, *, profile,
             temperature, readers, count=MIN_SAMPLES, deadline_seconds=30,
             on_sample=None):
    """Time full per-query native frontier + Source gate + typed traversal.

    Each host worker owns a distinct native handle. The writer mutates only an
    isolated BUILDING generation; the pinned READY generation stays immutable.
    """
    if temperature not in REQUIRED_TEMPERATURES or readers not in REQUIRED_READERS:
        raise ValueError("unknown measurement cell")
    if count < MIN_SAMPLES:
        raise ValueError("qualification requires at least 200 samples")
    if scenario["expected_status"] != "ok":
        raise ValueError("scenario lacks actual Memory oracle result")
    source, gid = scenario["source_id"], scenario["generation_id"]
    expected_hash = _path_sha256(scenario["expected"])
    stop = threading.Event()
    started = threading.Event()
    barrier = threading.Barrier(readers)
    emission_lock = threading.Lock()
    deadline = time.monotonic() + deadline_seconds

    def write_loop():
        return writer_fn(stop, started)

    def read_loop(worker_index):
        warm_reader = reader_factory() if temperature == "warm" else None
        records = []
        try:
            if not started.wait(timeout=5):
                raise TimeoutError("writer failed to start")
            if warm_reader is not None:
                warmed = PreparedBackend(warm_reader, source, gid, scenario["plan"])
                warm_authority = NativeSourceAuthority(warm_reader, scenario)
                warmed_paths = traverse(warmed, source, gid, scenario["plan"], warm_authority)
                warm_authority.verify_before_return()
                if warmed_paths != scenario["expected"]:
                    raise IntegrityFailure("warmup disagrees with Memory oracle")
            barrier.wait(timeout=5)
            for sample_id in range(worker_index, count, readers):
                if time.monotonic() >= deadline:
                    raise TimeoutError("30-second query-cell canary exceeded")
                start = time.perf_counter_ns()
                reader = warm_reader if warm_reader is not None else reader_factory()
                try:
                    # No persisted path/adjacency packet is reused across samples.
                    native = PreparedBackend(reader, source, gid, scenario["plan"])
                    current = NativeSourceAuthority(reader, scenario)
                    paths = traverse(native, source, gid, scenario["plan"], current)
                    current.verify_before_return()
                    actual_hash = _path_sha256(paths)
                    if paths != scenario["expected"] or actual_hash != expected_hash:
                        raise IntegrityFailure("native current-Source path disagrees with Memory oracle")
                    finished = time.perf_counter_ns()
                    elapsed = finished - start
                    record = {"profile": profile, "query_class": scenario["name"],
                                    "temperature": temperature, "readers": readers,
                                    "writer_active": started.is_set() and not stop.is_set(),
                                    "host_client_recreated": temperature == "cold",
                                    "native_read_transaction": "per frontier/Source request",
                                    "os_database_cache_reset": False,
                                    "warmup_requests_per_reader": 1 if temperature == "warm" else 0,
                                    "sample": sample_id, "worker": worker_index,
                                    "duration_ns": elapsed, "outcome": "PASS",
                                    "started_perf_ns": start,
                                    "finished_perf_ns": finished,
                                    "path_sha256": actual_hash,
                                    "oracle_path_sha256": expected_hash,
                                    "current_source_calls": sorted(set(current.calls)),
                                    "current_source_final_rereads": len(current.observed)}
                    records.append(record)
                    if on_sample is not None:
                        with emission_lock:
                            on_sample(record)
                except Exception as error:
                    if on_sample is not None:
                        with emission_lock:
                            on_sample({"profile": profile, "query_class": scenario["name"],
                                       "temperature": temperature, "readers": readers,
                                       "sample": sample_id, "worker": worker_index,
                                       "duration_ns": time.perf_counter_ns() - start,
                                       "outcome": "ERROR", "error_class": type(error).__name__,
                                       "error": str(error), "writer_active": started.is_set()})
                    raise
                finally:
                    if warm_reader is None:
                        reader.close()
        finally:
            if warm_reader is not None:
                warm_reader.close()
        return records

    with ThreadPoolExecutor(max_workers=readers + 1) as pool:
        writer_future = pool.submit(write_loop)
        reader_futures = [pool.submit(read_loop, index) for index in range(readers)]
        try:
            samples = [sample for future in reader_futures
                       for sample in future.result()]
        finally:
            stop.set()
        updates = writer_future.result(timeout=5)
    if not updates or any(item.get("outcome") != "committed" for item in updates):
        raise ValueError("no successful concurrent native incremental writer commits")
    return sorted(samples, key=lambda row: row["sample"]), updates
