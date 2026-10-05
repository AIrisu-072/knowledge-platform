"""Narrow persisted-incidence interface for correctness and later measurement runs."""

import json
import subprocess
from pathlib import Path

from bench import Neo, Pg, nanos
from refinement import IntegrityFailure


def _offset(stamp):
    return None if stamp is None else stamp[6] * 3600 + stamp[7] * 60 + stamp[8]


class RefinedPg(Pg):
    transport = "psycopg/PostgreSQL protocol"

    def neighbors(self, source, generation, resource, role):
        return [row[0] for row in self.conn.execute(
            "SELECT p.relation_id::text FROM p3_poc.participant p "
            "JOIN p3_poc.generation g USING(source_id,generation_id) "
            "WHERE p.source_id=%s AND p.generation_id=%s AND p.resource_id=%s "
            "AND p.role=%s AND g.state='READY' ORDER BY p.relation_id",
            (source, generation, resource, role)).fetchall()]

    def relation(self, source, generation, relation_id):
        row = self.conn.execute(
            "SELECT r.payload FROM p3_poc.relation r JOIN p3_poc.generation g "
            "USING(source_id,generation_id) WHERE r.source_id=%s AND r.generation_id=%s "
            "AND r.relation_id=%s AND g.state='READY'",
            (source, generation, relation_id)).fetchone()
        if row is None:
            return None
        participants = self.conn.execute(
            "SELECT ordinal,role,resource_id::text FROM p3_poc.participant "
            "WHERE source_id=%s AND generation_id=%s AND relation_id=%s ORDER BY ordinal",
            (source, generation, relation_id)).fetchall()
        return row[0], participants

    def resource(self, source, generation, resource):
        row = self.conn.execute(
            "SELECT r.payload,r.owner_id::text,r.valid_from_ns,r.valid_from_offset,"
            "r.valid_to_ns,r.valid_to_offset,r.freshness_anchor_ns,r.freshness_anchor_offset,"
            "r.freshness_basis,r.effective_from_ns,r.effective_from_offset,"
            "r.effective_to_ns,r.effective_to_offset FROM p3_poc.resource r "
            "JOIN p3_poc.generation g USING(source_id,generation_id) "
            "WHERE r.source_id=%s AND r.generation_id=%s AND r.resource_id=%s "
            "AND g.state='READY'",
            (source, generation, resource)).fetchone()
        if row is None:
            return None
        payload, owner, *columns = row
        if payload["resource_id"] != resource or payload["source_id"] != source or payload["generation_id"] != generation:
            raise IntegrityFailure("PostgreSQL resource key/payload mismatch")
        if payload["owner_id"] != owner:
            raise IntegrityFailure("PostgreSQL owner column mismatch")
        temporal = payload["temporal"]
        profile = temporal["profile"]
        stamps = [temporal["valid_from"], temporal["valid_to"], profile["freshness_anchor_at"],
                  profile["effective_from"], profile["effective_to"]]
        expected = []
        for item in stamps[:3]:
            expected.extend((nanos(item), _offset(item)))
        expected.append(profile["freshness_basis"])
        for item in stamps[3:]:
            expected.extend((nanos(item), _offset(item)))
        if [None if x is None else int(x) if i != 6 else x for i, x in enumerate(columns)] != expected:
            raise IntegrityFailure("PostgreSQL temporal column/payload mismatch")
        return payload

    def all_ids(self, source, generation):
        resources = [r[0] for r in self.conn.execute(
            "SELECT resource_id::text FROM p3_poc.resource WHERE source_id=%s AND generation_id=%s "
            "ORDER BY resource_id", (source, generation)).fetchall()]
        relations = [r[0] for r in self.conn.execute(
            "SELECT relation_id::text FROM p3_poc.relation WHERE source_id=%s AND generation_id=%s "
            "ORDER BY relation_id", (source, generation)).fetchall()]
        return resources, relations

    def incidence_rows(self, source, generation):
        return self.conn.execute(
            "SELECT relation_id::text,ordinal,role,resource_id::text,source_id::text,"
            "generation_id::text FROM p3_poc.participant "
            "WHERE source_id=%s AND generation_id=%s", (source, generation)).fetchall()

    def corrupt_participant(self, source, generation, relation_id, ordinal):
        self.conn.execute("DELETE FROM p3_poc.participant WHERE source_id=%s AND generation_id=%s "
                          "AND relation_id=%s AND ordinal=%s", (source, generation, relation_id, ordinal))

    def corrupt_reverse_incidence(self, source, generation, relation_id, ordinal):
        self.corrupt_participant(source, generation, relation_id, ordinal)

    def corrupt_temporal(self, source, generation, resource):
        self.conn.execute("UPDATE p3_poc.resource SET freshness_basis='tampered' "
                          "WHERE source_id=%s AND generation_id=%s AND resource_id=%s",
                          (source, generation, resource))

    def corrupt_mapping(self, source, generation, resource):
        self.conn.execute("UPDATE p3_poc.resource SET payload=jsonb_set(payload,"
                          "'{mapping,document_id}',to_jsonb(%s::text)) "
                          "WHERE source_id=%s AND generation_id=%s AND resource_id=%s",
                          ("00000000-0000-0000-0000-000000000fff", source, generation, resource))


class RefinedNeo(Neo):
    transport = "Neo4j Query API v2 HTTP"

    def neighbors(self, source, generation, resource, role):
        rows = self.run(
            "MATCH (r:P3Relation {source:$source,generation:$generation})"
            "-[p:P3_PARTICIPANT {role:$role}]->(n:P3Resource {key:$key}) "
            "RETURN r.id ORDER BY r.id",
            {"source": source, "generation": generation, "role": role,
             "key": self.key(source, generation, resource)})
        return [row[0] for row in rows]

    def relation(self, source, generation, relation_id):
        rows = self.run(
            "MATCH (r:P3Relation {key:$key}) OPTIONAL MATCH (r)-[p:P3_PARTICIPANT]->(n:P3Resource) "
            "RETURN r.payload,collect([p.ordinal,p.role,n.id])",
            {"key": self.key(source, generation, relation_id)})
        if not rows:
            return None
        payload, participants = rows[0]
        participants = [tuple(p) for p in participants if p[0] is not None]
        return json.loads(payload), participants

    def resource(self, source, generation, resource):
        rows = self.run("MATCH (n:P3Resource {key:$key}) RETURN n.payload",
                        {"key": self.key(source, generation, resource)})
        if not rows:
            return None
        payload = json.loads(rows[0][0])
        if payload["source_id"] != source or payload["generation_id"] != generation or payload["resource_id"] != resource:
            raise IntegrityFailure("Neo4j resource key/payload mismatch")
        return payload

    def all_ids(self, source, generation):
        resources = self.run("MATCH (n:P3Resource {source:$source,generation:$generation}) RETURN n.id ORDER BY n.id",
                             {"source": source, "generation": generation})
        relations = self.run("MATCH (n:P3Relation {source:$source,generation:$generation}) RETURN n.id ORDER BY n.id",
                             {"source": source, "generation": generation})
        return [r[0] for r in resources], [r[0] for r in relations]

    def incidence_rows(self, source, generation):
        return self.run(
            "MATCH (r:P3Relation {source:$source,generation:$generation})"
            "-[p:P3_PARTICIPANT]->(n:P3Resource) "
            "RETURN r.id,p.ordinal,p.role,n.id,n.source,n.generation",
            {"source": source, "generation": generation})

    def corrupt_participant(self, source, generation, relation_id, ordinal):
        self.run("MATCH (r:P3Relation {key:$key})-[p:P3_PARTICIPANT {ordinal:$ordinal}]->() DELETE p",
                 {"key": self.key(source, generation, relation_id), "ordinal": ordinal})

    def corrupt_reverse_incidence(self, source, generation, relation_id, ordinal):
        self.corrupt_participant(source, generation, relation_id, ordinal)

    def corrupt_temporal(self, source, generation, resource):
        # Neo4j stores the temporal value in canonical payload, so a payload mutation
        # is detected against the fixture digest rather than an independent SQL column.
        row = self.resource(source, generation, resource)
        row["temporal"]["profile"]["freshness_basis"] = "tampered"
        self.run("MATCH (n:P3Resource {key:$key}) SET n.payload=$payload",
                 {"key": self.key(source, generation, resource),
                  "payload": json.dumps(row, sort_keys=True)})

    def corrupt_mapping(self, source, generation, resource):
        row = self.resource(source, generation, resource)
        row["mapping"]["document_id"] = "00000000-0000-0000-0000-000000000fff"
        self.run("MATCH (n:P3Resource {key:$key}) SET n.payload=$payload",
                 {"key": self.key(source, generation, resource),
                  "payload": json.dumps(row, sort_keys=True)})


class RefinedRedb:
    name = "redb-4.3.0"
    transport = "redb native Rust read/write transaction over JSON-lines local adapter"

    def __init__(self, binary, fixture, path):
        self.binary, self.path = Path(binary), Path(path)
        if self.path.exists():
            raise FileExistsError(f"refuse to overwrite redb file: {self.path}")
        staged = subprocess.run([str(self.binary), "stage", str(fixture), str(self.path)],
                                check=True, capture_output=True, text=True, timeout=60)
        self.stage_receipt = json.loads(staged.stdout)
        self.proc = subprocess.Popen([str(self.binary), "serve", str(self.path)],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, bufsize=1)

    def _call(self, cmd, source, generation, **kwargs):
        request = {"cmd": cmd, "source": source, "generation": generation, **kwargs}
        self.proc.stdin.write(json.dumps(request) + "\n")
        self.proc.stdin.flush()
        response = self.proc.stdout.readline()
        if not response:
            raise RuntimeError(f"redb native adapter stopped: {self.proc.stderr.read()[:500]}")
        return json.loads(response)["ok"]

    def neighbors(self, source, generation, resource, role):
        return self._call("neighbors", source, generation, resource=resource, role=role)

    def relation(self, source, generation, relation_id):
        value = self._call("relation", source, generation, relation=relation_id)
        return None if value is None else (value[0], [tuple(x) for x in value[1]])

    def resource(self, source, generation, resource):
        value = self._call("resource", source, generation, resource=resource)
        if value is not None and (value["source_id"], value["generation_id"], value["resource_id"]) != (
                source, generation, resource):
            raise IntegrityFailure("redb resource key/payload mismatch")
        return value

    def all_ids(self, source, generation):
        return self._call("all_ids", source, generation)

    def incidence_rows(self, source, generation):
        return self._call("incidence_rows", source, generation)

    def corrupt_participant(self, source, generation, relation_id, ordinal):
        self._call("corrupt_participant", source, generation, relation=relation_id, ordinal=ordinal)

    def corrupt_reverse_incidence(self, source, generation, relation_id, ordinal):
        self._call("corrupt_reverse_incidence", source, generation,
                   relation=relation_id, ordinal=ordinal)

    def corrupt_temporal(self, source, generation, resource):
        self._call("corrupt_temporal", source, generation, resource=resource)

    def corrupt_mapping(self, source, generation, resource):
        self._call("corrupt_mapping", source, generation, resource=resource)

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)
