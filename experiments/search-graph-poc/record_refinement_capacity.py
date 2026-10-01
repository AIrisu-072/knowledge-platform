"""Append bounded local capacity observations for the fixed-fixture rerun."""

import argparse
import datetime as dt
import json
import shutil
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("event")
    parser.add_argument("container")
    parser.add_argument("--output", type=Path, default=Path("refinement-capacity.jsonl"))
    args = parser.parse_args()
    disk = shutil.disk_usage(Path(__file__).resolve().parent)
    result = subprocess.run(["docker", "inspect", "--size", args.container],
                            capture_output=True, text=True, check=False)
    size = json.loads(result.stdout)[0].get("SizeRw") if result.returncode == 0 else None
    row = {"utc": dt.datetime.now(dt.UTC).isoformat(), "event": args.event,
           "container": args.container, "disk_free_bytes": disk.free,
           "container_rw_bytes": size, "container_inspect_exit": result.returncode}
    with args.output.open("a") as stream:
        stream.write(json.dumps(row, sort_keys=True) + "\n")
    print(json.dumps(row, sort_keys=True))


if __name__ == "__main__":
    main()
