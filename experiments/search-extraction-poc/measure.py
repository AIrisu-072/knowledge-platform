#!/usr/bin/env python3
"""P1-V01 capacity probe driver.

Runs the release ``body_measure`` example once per case in a fresh process and
adds that process's CPU time and peak RSS (``os.wait4``). Writes
``report.json`` and a short ``report.md``. Readers run in-process in the probe;
fresh-process sandbox, cgroup and Linux numbers are not measured here.

Usage (repository root):
    cargo build -p search-source-document --example body_measure --release
    python3 experiments/search-extraction-poc/measure.py
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PROBE = ROOT / "target" / "release" / "examples" / "body_measure"

CASES = [
    ("text", [1, 10, 50]),
    ("csv", [1, 10, 50]),
    ("html", [1, 10]),
    ("docx", [1, 10]),
    ("zip-high-ratio", [10, 50]),
    ("many-parts", [1]),
    ("many-documents", [1]),
    ("max-unit", [1]),
]
TIMEOUT_SECONDS = 600


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run_case(case: str, mib: int) -> dict:
    started = time.monotonic()
    process = subprocess.Popen(
        [str(PROBE), "--case", case, "--mib", str(mib), "--queries", "200"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        stdout, stderr = process.communicate(timeout=TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate()
        return {"case": case, "mib": mib, "status": "timeout", "limit_s": TIMEOUT_SECONDS}
    wall = time.monotonic() - started
    # communicate() already reaped the child; rusage comes from RUSAGE_CHILDREN deltas.
    return {
        "case": case,
        "mib": mib,
        "exit_code": process.returncode,
        "wall_s": round(wall, 3),
        "stdout": stdout.decode("utf-8", "replace").strip(),
        "stderr_tail": stderr.decode("utf-8", "replace").strip()[-400:],
    }


def main() -> int:
    if not PROBE.exists():
        print(f"missing probe binary: {PROBE.relative_to(ROOT)}", file=sys.stderr)
        return 2
    rows = []
    for case, sizes in CASES:
        for mib in sizes:
            import resource

            before = resource.getrusage(resource.RUSAGE_CHILDREN)
            row = run_case(case, mib)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            row["cpu_s"] = round(
                (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime), 3
            )
            # ru_maxrss is the largest reaped child so far: bytes on macOS, KiB on Linux.
            scale = 1 if platform.system() == "Darwin" else 1024
            row["children_peak_rss_bytes_so_far"] = after.ru_maxrss * scale
            if row.get("exit_code") == 0 and row.get("stdout"):
                row["result"] = json.loads(row.pop("stdout"))
                row["status"] = "measured"
            elif "status" not in row:
                row["status"] = "failed"
            rows.append(row)
            print(json.dumps({key: row.get(key) for key in ("case", "mib", "status", "wall_s")}))
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False
    ).stdout.strip()
    rustc = subprocess.run(
        ["rustc", "--version"], cwd=ROOT, capture_output=True, text=True, check=False
    ).stdout.strip()
    report = {
        "schema": "p1-v01-capacity-probe/v1",
        "measured_at_unix": int(time.time()),
        "environment": {
            "os": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "rustc": rustc,
            "git_head": head,
            "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
            "probe_sha256": sha256(PROBE),
        },
        "scope": {
            "measured": [
                "host body pipeline in one process: raw read and SHA-256 binding, readers, "
                "locator round trip, Unit manifest, Tantivy Unit index seal, publication",
                "cold build (first in a fresh process) and warm full rebuild",
                "BodyOnly query latency, verified-hit ratio, unique-parent fill (limit 10)",
                "per-process CPU time and children peak RSS so far (monotonic across cases)",
            ],
            "not_measured": [
                "fresh-process Linux sandbox (Landlock/seccomp), cgroup high-water, scratch bytes",
                "PDF/XLSX/PPTX representatives (fixtures exist only in the reader PoC)",
                "real PostgreSQL/FS latency (covered functionally by body_vertical)",
                "production SLO: none is declared from these numbers",
            ],
            "profile_limits": "every applicable BudgetKey at its absolute code ceiling",
        },
        "cases": rows,
    }
    (HERE / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1) + "\n")
    lines = [
        "# P1-V01 capacity probe",
        "",
        f"Environment: {report['environment']['os']} {report['environment']['machine']}, "
        f"{rustc}, head `{head[:12]}`. Readers in-process; no sandbox/cgroup numbers. "
        "No production SLO is declared from these values.",
        "",
        "| case | MiB | status | outcomes | Units | cold ms | warm ms | query p50/p95/p99 µs | CPU s |",
        "| --- | ---: | --- | --- | ---: | ---: | ---: | --- | ---: |",
    ]
    for row in rows:
        result = row.get("result", {})
        query = result.get("query_us", {})
        lines.append(
            "| {case} | {mib} | {status} | {outcomes} | {units} | {cold} | {warm} | {q} | {cpu} |".format(
                case=row["case"],
                mib=row["mib"],
                status=row["status"],
                outcomes=", ".join(f"{k}×{v}" for k, v in result.get("outcomes", {}).items()) or "—",
                units=result.get("units", "—"),
                cold=result.get("cold_build_ms", "—"),
                warm=result.get("warm_build_ms", "—"),
                q="/".join(str(query.get(p, "—")) for p in ("p50", "p95", "p99")) if query else "—",
                cpu=row.get("cpu_s", "—"),
            )
        )
    (HERE / "report.md").write_text("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
