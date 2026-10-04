"""Independent traversal over each candidate's native persisted incidence."""

import copy
import datetime as dt
import hashlib
import json
import uuid


class IntegrityFailure(Exception):
    pass


class BudgetFailure(Exception):
    pass


def canonical_digest(source, resources, relations):
    """Versioned logical PoC digest; generation and build time are excluded."""
    h = hashlib.sha256()
    for value in ("p3-poc:logical-v1", source):
        raw = json.dumps(value).encode()
        h.update(len(raw).to_bytes(8, "big") + raw)
    for row in sorted(resources, key=lambda x: x["resource_id"]):
        logical = {k: v for k, v in row.items() if k not in ("generation_id", "built_at")}
        raw = json.dumps(logical, sort_keys=True, separators=(",", ":")).encode()
        h.update(len(raw).to_bytes(8, "big") + raw)
    for row in sorted(relations, key=lambda x: x["relation_id"]):
        logical = copy.deepcopy(row)
        logical["participants"].sort(key=lambda p: (p["role"], p["resource_ref"]))
        logical["evidence_refs"] = sorted(set(logical["evidence_refs"]))
        raw = json.dumps(logical, sort_keys=True, separators=(",", ":")).encode()
        h.update(len(raw).to_bytes(8, "big") + raw)
    return h.hexdigest()


def stamp_ns(value):
    if value is None:
        return None
    year, ordinal, hour, minute, second, nano, oh, om, os = value
    local = dt.datetime(year, 1, 1) + dt.timedelta(days=ordinal - 1, hours=hour,
                                                   minutes=minute, seconds=second)
    utc = local - dt.timedelta(hours=oh, minutes=om, seconds=os)
    delta = utc - dt.datetime(1970, 1, 1)
    return (delta.days * 86400 + delta.seconds) * 1_000_000_000 + nano


def interval(start, end, target):
    return (start is None or target >= stamp_ns(start)) and (end is None or target < stamp_ns(end))


class SourceAuthority:
    """Mutable Source fixture; decisions are looked up by Source and resource per query."""

    def __init__(self, entries):
        self.entries = entries
        self.calls = []
        self.current_revision = {source: "1" for source in entries}

    def set_current_revision(self, source, revision):
        if source not in self.entries:
            raise IntegrityFailure("unknown Source revision")
        self.current_revision[source] = str(revision)

    def validate_stage(self, source, resources, retention):
        if retention not in ("PersistentResource", "PersistentDiscoveryMetadata"):
            raise IntegrityFailure("retention forbids durable stage")
        seen = set()
        for row in resources:
            rid = row["resource_id"]
            policy = self.entries.get(source, {}).get(rid)
            if rid in seen or row["source_id"] != source or policy is None:
                raise IntegrityFailure("resource Source or identity mismatch")
            seen.add(rid)
            mapping = row.get("mapping")
            if mapping != policy["mapping"] or row["owner_id"] != mapping.get("document_id"):
                raise IntegrityFailure("Source owner mapping mismatch")
            if mapping["kind"] == "Version" and mapping.get("version_id") != rid:
                raise IntegrityFailure("Version identity mismatch")
            if mapping["kind"] == "Document" and rid != document_resource_id(
                    source, mapping["document_id"]):
                raise IntegrityFailure("Document identity mismatch")
            if mapping["kind"] == "FolderPlacement" and not mapping.get("folder_id"):
                raise IntegrityFailure("Folder mapping missing")
            if mapping["kind"] == "FolderPlacement" and rid != folder_resource_id(
                    source, mapping["document_id"], mapping["folder_id"]):
                raise IntegrityFailure("Folder identity mismatch")
            if mapping["kind"] not in ("Version", "Document", "FolderPlacement"):
                raise IntegrityFailure("unregistered Source kind")

    def decision(self, source, row):
        rid = row["resource_id"]
        revision = self.current_revision.get(source)
        self.calls.append((source, rid, str(revision)))
        policy = self.entries.get(source, {}).get(rid)
        if policy is None or row.get("mapping") != policy["mapping"]:
            raise IntegrityFailure("persisted mapping no longer matches Source")
        return policy["revision"].get(str(revision), "Unknown") == "Allowed"


def checked_relation(backend, source, generation, relation_id):
    """Fetch all persisted participants and require bidirectional index consistency."""
    pair = backend.relation(source, generation, relation_id)
    if pair is None:
        raise IntegrityFailure("dangling incidence")
    payload, stored = pair
    if payload["relation_id"] != relation_id or len(stored) < 2:
        raise IntegrityFailure("relation identity or arity mismatch")
    if sorted(i for i, _, _ in stored) != list(range(len(stored))):
        raise IntegrityFailure("participant ordinal mismatch")
    members = [{"role": role, "resource_ref": rid} for _, role, rid in sorted(stored)]
    order = lambda p: (p["role"], p["resource_ref"])
    if sorted(members, key=order) != sorted(payload["participants"], key=order):
        raise IntegrityFailure("persisted participants disagree with payload")
    if len(members) != len({(p["role"], p["resource_ref"]) for p in members}):
        raise IntegrityFailure("duplicate participant")
    for p in members:
        if relation_id not in backend.neighbors(source, generation, p["resource_ref"], p["role"]):
            raise IntegrityFailure("reverse incidence missing")
        if backend.resource(source, generation, p["resource_ref"]) is None:
            raise IntegrityFailure("participant resource missing")
    return payload


def traverse(backend, source, generation, plan, authority):
    budget = plan["expansion_budget"]
    patterns = plan["path_patterns"]
    if (not patterns or not plan["seed_nodes"] or plan["stop_conditions"]
        or len(patterns) > budget["max_hops"]
        or len(plan["seed_nodes"]) > budget["max_seed_nodes"]):
        raise BudgetFailure("invalid plan")
    target = stamp_ns(plan["temporal_context"]["temporal_target"])
    cache = {}

    def visible(rid):
        if rid in cache:
            return cache[rid]
        row = backend.resource(source, generation, rid)
        if row is None:
            cache[rid] = False
            return False
        t = row["temporal"]
        p = t["profile"]
        if not interval(t["valid_from"], t["valid_to"], target) or not interval(
                p["effective_from"], p["effective_to"], target):
            cache[rid] = False
            return False
        cache[rid] = authority.decision(source, row)
        return cache[rid]

    paths = [(seed, [seed], []) for seed in sorted(set(plan["seed_nodes"])) if visible(seed)]
    if len(paths) > budget["max_paths"]:
        raise BudgetFailure("seed paths")
    expansions = 0
    for pattern in patterns:
        next_paths = []
        for current, resources, steps in paths:
            branches = []
            local_expansions = 0
            for rid in backend.neighbors(source, generation, current, pattern["from_role"]):
                relation = checked_relation(backend, source, generation, rid)
                members = relation["participants"]
                if (relation["namespace"] not in plan["allowed_namespaces"]
                    or relation["relation_type"] not in plan["allowed_relation_types"]
                    or relation["namespace"] != pattern["namespace"]
                    or relation["relation_type"] != pattern["relation_type"]
                    or (plan["authority_requirement"] is not None
                        and relation["authority"] != plan["authority_requirement"])
                    or not interval(relation["temporal_scope"]["valid_from"],
                                    relation["temporal_scope"]["valid_to"], target)
                    or (pattern["from_resource"] is not None and pattern["from_resource"] != current)
                    or {"role": pattern["from_role"], "resource_ref": current} not in members
                    or any(p not in members for p in pattern["required_participants"])):
                    continue
                if not all(visible(p["resource_ref"]) for p in members):
                    continue
                before = len(branches)
                for member in members:
                    if member["role"] != pattern["to_role"] or (
                        pattern["to_resource"] is not None
                        and member["resource_ref"] != pattern["to_resource"]):
                        continue
                    if len(branches) + 1 > budget["max_branching_per_node"]:
                        raise BudgetFailure("branching")
                    if len(next_paths) + len(branches) + 1 > budget["max_paths"]:
                        raise BudgetFailure("paths")
                    step = {"relation_id": rid, "namespace": relation["namespace"],
                            "relation_type": relation["relation_type"],
                            "from_role": pattern["from_role"], "from_resource": current,
                            "to_role": pattern["to_role"], "to_resource": member["resource_ref"],
                            "participants": sorted(members, key=lambda p: (p["role"], p["resource_ref"])),
                            "evidence_refs": sorted(set(relation["evidence_refs"])),
                            "provenance": relation["provenance"]}
                    branches.append((member["resource_ref"], resources + [member["resource_ref"]], steps + [step]))
                if len(branches) > before:
                    local_expansions += 1
                    if expansions + local_expansions > budget["max_relations"]:
                        raise BudgetFailure("relations")
            expansions += local_expansions
            next_paths.extend(branches)
        paths = next_paths
    grouped = {}
    for rid, resources, steps in paths:
        grouped.setdefault(rid, []).append({"resource_path": resources, "steps": steps})
    return [{"target": rid, "paths": [json.loads(p) for p in sorted({
                json.dumps(p, sort_keys=True) for p in paths})]}
            for rid, paths in sorted(grouped.items())]


def _derived_resource_id(source, kind, *native):
    h = hashlib.sha256()
    for part in (b"search-source-document:resource:v1", uuid.UUID(source).bytes,
                 kind.encode(), *(uuid.UUID(value).bytes for value in native)):
        h.update(len(part).to_bytes(8, "big") + part)
    return str(uuid.UUID(bytes=h.digest()[:16]))


def document_resource_id(source, document):
    return _derived_resource_id(source, "document", document)


def folder_resource_id(source, document, folder):
    return _derived_resource_id(source, "folder_placement", document, folder)
