#!/usr/bin/env python3
"""Isolated one-hop typed n-ary incidence comparison against the real Rust oracle."""

import argparse
import base64
import datetime as dt
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

import psycopg
from psycopg.types.json import Jsonb

TARGET_NS = 100_000_000_001


def nanos(value):
    if value is None:
        return None
    year, ordinal, hour, minute, second, nano, offset_h, offset_m, offset_s = value
    local = dt.datetime(year, 1, 1) + dt.timedelta(
        days=ordinal - 1, hours=hour, minutes=minute, seconds=second
    )
    local -= dt.timedelta(hours=offset_h, minutes=offset_m, seconds=offset_s)
    epoch = dt.datetime(1970, 1, 1)
    delta = local - epoch
    return (delta.days * 86400 + delta.seconds) * 1_000_000_000 + nano


def valid_interval(start, end):
    return (start is None or TARGET_NS >= nanos(start)) and (
        end is None or TARGET_NS < nanos(end)
    )


def visible(resource, denied):
    if resource is None or resource["resource_id"] == denied:
        return False
    temporal = resource["temporal"]
    return valid_interval(temporal["valid_from"], temporal["valid_to"]) and valid_interval(
        temporal["profile"]["effective_from"], temporal["profile"]["effective_to"]
    )


def signatures(relation_rows, get_resource, query):
    if not visible(get_resource(query["seed"]), query["denied"]):
        return []
    found = []
    for relation in relation_rows:
        participants = relation["participants"]
        if relation["namespace"] != "Discovery" or relation["relation_type"] != "loan":
            continue
        if relation["authority"] != "finance:authoritative":
            continue
        if not valid_interval(
            relation["temporal_scope"]["valid_from"],
            relation["temporal_scope"]["valid_to"],
        ):
            continue
        if not any(
            p["role"] == "borrower" and p["resource_ref"] == query["seed"]
            for p in participants
        ):
            continue
        if not any(
            p["role"] == "collateral"
            and p["resource_ref"] == query["required_collateral"]
            for p in participants
        ):
            continue
        if not all(visible(get_resource(p["resource_ref"]), query["denied"]) for p in participants):
            continue
        canonical = sorted(participants, key=lambda p: (p["role"], p["resource_ref"]))
        for p in participants:
            if p["role"] == "product":
                found.append(
                    {
                        "target": p["resource_ref"],
                        "relation_id": relation["relation_id"],
                        "participants": canonical,
                        "evidence_refs": sorted(set(relation["evidence_refs"])),
                        "provenance": relation["provenance"],
                    }
                )
    return sorted(found, key=lambda p: (p["target"], p["relation_id"]))


def pct(samples, n):
    data = sorted(samples)
    return data[(len(data) - 1) * n // 100]


class Pg:
    name = "postgresql-18.6"

    def __init__(self, port):
        self.conn = psycopg.connect(
            host="127.0.0.1", port=port, dbname="p3poc", user="postgres", password="p3syntheticpass"
        )
        self.conn.autocommit = True
        self.schema()

    def schema(self):
        self.conn.execute("DROP SCHEMA IF EXISTS p3_poc CASCADE")
        self.conn.execute("CREATE SCHEMA p3_poc")
        self.conn.execute(
            "CREATE TABLE p3_poc.generation (source_id uuid, generation_id uuid, state text NOT NULL CHECK (state IN ('BUILDING','READY')), PRIMARY KEY(source_id,generation_id))"
        )
        self.conn.execute(
            "CREATE TABLE p3_poc.resource (source_id uuid, generation_id uuid, resource_id uuid, owner_id uuid NOT NULL, valid_from_ns numeric(30,0), valid_from_offset integer, valid_to_ns numeric(30,0), valid_to_offset integer, freshness_anchor_ns numeric(30,0), freshness_anchor_offset integer, freshness_basis text, effective_from_ns numeric(30,0), effective_from_offset integer, effective_to_ns numeric(30,0), effective_to_offset integer, payload jsonb NOT NULL, PRIMARY KEY(source_id,generation_id,resource_id), FOREIGN KEY(source_id,generation_id) REFERENCES p3_poc.generation(source_id,generation_id))"
        )
        self.conn.execute(
            "CREATE TABLE p3_poc.relation (source_id uuid, generation_id uuid, relation_id uuid, namespace text NOT NULL, relation_type text NOT NULL, payload jsonb NOT NULL, PRIMARY KEY(source_id,generation_id,relation_id), FOREIGN KEY(source_id,generation_id) REFERENCES p3_poc.generation(source_id,generation_id))"
        )
        self.conn.execute(
            "CREATE TABLE p3_poc.participant (source_id uuid, generation_id uuid, relation_id uuid, ordinal integer, role text NOT NULL, resource_id uuid NOT NULL, PRIMARY KEY(source_id,generation_id,relation_id,ordinal), FOREIGN KEY(source_id,generation_id,relation_id) REFERENCES p3_poc.relation(source_id,generation_id,relation_id), FOREIGN KEY(source_id,generation_id,resource_id) REFERENCES p3_poc.resource(source_id,generation_id,resource_id))"
        )
        self.conn.execute(
            "CREATE INDEX participant_incidence ON p3_poc.participant(source_id,generation_id,resource_id,role,relation_id)"
        )

    def _resource_args(self, row, generation):
        temporal = row["temporal"]
        profile = temporal["profile"]
        stamps = [
            temporal["valid_from"], temporal["valid_to"], profile["freshness_anchor_at"],
            profile["effective_from"], profile["effective_to"],
        ]
        vals = []
        for stamp in stamps:
            vals.extend([nanos(stamp), None if stamp is None else stamp[6] * 3600 + stamp[7] * 60 + stamp[8]])
        copy = dict(row, generation_id=generation)
        return (row["source_id"], generation, row["resource_id"], row["owner_id"], *vals[:6], profile["freshness_basis"], *vals[6:], Jsonb(copy))

    def stage_full(self, source, generation):
        s, g = source["source_id"], generation
        with self.conn.transaction():
            self.conn.execute("INSERT INTO p3_poc.generation VALUES (%s,%s,'BUILDING')", (s, g))
            with self.conn.cursor() as c:
                c.executemany(
                    "INSERT INTO p3_poc.resource VALUES (" + ",".join(["%s"] * 16) + ")",
                    [self._resource_args(r, g) for r in source["resources"]],
                )
                c.executemany(
                    "INSERT INTO p3_poc.relation VALUES (%s,%s,%s,%s,%s,%s)",
                    [(s, g, r["relation_id"], r["namespace"], r["relation_type"], Jsonb(r)) for r in source["relations"]],
                )
                c.executemany(
                    "INSERT INTO p3_poc.participant VALUES (%s,%s,%s,%s,%s,%s)",
                    [(s, g, r["relation_id"], i, p["role"], p["resource_ref"])
                     for r in source["relations"] for i, p in enumerate(r["participants"])],
                )
            self.conn.execute("UPDATE p3_poc.generation SET state='READY' WHERE source_id=%s AND generation_id=%s", (s, g))

    def stage_incremental(self, baseline, updated):
        s, base, target = baseline["source_id"], baseline["generation_id"], updated["generation_id"]
        old = {r["relation_id"]: r for r in baseline["relations"]}
        new = {r["relation_id"]: r for r in updated["relations"]}
        with self.conn.transaction():
            self.conn.execute("INSERT INTO p3_poc.generation VALUES (%s,%s,'BUILDING')", (s, target))
            self.conn.execute(
                "INSERT INTO p3_poc.resource SELECT source_id,%s,resource_id,owner_id,valid_from_ns,valid_from_offset,valid_to_ns,valid_to_offset,freshness_anchor_ns,freshness_anchor_offset,freshness_basis,effective_from_ns,effective_from_offset,effective_to_ns,effective_to_offset,jsonb_set(payload,'{generation_id}',to_jsonb(%s::text)) FROM p3_poc.resource WHERE source_id=%s AND generation_id=%s",
                (target, target, s, base),
            )
            self.conn.execute(
                "INSERT INTO p3_poc.relation SELECT source_id,%s,relation_id,namespace,relation_type,payload FROM p3_poc.relation WHERE source_id=%s AND generation_id=%s",
                (target, s, base),
            )
            self.conn.execute(
                "INSERT INTO p3_poc.participant SELECT source_id,%s,relation_id,ordinal,role,resource_id FROM p3_poc.participant WHERE source_id=%s AND generation_id=%s",
                (target, s, base),
            )
            for rid in old.keys() - new.keys():
                self.conn.execute("DELETE FROM p3_poc.participant WHERE source_id=%s AND generation_id=%s AND relation_id=%s", (s,target,rid))
                self.conn.execute("DELETE FROM p3_poc.relation WHERE source_id=%s AND generation_id=%s AND relation_id=%s", (s,target,rid))
            for rid in new.keys():
                if rid not in old or new[rid] != old[rid]:
                    self.conn.execute("DELETE FROM p3_poc.participant WHERE source_id=%s AND generation_id=%s AND relation_id=%s", (s,target,rid))
                    self.conn.execute("DELETE FROM p3_poc.relation WHERE source_id=%s AND generation_id=%s AND relation_id=%s", (s,target,rid))
                    r = new[rid]
                    self.conn.execute("INSERT INTO p3_poc.relation VALUES (%s,%s,%s,%s,%s,%s)", (s,target,rid,r["namespace"],r["relation_type"],Jsonb(r)))
                    with self.conn.cursor() as c:
                        c.executemany("INSERT INTO p3_poc.participant VALUES (%s,%s,%s,%s,%s,%s)", [(s,target,rid,i,p["role"],p["resource_ref"]) for i,p in enumerate(r["participants"])])
            self.conn.execute("UPDATE p3_poc.generation SET state='READY' WHERE source_id=%s AND generation_id=%s", (s,target))

    def lookup(self, source, generation, seed):
        rows = self.conn.execute(
            "SELECT r.payload FROM p3_poc.participant p JOIN p3_poc.relation r USING(source_id,generation_id,relation_id) JOIN p3_poc.generation g USING(source_id,generation_id) WHERE p.source_id=%s AND p.generation_id=%s AND p.resource_id=%s AND p.role='borrower' AND g.state='READY' ORDER BY p.relation_id",
            (source, generation, seed),
        ).fetchall()
        return [r[0] for r in rows]

    def resource(self, source, generation, rid):
        row = self.conn.execute("SELECT payload FROM p3_poc.resource WHERE source_id=%s AND generation_id=%s AND resource_id=%s", (source,generation,rid)).fetchone()
        return None if row is None else row[0]

    def size(self):
        return self.conn.execute("SELECT pg_database_size(current_database())").fetchone()[0]

    def close(self):
        self.conn.close()


class Neo:
    name = "neo4j-community-2026.09.0"

    def __init__(self, port):
        self.url = f"http://127.0.0.1:{port}/db/neo4j/query/v2"
        token = base64.b64encode(b"neo4j:p3syntheticpass").decode()
        self.auth = f"Basic {token}"
        for attempt in range(30):
            try:
                self.run("RETURN 1")
                break
            except (urllib.error.URLError, RuntimeError):
                if attempt == 29:
                    raise
                time.sleep(1)
        while True:
            deleted = self.run("MATCH (n) WITH n LIMIT 500 DETACH DELETE n RETURN count(n)")
            if not deleted or deleted[0][0] == 0:
                break
        self.run("CREATE CONSTRAINT p3_resource_key IF NOT EXISTS FOR (n:P3Resource) REQUIRE n.key IS UNIQUE")
        self.run("CREATE CONSTRAINT p3_relation_key IF NOT EXISTS FOR (n:P3Relation) REQUIRE n.key IS UNIQUE")

    def run(self, statement, parameters=None):
        body = json.dumps({"statement": statement, "parameters": parameters or {}}).encode()
        request = urllib.request.Request(self.url, data=body, headers={"Authorization": self.auth, "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                result = json.load(response)
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"Neo4j HTTP {e.code}: {e.read()[:1000]!r}") from e
        if result.get("errors"):
            raise RuntimeError(f"Neo4j query errors: {result['errors']}")
        data = result.get("data", {})
        return data.get("values", [])

    @staticmethod
    def key(source, generation, item):
        return f"{source}|{generation}|{item}"

    def _resource_rows(self, source, generation):
        return [{"key": self.key(source["source_id"], generation, r["resource_id"]), "source":source["source_id"], "generation":generation, "id":r["resource_id"], "payload":json.dumps(dict(r,generation_id=generation),sort_keys=True)} for r in source["resources"]]

    def _relation_rows(self, source, generation):
        return [{"key":self.key(source["source_id"],generation,r["relation_id"]), "source":source["source_id"], "generation":generation, "id":r["relation_id"], "payload":json.dumps(r,sort_keys=True), "participants":[{"key":self.key(source["source_id"],generation,p["resource_ref"]),"role":p["role"],"ordinal":i} for i,p in enumerate(r["participants"])]} for r in source["relations"]]

    def _insert_resources(self, rows):
        for pos in range(0,len(rows),250):
            self.run("UNWIND $rows AS x CREATE (r:P3Resource {key:x.key,source:x.source,generation:x.generation,id:x.id,payload:x.payload})",{"rows":rows[pos:pos+250]})

    def _insert_relations(self, rows):
        for pos in range(0,len(rows),100):
            self.run("UNWIND $rows AS x CREATE (r:P3Relation {key:x.key,source:x.source,generation:x.generation,id:x.id,payload:x.payload}) WITH r,x UNWIND x.participants AS p MATCH (t:P3Resource {key:p.key}) CREATE (r)-[:P3_PARTICIPANT {role:p.role,ordinal:p.ordinal}]->(t)",{"rows":rows[pos:pos+100]})

    def stage_full(self, source, generation):
        self._insert_resources(self._resource_rows(source,generation))
        self._insert_relations(self._relation_rows(source,generation))

    def stage_incremental(self, baseline, updated):
        s, base, target = baseline["source_id"], baseline["generation_id"], updated["generation_id"]
        # O(base rows) copy to a new immutable generation, then a relation-only delta.
        self._insert_resources(self._resource_rows(updated,target))
        old = {r["relation_id"]:r for r in baseline["relations"]}
        new = {r["relation_id"]:r for r in updated["relations"]}
        unchanged = [r for rid,r in old.items() if rid in new and new[rid] == r]
        self._insert_relations(self._relation_rows(dict(updated, relations=unchanged),target))
        changed = [r for rid,r in new.items() if rid not in old or r != old[rid]]
        self._insert_relations(self._relation_rows(dict(updated, relations=changed),target))

    def lookup(self, source, generation, seed):
        rows = self.run("MATCH (r:P3Relation {source:$source,generation:$generation})-[:P3_PARTICIPANT {role:'borrower'}]->(s:P3Resource {key:$key}) RETURN r.payload ORDER BY r.id",{"source":source,"generation":generation,"key":self.key(source,generation,seed)})
        return [json.loads(row[0]) for row in rows]

    def resource(self, source, generation, rid):
        rows = self.run("MATCH (r:P3Resource {key:$key}) RETURN r.payload",{"key":self.key(source,generation,rid)})
        return None if not rows else json.loads(rows[0][0])

    def size(self):
        rows = self.run("MATCH (n) RETURN count(n)")
        return {"nodes":rows[0][0]}

    def close(self):
        pass


def verify_generation(backend, fixture, generation):
    s, g = generation["source_id"], generation["generation_id"]
    for query in generation["queries"]:
        cache = {}
        def get(rid):
            if rid not in cache:
                cache[rid] = backend.resource(s,g,rid)
            return cache[rid]
        actual = signatures(backend.lookup(s,g,query["seed"]),get,query)
        if actual != query["expected"]:
            raise AssertionError(f"{backend.name} {fixture['relation_groups']} {query['name']} expected={query['expected']} actual={actual}")
    if backend.lookup(s,"00000000-0000-0000-0000-000000000099",generation["queries"][0]["seed"]):
        raise AssertionError("wrong generation returned relation")
    for sample in generation["resources"][8:12]:
        if backend.resource(s,g,sample["resource_id"]) != sample:
            raise AssertionError("temporal/owner/source resource roundtrip mismatch")


def benchmark(backend, fixture):
    baseline, updated = fixture["baseline"], fixture["updated"]
    start = time.perf_counter_ns()
    backend.stage_full(baseline,baseline["generation_id"])
    ingest_ms = (time.perf_counter_ns()-start)/1e6
    verify_generation(backend,fixture,baseline)
    s,g,seed = baseline["source_id"],baseline["generation_id"],baseline["queries"][0]["seed"]
    durations=[]
    for _ in range(100):
        now=time.perf_counter_ns()
        relations=backend.lookup(s,g,seed)
        durations.append((time.perf_counter_ns()-now)/1000)
        if len(relations)!=1:
            raise AssertionError("lookup relation count changed")
    start=time.perf_counter_ns()
    backend.stage_incremental(baseline,updated)
    incremental_ms=(time.perf_counter_ns()-start)/1e6
    verify_generation(backend,fixture,updated)
    # Independent full updated build at a separate key; semantic oracle is generation-independent.
    full_id="00000000-0000-0000-0000-000000000066"
    start=time.perf_counter_ns()
    backend.stage_full(updated,full_id)
    updated_full_ms=(time.perf_counter_ns()-start)/1e6
    for q in updated["queries"]:
        full_rows=backend.lookup(s,full_id,q["seed"])
        incremental_rows=backend.lookup(s,updated["generation_id"],q["seed"])
        if sorted(full_rows,key=lambda r:r["relation_id"]) != sorted(incremental_rows,key=lambda r:r["relation_id"]):
            raise AssertionError("full-incremental relation parity mismatch")
    size=backend.size()
    cross_isolated=None
    if fixture.get("cross_source"):
        cross=fixture["cross_source"]
        backend.stage_full(cross,cross["generation_id"])
        verify_generation(backend,fixture,cross)
        original=backend.lookup(s,g,seed)
        other=backend.lookup(cross["source_id"],cross["generation_id"],seed)
        cross_isolated=(len(original)==1 and len(other)==1 and original[0]["provenance"] != other[0]["provenance"])
        if not cross_isolated:
            raise AssertionError("cross-Source same bare ID contamination")
    return {"backend":backend.name,"groups":fixture["relation_groups"],"resources":len(baseline["resources"]),"ingest_ms":round(ingest_ms,3),"lookup_us_p50":round(pct(durations,50),3),"lookup_us_p95":round(pct(durations,95),3),"lookup_us_p99":round(pct(durations,99),3),"incremental_ms":round(incremental_ms,3),"updated_full_ms":round(updated_full_ms,3),"size":size,"oracle_queries_per_generation":len(baseline["queries"]),"oracle_parity":True,"full_incremental_parity":True,"cross_source_isolated":cross_isolated}


def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--backend",choices=["pg","neo"],required=True)
    ap.add_argument("--port",type=int,required=True)
    ap.add_argument("fixtures",nargs="+",type=Path)
    args=ap.parse_args()
    backend=Pg(args.port) if args.backend=="pg" else Neo(args.port)
    try:
        for path in args.fixtures:
            print(json.dumps(benchmark(backend,json.loads(path.read_text())),sort_keys=True),flush=True)
    finally:
        backend.close()


if __name__=="__main__":
    main()
