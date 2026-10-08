#!/usr/bin/env bash
# Tuning-split sweep of the Vector similarity floor seen by the validation
# host (query side only; the index is unchanged). Final questions are not
# scored here. Usage: sweep_floor.sh 0.89 0.87 0.85 ...
set -euo pipefail
ROOT=${ROOT:-$HOME/kpval}
REPO=${REPO:-/Users/airisu/.claude/worktrees/kp-search-validation}
S=$REPO/experiments/search-validation-corpus/scripts
DATA=/Users/airisu/kp-validation-data
source "$ROOT/secrets/db.env"
export SEARCH_API_DATABASE_URL=$DATABASE_URL SEARCH_WORKER_CONFIG=$ROOT/config/source.json SEARCH_VALIDATION_ACTORS=$ROOT/config/actors.json
for floor in "$@"; do
  python3 - "$floor" <<'EOF'
import json, sys
path = "/home/airisu/kpval/config/source.json"
config = json.load(open(path))
config["vector"]["similarity_floor"] = float(sys.argv[1])
json.dump(config, open(path, "w"), indent=2)
EOF
  kill "$(cat "$ROOT/run/search-host.pid")" 2>/dev/null || true
  sleep 2
  nohup "$HOME/kp-target-host/release/search-validation-host" > "$ROOT/logs/search-host.log" 2>&1 &
  echo $! > "$ROOT/run/search-host.pid"
  sleep 3
  python3 "$S/evaluate.py" --split tuning --systems discover \
    --questions "$DATA/eval/stage1-auto.jsonl" "$REPO/experiments/search-validation-corpus/questions/stage1-llm.jsonl" \
    --corpus "$DATA/corpus/jawiki-970" "$DATA/corpus/laws-20261006" \
    --ingest "$ROOT/ingest/jawiki-970.jsonl" "$ROOT/ingest/laws.jsonl" \
    --out "$ROOT/eval/floor-$floor.jsonl" > "$ROOT/eval/floor-$floor-summary.json"
  python3 - "$floor" <<'EOF'
import json, sys
s = json.load(open(f"/home/airisu/kpval/eval/floor-{sys.argv[1]}-summary.json"))
a = s["discover/tuning/ALL"]
print(sys.argv[1], "recall@10", a.get("recall@10"), "fp", a.get("false_positive_rate"), "p50", a.get("p50_ms"))
EOF
done
