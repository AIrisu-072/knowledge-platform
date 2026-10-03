"""P3-P04 qualification contract tests; no service or benchmark side effects."""

import hashlib
import json
import unittest
from pathlib import Path

from qualification import (
    BENCHMARK_CLASSES,
    REQUIRED_PROFILES,
    REQUIRED_READERS,
    REQUIRED_TEMPERATURES,
    MIN_SAMPLES,
    make_qualification_fixture,
    PreparedBackend,
    summarize_samples,
    select_backend,
    audit_bulk_native,
    run_cell,
    NativeSourceAuthority,
)
from refinement import SourceAuthority, traverse
from qualification_native import PgReader, NeoReader, writer_variant
from qualification_redb_client import RedbReader
from qualification_recovery import expect_fault, source_drift_fault
from qualification_runtime import parse_human_bytes


HERE = Path(__file__).resolve().parent.parent


class QualificationContractTests(unittest.TestCase):
    def test_fixed_100_fixture_preserves_audited_bytes(self):
        fixture = make_qualification_fixture(100)
        raw = json.dumps(fixture, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(raw).hexdigest(),
                         "d870c538f762e0fdaf30b8f901a48b3b8411504eac1da4bbe36766ee10fc0cf3")

    def test_scaled_fixtures_preserve_oracle_cases_and_group_counts(self):
        original = json.loads((HERE / "refinement-fixture.json").read_text())
        for count in REQUIRED_PROFILES:
            with self.subTest(groups=count):
                fixture = make_qualification_fixture(count)
                self.assertEqual(fixture["seed"], 314159)
                self.assertEqual(len(fixture["generations"][0]["resources"]), 4 * count + 3)
                self.assertEqual(len(fixture["generations"][0]["relations"]), count + 130)
                self.assertEqual([case["name"] for case in fixture["scenarios"]],
                                 [case["name"] for case in original["scenarios"]])
                self.assertEqual([case["expected"] for case in fixture["scenarios"]],
                                 [case["expected"] for case in original["scenarios"]])

    def test_sample_summary_refuses_missing_cell_and_199_samples(self):
        base = dict(profile=100, query_class=BENCHMARK_CLASSES[0], temperature="warm",
                    readers=1, writer_active=True, outcome="PASS", duration_ns=100)
        with self.assertRaisesRegex(ValueError, "200"):
            summarize_samples([dict(base, sample=i) for i in range(MIN_SAMPLES - 1)],
                              profile=100, query_class=BENCHMARK_CLASSES[0],
                              temperature="warm", readers=1)
        complete = [dict(base, sample=i, started_perf_ns=i * 100,
                         finished_perf_ns=i * 100 + 100) for i in range(MIN_SAMPLES)]
        self.assertEqual(summarize_samples(complete, profile=100,
                         query_class=BENCHMARK_CLASSES[0], temperature="warm",
                         readers=1)["n"], MIN_SAMPLES)

    def test_selection_refuses_missing_backend_or_recovery(self):
        complete = {"semantic": "PASS", "current_source": "PASS", "retention": "PASS",
                    "restart": "PASS", "restore": "PASS", "faults": "PASS",
                    "publication": "PASS", "capacity": "PASS", "license": "PASS",
                    "measurements": "PASS"}
        self.assertEqual(select_backend({"postgresql": complete})["status"], "Blocked")
        reports = {backend: dict(complete) for backend in ("postgresql", "redb", "neo4j")}
        reports["postgresql"]["restore"] = "UNMEASURED"
        self.assertEqual(select_backend(reports)["status"], "Blocked")

    def test_schedule_declares_all_reader_and_temperature_cells(self):
        self.assertEqual(REQUIRED_READERS, (1, 2, 4, 8))
        self.assertEqual(REQUIRED_TEMPERATURES, ("cold", "warm"))
        self.assertGreaterEqual(MIN_SAMPLES, 200)
        self.assertIn("hidden-degree-visible-budget", BENCHMARK_CLASSES)
        self.assertIn("two-hop", BENCHMARK_CLASSES)

    def test_native_frontier_packet_reconstructs_all_fixed_oracle_paths(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        rows = {(g["source_id"], g["generation_id"]): g for g in fixture["generations"]}

        class NativePacketReader:
            def fetch_frontier(self, source, generation, resources, role):
                stored = rows[source, generation]
                found = []
                for relation in stored["relations"]:
                    if any(p["role"] == role and p["resource_ref"] in resources
                           for p in relation["participants"]):
                        found.append((relation, [(i, p["role"], p["resource_ref"])
                                                 for i, p in enumerate(relation["participants"])]))
                return found

            def resource(self, source, generation, resource):
                return next((r for r in rows[source, generation]["resources"]
                             if r["resource_id"] == resource), None)

        for scenario in fixture["scenarios"]:
            with self.subTest(scenario=scenario["name"]):
                source, generation = scenario["source_id"], scenario["generation_id"]
                prepared = PreparedBackend(NativePacketReader(), source, generation,
                                           scenario["plan"])
                authority = SourceAuthority(fixture["authority"])
                authority.set_current_revision(source, scenario["revision"])
                self.assertEqual(traverse(prepared, source, generation,
                                          scenario["plan"], authority), scenario["expected"])

    def test_reader_adapters_do_not_reset_schema_and_keep_independent_handles(self):
        from unittest.mock import patch

        class Connection:
            autocommit = False

            def execute(self, *_args, **_kwargs):
                raise AssertionError("reader constructor attempted schema SQL")

            def close(self):
                pass

        with patch("qualification_native.psycopg.connect", side_effect=[Connection(), Connection()]):
            first, second = PgReader(15432), PgReader(15432)
            self.assertIsNot(first.conn, second.conn)
            self.assertTrue(first.conn.autocommit)
            first.close()
            second.close()
        with patch("qualification_native.http.client.HTTPConnection",
                   side_effect=[object(), object()]) as client:
            first, second = NeoReader(17474), NeoReader(17474)
            self.assertIsNot(first.connection, second.connection)
            self.assertEqual(client.call_count, 2)

    def test_bulk_native_audit_detects_surplus_and_same_id_retirement(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        generation = fixture["generations"][1]
        rows = generation["resources"]
        relations = generation["relations"]
        incidence = [(r["relation_id"], i, p["role"], p["resource_ref"],
                      generation["source_id"], generation["generation_id"])
                     for r in relations for i, p in enumerate(r["participants"])]
        result = audit_bulk_native(generation, rows, relations, incidence,
                                   fixture["authority"], fixture["retention"])
        self.assertEqual(result["raw_incidence_count"], len(incidence))
        self.assertTrue(result["all_participants_checked"])
        with self.assertRaisesRegex(ValueError, "incidence"):
            audit_bulk_native(generation, rows, relations, incidence + [incidence[0]],
                              fixture["authority"], fixture["retention"])

    def test_timed_cell_reads_native_frontier_each_sample_with_writer(self):
        import threading

        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        scenario = next(x for x in fixture["scenarios"]
                        if x["name"] == "nary-repeated-role")
        generation = fixture["generations"][0]
        calls = 0
        lock = threading.Lock()

        class Reader:
            def fetch_frontier(self, source, gid, resources, role):
                nonlocal calls
                with lock:
                    calls += 1
                return [(r, [(i, p["role"], p["resource_ref"])
                             for i, p in enumerate(r["participants"])])
                        for r in generation["relations"]
                        if any(p["role"] == role and p["resource_ref"] in resources
                               for p in r["participants"])]

            def resource(self, source, gid, resource):
                return next((r for r in generation["resources"]
                             if r["resource_id"] == resource), None)

            def authority_decision(self, source, case, resource):
                policy = fixture["authority"][source][resource]
                revision = scenario["revision"]
                return revision, policy["mapping"], policy["revision"].get(revision, "Unknown")

            def close(self):
                pass

        def writer(stop, started):
            commits = 0
            while not stop.is_set():
                commits += 1
                started.set()
                stop.wait(0.001)
            return [{"duration_ns": 100, "outcome": "committed"}] * commits

        samples, updates = run_cell(lambda: Reader(), writer, fixture, scenario,
                                    profile=100, temperature="warm", readers=4,
                                    count=MIN_SAMPLES, deadline_seconds=30)
        self.assertEqual(len(samples), MIN_SAMPLES)
        self.assertGreaterEqual(calls, MIN_SAMPLES)
        self.assertGreater(len(updates), 0)
        self.assertTrue(all(s["outcome"] == "PASS" and s["writer_active"]
                            for s in samples))

    def test_writer_variant_is_real_relation_only_incidence_delta(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        old = fixture["generations"][0]["relations"][0]
        new = writer_variant(old)
        self.assertEqual(new["relation_id"], old["relation_id"])
        self.assertNotEqual(new["participants"], old["participants"])
        self.assertNotEqual(new["qualifiers"], old["qualifiers"])
        self.assertEqual({(p["role"], p["resource_ref"]) for p in new["participants"]},
                         {(p["role"], p["resource_ref"]) for p in old["participants"]})

    def test_redb_client_preserves_native_frontier_and_current_source_path(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        scenario = fixture["scenarios"][0]
        generation = fixture["generations"][0]

        class Mux:
            def request(self, command, **args):
                if command == "frontier":
                    return [(r, [(i, p["role"], p["resource_ref"])
                                 for i, p in enumerate(r["participants"])])
                            for r in generation["relations"]
                            if any(p["role"] == args["role"] and
                                   p["resource_ref"] in args["resources"]
                                   for p in r["participants"])]
                if command == "resource":
                    return next((r for r in generation["resources"]
                                 if r["resource_id"] == args["resource"]), None)
                raise AssertionError(command)

        reader = RedbReader(Mux())
        prepared = PreparedBackend(reader, scenario["source_id"],
                                   scenario["generation_id"], scenario["plan"])
        authority = SourceAuthority(fixture["authority"])
        self.assertEqual(traverse(prepared, scenario["source_id"],
                                  scenario["generation_id"], scenario["plan"], authority),
                         scenario["expected"])

    def test_native_source_authority_reads_candidate_each_decision_and_fails_closed(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        scenario = fixture["scenarios"][0]
        row = fixture["generations"][0]["resources"][0]
        original_mapping = row["mapping"]
        class Reader:
            calls = 0
            def authority_decision(self, source, case, resource):
                self.calls += 1
                return scenario["revision"], original_mapping, "Allowed"
        reader = Reader()
        native = NativeSourceAuthority(reader, scenario)
        self.assertTrue(native.decision(scenario["source_id"], row))
        self.assertTrue(native.decision(scenario["source_id"], row))
        self.assertEqual(reader.calls, 2)
        row = dict(row, mapping={"kind": "Version", "document_id": "wrong"})
        with self.assertRaisesRegex(Exception, "Source"):
            native.decision(scenario["source_id"], row)

    def test_fault_gate_rejects_silent_empty_result(self):
        with self.assertRaisesRegex(AssertionError, "accepted"):
            expect_fault("missing-reverse", lambda: [], lambda _row: None)
        rows = []
        def rejected():
            raise RuntimeError("missing reverse incidence key")
        receipt = expect_fault("missing-reverse", rejected, rows.append)
        self.assertEqual(receipt["outcome"], "FAIL_CLOSED")
        self.assertEqual(rows[0], receipt)

    def test_native_source_drift_after_path_is_rejected_before_return(self):
        fixture = json.loads((HERE / "refinement-fixture.json").read_text())
        scenario = fixture["scenarios"][0]
        row = fixture["generations"][0]["resources"][0]
        class Reader:
            calls = 0
            def authority_decision(self, source, case, resource):
                self.calls += 1
                return scenario["revision"], row["mapping"], (
                    "Allowed" if self.calls == 1 else "Denied")
        authority = NativeSourceAuthority(Reader(), scenario)
        self.assertTrue(authority.decision(scenario["source_id"], row))
        with self.assertRaisesRegex(Exception, "changed before return"):
            authority.verify_before_return()

    def test_drift_fault_mutates_denied_policy_instead_of_allowed_noop(self):
        from unittest.mock import patch
        class Reader:
            status = {"seed": "Allowed", "revoked": "Denied"}
            def authority_decision(self, source, scenario, resource):
                return "2", resource, self.status[resource]
        reader = Reader()
        mutations = []
        def mutate(source, resource):
            mutations.append(resource)
            reader.status[resource] = "Allowed"
        def query(current, _fixture, case):
            observed = {resource: current.authority_decision("source", case, resource)
                        for resource in ["seed", "revoked"]}
            for resource, previous in observed.items():
                if current.authority_decision("source", case, resource) != previous:
                    raise RuntimeError("native Source changed before return")
            return []
        with patch("qualification_recovery.query_case", side_effect=query):
            result = source_drift_fault(reader, {}, mutate, lambda _row: None)
        self.assertEqual(mutations, ["revoked"])
        self.assertEqual(result["outcome"], "FAIL_CLOSED")

    def test_neo_drift_changes_only_selected_policy_and_preserves_mapping(self):
        from qualification_recovery import mutate_neo_source_status
        class Reader:
            rows = {"source|seed": {"mapping": "seed", "revision": {"2": "Allowed"}},
                    "source|revoked": {"mapping": "revoked", "revision": {"2": "Denied"}}}
            def run(self, query, params):
                key = params["key"]
                if "RETURN p.payload" in query:
                    return [[json.dumps(self.rows[key])]]
                self.rows[key] = json.loads(params["payload"])
                return []
        reader = Reader()
        key, original = mutate_neo_source_status(reader, "source", "revoked")
        self.assertEqual(key, "source|revoked")
        self.assertEqual(json.loads(original)["revision"]["2"], "Denied")
        self.assertEqual(reader.rows[key]["mapping"], "revoked")
        self.assertEqual(reader.rows[key]["revision"]["2"], "Allowed")
        self.assertEqual(reader.rows["source|seed"]["revision"]["2"], "Allowed")

    def test_docker_memory_unit_is_converted_without_guessing(self):
        self.assertEqual(parse_human_bytes("828MiB"), 828 * 1024 * 1024)
        self.assertEqual(parse_human_bytes("1.25GiB"), int(1.25 * 1024**3))
        with self.assertRaises(ValueError):
            parse_human_bytes("828unknown")


if __name__ == "__main__":
    unittest.main()
