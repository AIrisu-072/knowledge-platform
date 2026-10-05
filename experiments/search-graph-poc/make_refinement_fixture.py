"""Derive a named semantic corpus from frozen fixture-100 without changing it."""

import argparse
import copy
import hashlib
import json
from datetime import datetime, timedelta
from pathlib import Path

from refinement import document_resource_id, folder_resource_id


HERE = Path(__file__).resolve().parent
ORIGINAL = HERE / "fixture-100.json"


def uid(value):
    return f"00000000-0000-0000-0000-{value:012x}"


def stamp(utc_ns, offset_minutes):
    seconds, ns = divmod(utc_ns, 1_000_000_000)
    local = datetime(1970, 1, 1) + timedelta(seconds=seconds, minutes=offset_minutes)
    sign = -1 if offset_minutes < 0 else 1
    hours, minutes = divmod(abs(offset_minutes), 60)
    return [local.year, local.timetuple().tm_yday, local.hour, local.minute,
            local.second, ns, sign * hours, sign * minutes, 0]


def plan(seed, collateral, *, hops=1, tight=False):
    first = {"namespace": "Discovery", "relation_type": "loan", "from_role": "borrower",
             "to_role": "product", "from_resource": None, "to_resource": None,
             "required_participants": [{"role": "collateral", "resource_ref": collateral}]}
    steps = [first]
    if hops == 2:
        steps.append({**copy.deepcopy(first), "required_participants": [
            {"role": "collateral", "resource_ref": uid(10023)}]})
    return {"seed_nodes": [seed], "path_patterns": steps, "allowed_relation_types": ["loan"],
            "allowed_namespaces": ["Discovery"], "authority_requirement": "finance:authoritative",
            "temporal_context": {"evaluation_id": uid(9), "evaluated_at": stamp(100_000_000_001, 0),
                                 "temporal_target": stamp(100_000_000_001, 0),
                                 "business_timezone": "UTC"},
            "access_context": "p3-refinement:trusted", "expansion_budget": {
                "max_hops": hops, "max_relations": 1 if tight else 16,
                "max_branching_per_node": 2 if tight else 4, "max_seed_nodes": 1,
                "max_paths": 2 if tight else 8}, "stop_conditions": []}


def add_mappings(generation):
    policies = {}
    for row in generation["resources"]:
        rid = row["resource_id"]
        mapping = {"kind": "Version", "version_id": rid,
                   "document_id": row["owner_id"]}
        row["mapping"] = mapping
        policies[rid] = {"mapping": mapping,
                         "revision": {"1": "Allowed", "2": "Allowed",
                                      "3": "Unknown", "4": "Error"}}
    return policies


def add_structural(generation, policies):
    source, gid = generation["source_id"], generation["generation_id"]
    doc, folder, version = uid(90001), uid(90002), uid(90003)
    doc_rid = document_resource_id(source, doc)
    folder_rid = folder_resource_id(source, doc, folder)
    template = generation["resources"][0]
    for rid, mapping in [
        (doc_rid, {"kind": "Document", "document_id": doc}),
        (folder_rid, {"kind": "FolderPlacement", "document_id": doc, "folder_id": folder}),
        (version, {"kind": "Version", "document_id": doc, "version_id": version}),
    ]:
        row = copy.deepcopy(template)
        row.update(resource_id=rid, owner_id=doc, mapping=mapping)
        row["temporal"]["resource_ref"] = rid
        generation["resources"].append(row)
        policies[rid] = {"mapping": mapping,
                         "revision": {"1": "Allowed", "2": "Allowed",
                                      "3": "Unknown", "4": "Error"}}
    relation = copy.deepcopy(generation["relations"][0])
    relation["relation_id"] = uid(3_000_001)
    relation["participants"] = [
        {"role": "borrower", "resource_ref": doc_rid},
        {"role": "product", "resource_ref": version},
        {"role": "collateral", "resource_ref": folder_rid},
    ]
    relation["provenance"] = "synthetic-structural-mapping"
    generation["relations"].append(relation)
    return doc_rid, folder_rid


def add_semantics(generation, policies, updated):
    source = generation["source_id"]
    # Complete nanosecond/negative offset/None-vs-empty temporal fields.
    for pos in (1, 2, 3):
        row = generation["resources"][4 * 4 + pos]
        if pos == 1:
            row["temporal"]["valid_from"] = stamp(100_000_000_000, -270)
            row["temporal"]["profile"].update(
                freshness_anchor_at=stamp(99_123_456_789, -270), freshness_basis="",
                effective_from=stamp(100_000_000_000, -270),
                effective_to=stamp(100_000_000_002, -270))
        elif pos == 3:
            row["temporal"]["valid_to"] = stamp(100_000_000_002, -270)
    # One visible second hop with a repeated role in the first relation.
    second = copy.deepcopy(generation["relations"][5])
    second["relation_id"] = uid(3_000_000)
    second["participants"][0]["resource_ref"] = uid(10001)
    second["provenance"] = "synthetic-second-hop"
    generation["relations"].append(second)
    # 128 hidden relations, 5 participants each: 640 hidden incidences.
    hidden = uid(10027)
    policies[hidden]["revision"] = {str(i): "Denied" for i in range(1, 5)}
    for number in range(128):
        rel = copy.deepcopy(generation["relations"][0])
        rel["relation_id"] = uid(100_000 + number)
        rel["participants"].append({"role": "observer", "resource_ref": hidden})
        rel["provenance"] = f"synthetic-hidden-degree-{number}"
        generation["relations"].append(rel)
    if updated:
        first = next(r for r in generation["relations"] if r["relation_id"] == uid(1_000_000))
        first["participants"][1]["resource_ref"] = uid(10017)
        # This same-ID mutation changes both incidence and typed List order.
    return hidden


def make():
    raw = ORIGINAL.read_bytes()
    old = json.loads(raw)
    gens = [copy.deepcopy(old[name]) for name in ("baseline", "updated", "cross_source")]
    policy = {}
    for gen in gens:
        # Original 1-hop query expectations are historical and no longer apply
        # after the named participant/delta additions below.
        gen["queries"] = []
        current = add_mappings(gen)
        policy[gen["source_id"]] = current
        if gen is not gens[2]:
            add_structural(gen, current)
            add_semantics(gen, current, gen is gens[1])
    # Same bare ResourceId has a different Source decision.
    source2 = gens[2]["source_id"]
    policy[source2][uid(10003)]["revision"]["1"] = "Denied"
    source1 = gens[0]["source_id"]
    policy[source1][uid(10003)]["revision"]["2"] = "Denied"
    cases = []
    def case(name, gen, query, revision="1"):
        cases.append({"name": name, "source_id": gen["source_id"],
                      "generation_id": gen["generation_id"], "revision": revision,
                      "plan": query, "expected": None, "expected_status": None})
    base, updated, cross = gens
    case("nary-repeated-role", base, plan(uid(10000), uid(10003)))
    case("false-composite", base, plan(uid(10000), uid(10007)))
    case("two-hop", base, plan(uid(10000), uid(10003), hops=2))
    case("hidden-degree-visible-budget", base, plan(uid(10000), uid(10003), tight=True))
    case("negative-offset-nanosecond", base, plan(uid(10016), uid(10019)))
    case("positive-offset", base, plan(uid(10008), uid(10011)))
    case("half-open-boundary", base, plan(uid(10012), uid(10015)))
    case("source-one-before-revocation", base, plan(uid(10000), uid(10003)))
    case("source-one-revoked", base, plan(uid(10000), uid(10003)), "2")
    case("source-one-unknown", base, plan(uid(10000), uid(10003)), "3")
    case("source-one-error", base, plan(uid(10000), uid(10003)), "4")
    case("source-two-same-bare-id-denied", cross, plan(uid(10000), uid(10003)))
    case("structural-document-folder-version", base,
         plan(document_resource_id(source1, uid(90001)),
              folder_resource_id(source1, uid(90001), uid(90002))))
    case("same-id-participant-qualifier-update", updated, plan(uid(10000), uid(10003)))
    case("relation-only-add", updated, plan(uid(10000), uid(10011)))
    case("relation-only-delete", updated, plan(uid(10004), uid(10007)))
    return {"format": "p3-refinement-v1", "base_fixture_sha256": hashlib.sha256(raw).hexdigest(),
            "seed": old["seed"], "retention": "PersistentDiscoveryMetadata",
            "generations": gens, "authority": policy, "scenarios": cases}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_text(json.dumps(make(), sort_keys=True, separators=(",", ":")))
    print(args.output, hashlib.sha256(args.output.read_bytes()).hexdigest())
