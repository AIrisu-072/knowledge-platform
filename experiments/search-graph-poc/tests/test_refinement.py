import copy
import json
import unittest
from pathlib import Path

from refinement import (
    IntegrityFailure,
    SourceAuthority,
    canonical_digest,
    checked_relation,
    traverse,
    stamp_ns,
)
from run_refinement import audit


S = "00000000-0000-0000-0000-000000000001"
G = "00000000-0000-0000-0000-000000000064"
A = "00000000-0000-0000-0000-000000002710"
B = "00000000-0000-0000-0000-000000002711"
C = "00000000-0000-0000-0000-000000002713"
R = "00000000-0000-0000-0000-0000000f4240"


def relation():
    return {
        "relation_id": R, "namespace": "Discovery", "relation_type": "loan",
        "participants": [
            {"role": "borrower", "resource_ref": A},
            {"role": "product", "resource_ref": B},
            {"role": "collateral", "resource_ref": C},
        ],
        "qualifiers": {"ordered": {"List": [{"String": "first"}, {"String": "second"}]}},
        "temporal_scope": {"valid_from": None, "valid_to": None},
        "authority": "finance:authoritative", "provenance": "test", "evidence_refs": ["e1"],
    }


class FakeBackend:
    def __init__(self, resources, relations, incidence):
        self.resources, self.relations, self.incidence = resources, relations, incidence
        self.neighbor_calls = []
        self.raw_incidence = [
            (relation_id, ordinal, role, resource, source, generation)
            for (source, generation, relation_id), (_, participants) in relations.items()
            for ordinal, role, resource in participants
        ]

    def neighbors(self, source, generation, resource, role):
        self.neighbor_calls.append((source, generation, resource, role))
        return self.incidence.get((source, generation, resource, role), [])

    def relation(self, source, generation, relation_id):
        return self.relations.get((source, generation, relation_id))

    def resource(self, source, generation, resource):
        return self.resources.get((source, generation, resource))

    def all_ids(self, source, generation):
        return ([rid for s, g, rid in self.resources if (s, g) == (source, generation)],
                [rid for s, g, rid in self.relations if (s, g) == (source, generation)])

    def incidence_rows(self, source, generation):
        return [row for row in self.raw_incidence if row[4:] == (source, generation)]


def resource(rid):
    return {
        "source_id": S, "generation_id": G, "resource_id": rid,
        "owner_id": A, "mapping": {"kind": "Version", "version_id": rid, "document_id": A},
        "temporal": {"resource_ref": rid, "valid_from": None, "valid_to": None,
                     "profile": {"freshness_anchor_at": None, "freshness_basis": None,
                                 "effective_from": None, "effective_to": None}},
    }


def authority():
    return SourceAuthority({S: {rid: {"mapping": resource(rid)["mapping"],
                                       "revision": {"1": "Allowed", "2": "Denied" if rid == C else "Allowed"}}
                                 for rid in (A, B, C)}})


class RefinementTests(unittest.TestCase):
    def setUp(self):
        self.rel = relation()
        self.resources = {(S, G, rid): resource(rid) for rid in (A, B, C)}
        self.relations = {(S, G, R): (self.rel, [(i, p["role"], p["resource_ref"])
                                                  for i, p in enumerate(self.rel["participants"])])}
        self.incidence = {(S, G, p["resource_ref"], p["role"]): [R]
                          for p in self.rel["participants"]}
        self.db = FakeBackend(self.resources, self.relations, self.incidence)
        self.plan = {"seed_nodes": [A], "path_patterns": [{"namespace": "Discovery",
            "relation_type": "loan", "from_role": "borrower", "to_role": "product",
            "from_resource": None, "to_resource": None,
            "required_participants": [{"role": "collateral", "resource_ref": C}]}],
            "allowed_relation_types": ["loan"], "allowed_namespaces": ["Discovery"],
            "authority_requirement": "finance:authoritative", "temporal_context": {"temporal_target":
                [1970, 1, 1, 0, 1, 40, 1, 0, 0]},
            "access_context": "scoped", "expansion_budget": {"max_hops": 2,
                "max_relations": 1, "max_branching_per_node": 1, "max_seed_nodes": 1,
                "max_paths": 1}, "stop_conditions": []}

    def test_corrupted_collateral_incidence_fails_closed(self):
        payload, participants = self.db.relation(S, G, R)
        self.db.relations[(S, G, R)] = payload, participants[:-1]
        with self.assertRaises(IntegrityFailure):
            checked_relation(self.db, S, G, R)

    def test_surplus_raw_incidence_fails_complete_audit(self):
        generation = {"source_id": S, "generation_id": G,
                      "resources": [resource(rid) for rid in (A, B, C)],
                      "relations": [self.rel]}
        self.assertEqual(audit(self.db, generation, authority())["raw_incidence_count"], 3)
        self.db.raw_incidence.append((R, 99, "borrower", B, S, G))
        with self.assertRaisesRegex(IntegrityFailure, "complete persisted raw incidence"):
            audit(self.db, generation, authority())

    def test_scope_and_current_revision_are_reread(self):
        current = authority()
        self.assertEqual(len(traverse(self.db, S, G, self.plan, current)), 1)
        current.set_current_revision(S, "2")
        self.assertEqual(traverse(self.db, S, G, self.plan, current), [])
        self.assertEqual(self.db.neighbor_calls[0][:2], (S, G))

    def test_owner_mapping_spoof_rejected(self):
        wrong = copy.deepcopy(resource(B))
        wrong["mapping"]["document_id"] = B
        with self.assertRaises(IntegrityFailure):
            authority().validate_stage(S, [wrong], "PersistentDiscoveryMetadata")
        with self.assertRaises(IntegrityFailure):
            authority().validate_stage(S, [resource(B)], "SessionOnly")

    def test_digest_excludes_generation_but_keeps_typed_list_order(self):
        first = canonical_digest(S, [resource(A)], [self.rel])
        second = copy.deepcopy(self.rel)
        second["qualifiers"]["ordered"]["List"].reverse()
        self.assertNotEqual(first, canonical_digest(S, [resource(A)], [second]))
        copied = resource(A)
        copied["generation_id"] = "00000000-0000-0000-0000-000000000065"
        self.assertEqual(first, canonical_digest(S, [copied], [self.rel]))

    def test_negative_offset_nanoseconds_and_empty_basis_are_distinct(self):
        negative = [1969, 365, 19, 31, 40, 1, -4, -30, 0]
        self.assertEqual(stamp_ns(negative), 100_000_000_001)
        original = resource(A)
        empty = copy.deepcopy(original)
        empty["temporal"]["profile"]["freshness_basis"] = ""
        self.assertNotEqual(canonical_digest(S, [original], [self.rel]),
                            canonical_digest(S, [empty], [self.rel]))


class OracleAlgorithmPreflight(unittest.TestCase):
    def test_fixture_backed_algorithm_matches_real_memory_oracle(self):
        fixture = json.loads((Path(__file__).resolve().parent.parent / "refinement-fixture.json").read_text())
        resources, relations, incidence = {}, {}, {}
        for generation in fixture["generations"]:
            source, gid = generation["source_id"], generation["generation_id"]
            for row in generation["resources"]:
                resources[(source, gid, row["resource_id"])] = row
            for relation in generation["relations"]:
                rid = relation["relation_id"]
                stored = []
                for ordinal, participant in enumerate(relation["participants"]):
                    role, resource_id = participant["role"], participant["resource_ref"]
                    stored.append((ordinal, role, resource_id))
                    incidence.setdefault((source, gid, resource_id, role), []).append(rid)
                relations[(source, gid, rid)] = relation, stored
        for values in incidence.values():
            values.sort()
        backend = FakeBackend(resources, relations, incidence)
        for scenario in fixture["scenarios"]:
            with self.subTest(scenario=scenario["name"]):
                current = SourceAuthority(fixture["authority"])
                current.set_current_revision(scenario["source_id"], scenario["revision"])
                actual = traverse(backend, scenario["source_id"], scenario["generation_id"],
                                  scenario["plan"], current)
                self.assertEqual(actual, scenario["expected"])


if __name__ == "__main__":
    unittest.main()
