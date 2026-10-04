"""Multiplexed transport to native parallel redb read transactions.

Unlike the audited serial correctness pipe, requests carry IDs and the Rust
server dispatches independent native read transactions concurrently on one
shared Database handle. A single Rust writer mutates a separate BUILDING key.
"""

import json
import queue
import subprocess
import threading

from refinement import IntegrityFailure


class RedbMux:
    def __init__(self, binary, db_path):
        self.proc = subprocess.Popen([str(binary), "serve", str(db_path)],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, bufsize=1)
        self.write_lock = threading.Lock()
        self.pending_lock = threading.Lock()
        self.pending = {}
        self.next_id = 0
        self.reader = threading.Thread(target=self._read_responses, daemon=True)
        self.reader.start()

    def _read_responses(self):
        try:
            for line in self.proc.stdout:
                response = json.loads(line)
                with self.pending_lock:
                    waiter = self.pending.pop(response["id"], None)
                if waiter is not None:
                    waiter.put(response)
        finally:
            with self.pending_lock:
                waiters = list(self.pending.values())
                self.pending.clear()
            for waiter in waiters:
                waiter.put({"error": "native redb server stopped"})

    def request(self, command, **args):
        waiter = queue.Queue(maxsize=1)
        with self.write_lock:
            with self.pending_lock:
                self.next_id += 1
                request_id = self.next_id
                self.pending[request_id] = waiter
            self.proc.stdin.write(json.dumps({"id": request_id, "cmd": command,
                                              **args}, separators=(",", ":")) + "\n")
            self.proc.stdin.flush()
        try:
            response = waiter.get(timeout=30)
        except queue.Empty as error:
            with self.pending_lock:
                self.pending.pop(request_id, None)
            raise TimeoutError(f"redb native request timed out: {command}") from error
        if "error" in response:
            raise RuntimeError(f"redb native {command}: {response['error']}")
        return response["ok"]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=15)
        self.reader.join(timeout=5)
        if self.proc.returncode:
            raise RuntimeError(f"redb native server exited {self.proc.returncode}: "
                               f"{self.proc.stderr.read()[:500]}")


class RedbReader:
    def __init__(self, mux):
        self.mux = mux

    def fetch_frontier(self, source, generation, resources, role):
        pairs = self.mux.request("frontier", source=source, generation=generation,
                                 resources=resources, role=role)
        if pairs is None:
            raise IntegrityFailure("redb generation is not READY")
        return [(payload, [tuple(row) for row in participants])
                for payload, participants in pairs]

    def resource(self, source, generation, resource):
        row = self.mux.request("resource", source=source, generation=generation,
                               resource=resource)
        if row is not None and (row.get("source_id"), row.get("generation_id"),
                                row.get("resource_id")) != (source, generation, resource):
            raise IntegrityFailure("redb native resource key/payload mismatch")
        return row

    def authority_decision(self, source, scenario, resource):
        return self.mux.request("authority", source=source, scenario=scenario,
                                resource=resource)

    def neighbors(self, source, generation, resource, role):
        return self.mux.request("neighbors", source=source, generation=generation,
                                resource=resource, role=role)

    def bulk_rows(self, source, generation):
        rows = self.mux.request("bulk_rows", source=source, generation=generation)
        return rows[0], rows[1], rows[2]

    def bulk_authority(self):
        return self.mux.request("source_bulk")

    def close(self):
        # The shared native server belongs to the profile runner, not a reader.
        pass
