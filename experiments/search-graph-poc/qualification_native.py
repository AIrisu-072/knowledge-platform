"""Independent native reader handles for P3-P04 timed frontiers.

Neither constructor initializes or deletes state. Staging remains with the
audited refined backends on an isolated, empty disposable candidate store.
"""

import base64
import copy
import http.client
import json
import time
import uuid

import psycopg
from psycopg.types.json import Jsonb

from bench import nanos
from refinement import IntegrityFailure
from refinement_backends import RefinedNeo, RefinedPg


WRITER_GID = "00000000-0000-0000-0000-0000000000c8"


def stage_source_authority(stager, fixture, backend):
    """Persist Source policy and scenario current-revision snapshots in candidate DB."""
    policies = [(source, resource, policy)
                for source, entries in fixture["authority"].items()
                for resource, policy in entries.items()]
    revisions = [(case["source_id"], case["name"], case["revision"])
                 for case in fixture["scenarios"]]
    if backend == "pg":
        stager.conn.execute(
            "CREATE TABLE p3_poc.source_policy (source_id uuid, resource_id uuid, "
            "policy jsonb NOT NULL, PRIMARY KEY(source_id,resource_id))")
        stager.conn.execute(
            "CREATE TABLE p3_poc.source_revision (source_id uuid, scenario text, "
            "revision text NOT NULL, PRIMARY KEY(source_id,scenario))")
        with stager.conn.transaction():
            with stager.conn.cursor() as cur:
                cur.executemany("INSERT INTO p3_poc.source_policy VALUES (%s,%s,%s)",
                                [(s, r, Jsonb(p)) for s, r, p in policies])
                cur.executemany("INSERT INTO p3_poc.source_revision VALUES (%s,%s,%s)",
                                revisions)
    elif backend == "neo":
        stager.run("CREATE CONSTRAINT p3_source_policy_key IF NOT EXISTS "
                   "FOR (n:P3SourcePolicy) REQUIRE n.key IS UNIQUE")
        stager.run("CREATE CONSTRAINT p3_source_revision_key IF NOT EXISTS "
                   "FOR (n:P3SourceRevision) REQUIRE n.key IS UNIQUE")
        for pos in range(0, len(policies), 250):
            rows = [{"key": f"{s}|{r}", "source": s, "resource": r,
                     "payload": json.dumps(p, sort_keys=True)}
                    for s, r, p in policies[pos:pos + 250]]
            stager.run("UNWIND $rows AS x CREATE (n:P3SourcePolicy "
                       "{key:x.key,source:x.source,resource:x.resource,payload:x.payload})",
                       {"rows": rows})
        rows = [{"key": f"{s}|{case}", "source": s,
                 "scenario": case, "revision": revision}
                for s, case, revision in revisions]
        stager.run("UNWIND $rows AS x CREATE (n:P3SourceRevision "
                   "{key:x.key,source:x.source,scenario:x.scenario,revision:x.revision})",
                   {"rows": rows})
    else:
        raise ValueError("Source authority staging backend")
    return {"source_policy_rows": len(policies), "source_revision_rows": len(revisions)}


def writer_variant(relation):
    """Same-ID typed relation replacement moves the two product incidences."""
    value = copy.deepcopy(relation)
    products = [i for i, member in enumerate(value["participants"])
                if member["role"] == "product"]
    if len(products) != 2:
        raise ValueError("writer canary requires two product participants")
    a, b = products
    value["participants"][a], value["participants"][b] = (
        value["participants"][b], value["participants"][a])
    value["qualifiers"]["ordered_terms"]["List"].reverse()
    return value


def prepare_writer_generation(stager, base, *, backend):
    relation = base["relations"][0]
    ids = {p["resource_ref"] for p in relation["participants"]}
    small = {"source_id": base["source_id"], "generation_id": WRITER_GID,
             "resources": [r for r in base["resources"] if r["resource_id"] in ids],
             "relations": [relation]}
    stager.stage_full(small, WRITER_GID)
    if backend == "pg":
        stager.conn.execute("UPDATE p3_poc.generation SET state='BUILDING' "
                            "WHERE source_id=%s AND generation_id=%s",
                            (base["source_id"], WRITER_GID))
    return relation


class PgReader(RefinedPg):
    def __init__(self, port, dbname="p3poc"):
        self.conn = psycopg.connect(host="127.0.0.1", port=port, dbname=dbname,
                                    user="postgres", password="p3syntheticpass")
        self.conn.autocommit = True

    def fetch_frontier(self, source, generation, resources, role):
        if not resources:
            return []
        rows = self.conn.execute(
            "SELECT r.relation_id::text,r.payload,p.ordinal,p.role,p.resource_id::text "
            "FROM p3_poc.participant seed "
            "JOIN p3_poc.relation r USING(source_id,generation_id,relation_id) "
            "JOIN p3_poc.participant p USING(source_id,generation_id,relation_id) "
            "JOIN p3_poc.generation g USING(source_id,generation_id) "
            "WHERE seed.source_id=%s AND seed.generation_id=%s "
            "AND seed.resource_id=ANY(%s::uuid[]) AND seed.role=%s "
            "AND g.state='READY' ORDER BY r.relation_id,p.ordinal",
            (source, generation, [uuid.UUID(r) for r in resources], role)).fetchall()
        by_id = {}
        for rid, payload, ordinal, member_role, member_id in rows:
            if rid not in by_id:
                by_id[rid] = (payload, [])
            elif by_id[rid][0] != payload:
                raise ValueError("native PostgreSQL relation payload changed within statement")
            member = (ordinal, member_role, member_id)
            if member not in by_id[rid][1]:
                by_id[rid][1].append(member)
        return list(by_id.values())

    def authority_decision(self, source, scenario, resource):
        row = self.conn.execute(
            "SELECT r.revision,p.policy FROM p3_poc.source_revision r "
            "JOIN p3_poc.source_policy p USING(source_id) "
            "WHERE r.source_id=%s AND r.scenario=%s AND p.resource_id=%s",
            (source, scenario, resource)).fetchone()
        if row is None:
            raise IntegrityFailure("PostgreSQL Source policy/revision absent")
        revision, policy = row
        return revision, policy["mapping"], policy["revision"].get(revision, "Unknown")

    def bulk_authority(self):
        policies = [tuple(row) for row in self.conn.execute(
            "SELECT source_id::text,resource_id::text,policy FROM p3_poc.source_policy "
            "ORDER BY source_id,resource_id").fetchall()]
        revisions = [tuple(row) for row in self.conn.execute(
            "SELECT source_id::text,scenario,revision FROM p3_poc.source_revision "
            "ORDER BY source_id,scenario").fetchall()]
        return policies, revisions

    def bulk_rows(self, source, generation):
        resources = []
        for row in self.conn.execute(
                "SELECT payload,owner_id::text,valid_from_ns,valid_from_offset,"
                "valid_to_ns,valid_to_offset,freshness_anchor_ns,freshness_anchor_offset,"
                "freshness_basis,effective_from_ns,effective_from_offset,effective_to_ns,"
                "effective_to_offset FROM p3_poc.resource WHERE source_id=%s AND generation_id=%s",
                (source, generation)).fetchall():
            payload, owner, *columns = row
            temporal = payload["temporal"]
            profile = temporal["profile"]
            stamps = [temporal["valid_from"], temporal["valid_to"],
                      profile["freshness_anchor_at"], profile["effective_from"],
                      profile["effective_to"]]
            expected = []
            for stamp in stamps[:3]:
                expected.extend((nanos(stamp), None if stamp is None else
                                 stamp[6] * 3600 + stamp[7] * 60 + stamp[8]))
            expected.append(profile["freshness_basis"])
            for stamp in stamps[3:]:
                expected.extend((nanos(stamp), None if stamp is None else
                                 stamp[6] * 3600 + stamp[7] * 60 + stamp[8]))
            observed = [None if value is None else
                        int(value) if i != 6 else value for i, value in enumerate(columns)]
            if owner != payload["owner_id"] or observed != expected:
                raise IntegrityFailure("PostgreSQL native temporal/owner column mismatch")
            resources.append(payload)
        relations = [row[0] for row in self.conn.execute(
            "SELECT payload FROM p3_poc.relation WHERE source_id=%s AND generation_id=%s",
            (source, generation)).fetchall()]
        incidence = self.incidence_rows(source, generation)
        return resources, relations, incidence

    def close(self):
        self.conn.close()


class NeoReader(RefinedNeo):
    transport = "Neo4j Community Query API v2 HTTP persistent connection"

    def __init__(self, port):
        self.connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
        self.auth = "Basic " + base64.b64encode(b"neo4j:p3syntheticpass").decode()

    def run(self, statement, parameters=None):
        body = json.dumps({"statement": statement, "parameters": parameters or {}}).encode()
        self.connection.request("POST", "/db/neo4j/query/v2", body=body,
                                headers={"Authorization": self.auth,
                                         "Content-Type": "application/json"})
        response = self.connection.getresponse()
        raw = response.read()
        if response.status != 202:
            raise RuntimeError(f"Neo4j Query API HTTP {response.status}: {raw[:500]!r}")
        value = json.loads(raw)
        if value.get("errors"):
            raise RuntimeError(f"Neo4j Query API errors: {value['errors']}")
        return value.get("data", {}).get("values", [])

    def fetch_frontier(self, source, generation, resources, role):
        if not resources:
            return []
        rows = self.run(
            "MATCH (seed:P3Resource) WHERE seed.key IN $keys "
            "MATCH (r:P3Relation)-[:P3_PARTICIPANT {role:$role}]->(seed) "
            "MATCH (r)-[p:P3_PARTICIPANT]->(n:P3Resource) "
            "WHERE r.source=$source AND r.generation=$generation "
            "RETURN r.id,r.payload,p.ordinal,p.role,n.id ORDER BY r.id,p.ordinal",
            {"source": source, "generation": generation, "role": role,
             "keys": [self.key(source, generation, r) for r in resources]})
        by_id = {}
        for rid, payload_raw, ordinal, member_role, member_id in rows:
            payload = json.loads(payload_raw)
            if rid not in by_id:
                by_id[rid] = (payload, [])
            elif by_id[rid][0] != payload:
                raise ValueError("native Neo4j relation payload changed within query")
            member = (ordinal, member_role, member_id)
            if member not in by_id[rid][1]:
                by_id[rid][1].append(member)
        return list(by_id.values())

    def authority_decision(self, source, scenario, resource):
        rows = self.run(
            "MATCH (p:P3SourcePolicy {key:$policy_key}) "
            "MATCH (r:P3SourceRevision {key:$revision_key}) "
            "RETURN p.payload,r.revision",
            {"policy_key": f"{source}|{resource}",
             "revision_key": f"{source}|{scenario}"})
        if len(rows) != 1:
            raise IntegrityFailure("Neo4j Source policy/revision absent")
        policy = json.loads(rows[0][0])
        revision = rows[0][1]
        return revision, policy["mapping"], policy["revision"].get(revision, "Unknown")

    def bulk_authority(self):
        def page(statement):
            result = []
            offset = 0
            while True:
                block = self.run(statement + " SKIP $skip LIMIT $limit",
                                 {"skip": offset, "limit": 500})
                result.extend(block)
                if len(block) < 500:
                    return result
                offset += len(block)
        policies = [(s, r, json.loads(raw)) for s, r, raw in page(
            "MATCH (p:P3SourcePolicy) RETURN p.source,p.resource,p.payload "
            "ORDER BY p.source,p.resource")]
        revisions = [tuple(row) for row in page(
            "MATCH (r:P3SourceRevision) RETURN r.source,r.scenario,r.revision "
            "ORDER BY r.source,r.scenario")]
        return policies, revisions

    def bulk_rows(self, source, generation):
        def page(statement):
            values = []
            offset = 0
            while True:
                block = self.run(statement + " SKIP $skip LIMIT $limit",
                                 {"source": source, "generation": generation,
                                  "skip": offset, "limit": 500})
                values.extend(block)
                if len(block) < 500:
                    break
                offset += len(block)
            return values

        resources = [json.loads(row[0]) for row in page(
            "MATCH (n:P3Resource {source:$source,generation:$generation}) "
            "RETURN n.payload ORDER BY n.id")]
        relations = [json.loads(row[0]) for row in page(
            "MATCH (n:P3Relation {source:$source,generation:$generation}) "
            "RETURN n.payload ORDER BY n.id")]
        incidence = [tuple(row) for row in page(
            "MATCH (r:P3Relation {source:$source,generation:$generation})"
            "-[p:P3_PARTICIPANT]->(n:P3Resource) "
            "RETURN r.id,p.ordinal,p.role,n.id,n.source,n.generation "
            "ORDER BY r.id,p.ordinal")]
        return resources, relations, incidence

    def close(self):
        self.connection.close()


def pg_incremental_writer(port, source, original):
    variant = writer_variant(original)
    relation_id = original["relation_id"]

    def run(stop, started):
        reader = PgReader(port)
        results = []
        changed = False
        try:
            while not stop.is_set():
                replacement = variant if not changed else original
                start = time.perf_counter_ns()
                with reader.conn.transaction():
                    reader.conn.execute(
                        "UPDATE p3_poc.relation SET payload=%s WHERE source_id=%s "
                        "AND generation_id=%s AND relation_id=%s",
                        (Jsonb(replacement), source, WRITER_GID, relation_id))
                    for ordinal, member in enumerate(replacement["participants"]):
                        reader.conn.execute(
                            "UPDATE p3_poc.participant SET resource_id=%s WHERE source_id=%s "
                            "AND generation_id=%s AND relation_id=%s AND ordinal=%s",
                            (member["resource_ref"], source, WRITER_GID, relation_id, ordinal))
                results.append({"outcome": "committed", "duration_ns":
                                time.perf_counter_ns() - start, "relation_id": relation_id})
                started.set()
                changed = not changed
                stop.wait(0.005)
        finally:
            reader.close()
        return results

    return run


def neo_incremental_writer(port, source, original):
    variant = writer_variant(original)
    relation_id = original["relation_id"]
    key = RefinedNeo.key(source, WRITER_GID, relation_id)
    statement = (
        "MATCH (r:P3Relation {key:$key})-[p1:P3_PARTICIPANT {ordinal:1}]->(n1:P3Resource) "
        "MATCH (r)-[p2:P3_PARTICIPANT {ordinal:2}]->(n2:P3Resource) "
        "DELETE p1,p2 "
        "CREATE (r)-[:P3_PARTICIPANT {role:'product',ordinal:1}]->(n2) "
        "CREATE (r)-[:P3_PARTICIPANT {role:'product',ordinal:2}]->(n1) "
        "SET r.payload=$payload RETURN r.id"
    )

    def run(stop, started):
        reader = NeoReader(port)
        results = []
        changed = False
        try:
            while not stop.is_set():
                replacement = variant if not changed else original
                start = time.perf_counter_ns()
                rows = reader.run(statement, {"key": key,
                                              "payload": json.dumps(replacement, sort_keys=True)})
                if rows != [[relation_id]]:
                    raise RuntimeError("Neo4j incremental writer relation disappeared")
                results.append({"outcome": "committed", "duration_ns":
                                time.perf_counter_ns() - start, "relation_id": relation_id})
                started.set()
                changed = not changed
                stop.wait(0.005)
        finally:
            reader.close()
        return results

    return run
