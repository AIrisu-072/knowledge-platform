"""Owned, offline candidate runtime and per-cell machine counters."""

import json
import argparse
import os
import platform
import re
import shutil
import subprocess
import time
from decimal import Decimal
from pathlib import Path


HERE = Path(__file__).resolve().parent
QUAL_DATA = HERE / "data" / "qualification"
OWNER_LABEL = "p3.native.qualification"
PREFIX = "p3-native-qualification-"
GIB = 1024 ** 3
MIB = 1024 ** 2


def parse_human_bytes(value):
    match = re.fullmatch(r"([0-9]+(?:\.[0-9]+)?)(B|kB|KB|MB|GB|TB|KiB|MiB|GiB|TiB)",
                         value.strip())
    if not match:
        raise ValueError(f"unknown memory size unit: {value}")
    amount, unit = match.groups()
    multipliers = {"B": 1, "kB": 1000, "KB": 1000, "MB": 1000**2,
                   "GB": 1000**3, "TB": 1000**4, "KiB": 1024,
                   "MiB": 1024**2, "GiB": 1024**3, "TiB": 1024**4}
    return int(Decimal(amount) * multipliers[unit])


def command(args, *, timeout=15, input_bytes=None):
    return subprocess.run(args, input=input_bytes, capture_output=True, check=True,
                          timeout=timeout).stdout


def installed_image(image_id):
    value = json.loads(command(["docker", "image", "inspect", image_id]))[0]
    if value["Id"] != image_id:
        raise ValueError("cached image does not match exact pinned image ID")
    return {"image_id": image_id, "repo_digests": value.get("RepoDigests", [])}


def owned_container(name, image_id, data_dir):
    if not name.startswith(PREFIX) or "/" in name:
        raise ValueError("candidate container name lacks qualification namespace")
    value = json.loads(command(["docker", "inspect", name]))[0]
    labels = value["Config"].get("Labels") or {}
    if (value["Name"] != "/" + name or labels.get(OWNER_LABEL) != "1" or
            value["Image"] != image_id or not value["State"]["Running"]):
        raise ValueError("candidate container owner/image/running state mismatch")
    data_dir = Path(data_dir).resolve()
    if not data_dir.is_relative_to(QUAL_DATA.resolve()):
        raise ValueError("candidate data dir outside owned qualification tree")
    marker = data_dir / ".owner"
    if not marker.exists() or marker.read_text() != name + "\n":
        raise ValueError("candidate data dir lacks matching owner marker")
    if not any(Path(m["Source"]).resolve() == data_dir for m in value["Mounts"]):
        raise ValueError("candidate container does not mount the stated data dir")
    return value


def machine_inventory():
    def version(args):
        try:
            return command(args, timeout=5).decode(errors="replace").strip()
        except (FileNotFoundError, subprocess.CalledProcessError,
                subprocess.TimeoutExpired):
            return "unavailable"
    cpu_model = version(["sysctl", "-n", "machdep.cpu.brand_string"])
    memory_bytes = version(["sysctl", "-n", "hw.memsize"])
    if platform.system() == "Linux":
        cpu_info = Path("/proc/cpuinfo").read_text()
        cpu_model = next((row.split(":", 1)[1].strip() for row in cpu_info.splitlines()
                          if row.startswith("model name")), "unavailable")
        mem_info = Path("/proc/meminfo").read_text()
        memory_bytes = str(int(next(row.split()[1] for row in mem_info.splitlines()
                                    if row.startswith("MemTotal:"))) * 1024)
    return {"platform": platform.platform(), "machine": platform.machine(),
            "cpu_count": os.cpu_count(), "uname": version(["uname", "-a"]),
            "cpu_model": cpu_model, "memory_bytes": memory_bytes,
            "rustc": version(["rustc", "--version"]),
            "cargo": version(["cargo", "--version"]),
            "docker": version(["docker", "--version"])}


def native_candidate_bytes(name, data_dir):
    """Read actual native bytes rather than silently skipping UID-owned dirs."""
    if not name.startswith(PREFIX):
        raise ValueError("candidate name outside owned namespace")
    value = json.loads(command(["docker", "inspect", name]))[0]
    if (value["Name"] != "/" + name or
            (value["Config"].get("Labels") or {}).get(OWNER_LABEL) != "1"):
        raise ValueError("candidate owner does not match native disk probe")
    matches = [row["Destination"] for row in value["Mounts"]
               if Path(row["Source"]).resolve() == Path(data_dir).resolve()]
    if len(matches) != 1 or matches[0] not in ("/data", "/var/lib/postgresql"):
        raise ValueError("candidate mount does not match native disk probe")
    raw = command(["docker", "exec", name, "du", "-sb", matches[0]], timeout=15)
    return int(raw.split(b"\t", 1)[0])


def enforce_candidate_budget(metrics, profile):
    if metrics["disk_free_bytes"] < int(1.5 * GIB):
        raise RuntimeError("host free disk crossed 1.5 GiB stop floor")
    size = metrics.get("candidate_data_bytes")
    if size is None:
        raise RuntimeError("candidate disk usage was not measured")
    if size > int(1.5 * GIB) or (profile == 100 and size >= 512 * MIB):
        raise RuntimeError("candidate disk growth budget exceeded")
    rss = max(metrics.get("docker_memory_bytes", 0),
              metrics.get("redb_rss_kib", 0) * 1024,
              metrics.get("driver_rss_peak_bytes", 0))
    if rss > 6 * GIB:
        raise RuntimeError("candidate process RSS exceeded 6 GiB")


def machine_sample(name=None, data_dir=None, redb_pid=None):
    disk = shutil.disk_usage(HERE)
    row = {"disk_free_bytes": disk.free, "disk_used_bytes": disk.used}
    if data_dir is not None:
        data_dir = Path(data_dir)
        row["candidate_data_bytes"] = (native_candidate_bytes(name, data_dir) if name else
            data_dir.stat().st_size if data_dir.is_file() else
            sum(p.stat().st_size for p in data_dir.rglob("*") if p.is_file()))
    if name:
        raw = command(
            ["docker", "stats", "--no-stream", "--format", "{{json .}}", name],
            timeout=15).decode().strip()
        row["docker_stats_raw"] = raw
        row["docker_memory_bytes"] = parse_human_bytes(
            json.loads(raw)["MemUsage"].split(" / ", 1)[0])
    if redb_pid:
        row["redb_rss_kib"] = int(command(["ps", "-o", "rss=", "-p", str(redb_pid)]).strip())
    return row


def stop_on_disk_floor(name, image_id, data_dir):
    """Only remove a verified task-owned candidate and its marked private dir."""
    if shutil.disk_usage(HERE).free >= int(1.5 * GIB):
        return False
    owned_container(name, image_id, data_dir)
    command(["docker", "stop", "--time", "10", name], timeout=30)
    command(["docker", "rm", name], timeout=15)
    data_dir = Path(data_dir).resolve()
    if (data_dir / ".owner").read_text() != name + "\n":
        raise ValueError("owner marker changed during candidate stop")
    shutil.rmtree(data_dir)
    return True


def start_candidate(backend, profile, image_id):
    if backend not in ("pg", "neo") or profile not in (100, 1000, 3000):
        raise ValueError("unsupported candidate")
    if shutil.disk_usage(HERE).free < 2 * GIB:
        raise RuntimeError("candidate disk admission below 2 GiB")
    installed_image(image_id)
    name = f"{PREFIX}{backend}-{profile}"
    data_dir = QUAL_DATA / name
    if data_dir.exists():
        raise FileExistsError("candidate data directory already exists")
    data_dir.mkdir(parents=True)
    (data_dir / ".owner").write_text(name + "\n")
    common = ["docker", "run", "-d", "--pull=never", "--name", name,
              "--label", f"{OWNER_LABEL}=1"]
    if backend == "pg":
        args = ["--memory=1g", "-e", "POSTGRES_PASSWORD=p3syntheticpass",
                "-e", "POSTGRES_DB=p3poc", "-p", "127.0.0.1::5432",
                "-v", f"{data_dir}:/var/lib/postgresql", image_id]
        port_id = "5432/tcp"
    else:
        args = ["--memory=2g", "-e", "NEO4J_AUTH=neo4j/p3syntheticpass",
                "-e", "NEO4J_db_tx__log_preallocate=false",
                "-e", "NEO4J_server_memory_heap_initial__size=256m",
                "-e", "NEO4J_server_memory_heap_max__size=256m",
                "-e", "NEO4J_server_memory_pagecache_size=128m",
                "-p", "127.0.0.1::7474", "-v", f"{data_dir}:/data", image_id]
        port_id = "7474/tcp"
    try:
        container_id = command(common + args, timeout=30).decode().strip()
        owned_container(name, image_id, data_dir)
        bound = command(["docker", "port", name, port_id]).decode().strip()
        host_port = int(bound.rsplit(":", 1)[1])
        from qualification_native import NeoReader, PgReader
        deadline = time.monotonic() + 90
        while True:
            try:
                reader = PgReader(host_port) if backend == "pg" else NeoReader(host_port)
                try:
                    if backend == "pg":
                        reader.conn.execute("SELECT 1")
                    else:
                        reader.run("RETURN 1")
                finally:
                    reader.close()
                break
            except Exception:
                if time.monotonic() >= deadline:
                    raise TimeoutError("owned candidate did not become ready")
                time.sleep(0.5)
        return {"container_name": name, "container_id": container_id,
                "data_dir": str(data_dir), "image_id": image_id,
                "host_port": host_port,
                "after_start": machine_sample(name, data_dir)}
    except Exception:
        # A failed start may leave an owned container; keep it for explicit
        # inspection rather than deleting state with an uncertain result.
        raise


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    start = sub.add_parser("start")
    start.add_argument("--backend", choices=("pg", "neo"), required=True)
    start.add_argument("--profile", type=int, required=True)
    start.add_argument("--image-id", required=True)
    args = parser.parse_args()
    if args.command == "start":
        print(json.dumps(start_candidate(args.backend, args.profile, args.image_id),
                         sort_keys=True))


if __name__ == "__main__":
    main()
