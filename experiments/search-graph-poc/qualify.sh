#!/usr/bin/env bash
# P3-P04 native backend measurement for one backend and profile.
# Inputs: BACKEND (pg|neo|redb), PROFILE (100|1000|3000), PG_REF, NEO_REF.
# Invoked through `mise run poc:search-graph:qualify` by the measurement workflow.
set -euo pipefail
cd "$(dirname "$0")"
: "${BACKEND:?}" "${PROFILE:?}" "${PG_REF:?}" "${NEO_REF:?}"

python3 -m venv .venv
.venv/bin/pip install --quiet 'psycopg[binary]==3.2.10'
docker pull "$PG_REF"
docker pull "$NEO_REF"
git rev-parse HEAD 'HEAD^{tree}'

cargo build --release --locked --bins

mkdir -p data/runs
for groups in 100 "$PROFILE"; do
  if [ -f "base-fixtures/fixture-$groups.json.gz" ]; then
    # Historical 1,000/3,000 bases predate the Source-2 canary generator.
    gzip -dc "base-fixtures/fixture-$groups.json.gz" > "fixture-$groups.json"
  elif [ ! -f "fixture-$groups.json" ]; then
    ./target/release/search-graph-backend-poc make "$groups" "fixture-$groups.json"
  fi
done
.venv/bin/python qualification_driver.py generate --profile "$PROFILE" \
  --output "data/runs/fixture-$PROFILE.json" \
  --oracle-binary target/release/search-graph-backend-poc
if [ "$PROFILE" != 100 ]; then
  printf '{"profile": %s, "status": "accepted", "basis": "%s"}\n' "$PROFILE" \
    "dedicated hosted runner per candidate and profile; owner chose measurement before selection on 2026-10-05" \
    > "data/runs/admission-$PROFILE.json"
fi

oracle=target/release/search-graph-backend-poc
fixture="data/runs/fixture-$PROFILE.json"
admission=()
if [ "$PROFILE" != 100 ]; then
  admission=(--admission "data/runs/admission-$PROFILE.json")
fi
status=0
# The predeclared five-minute canary stops a slice for diagnosis; resume
# continues from the saved cells without counting incomplete attempts.
measure() {
  local output resume=() attempt
  output="${*: -1}"
  for attempt in $(seq 1 20); do
    if .venv/bin/python qualification_driver.py measure --profile "$PROFILE" \
        --fixture "$fixture" --oracle-binary "$oracle" "${admission[@]}" \
        "${resume[@]}" "$@"; then
      return 0
    fi
    if [ "$(jq -r '.error.message // ""' "$output")" != "five-minute candidate/profile canary exceeded" ]; then
      return 1
    fi
    echo "canary slice $attempt stopped; resuming"
    resume=(--resume)
  done
  return 1
}
if [ "$BACKEND" = redb ]; then
  mkdir -p data/qualification
  db="data/qualification/redb-$PROFILE.db"
  measure --backend redb --redb-binary target/release/qualification_redb --db-path "$db" \
    --output "data/runs/measure-redb-$PROFILE.json" || status=$?
  .venv/bin/python qualification_recovery.py --backend redb \
    --redb-binary target/release/qualification_redb --db-path "$db" \
    --fixture "$fixture" --output "data/runs/recovery-redb-$PROFILE.json" || status=$?
  exit "$status"
fi
ref="$PG_REF"
if [ "$BACKEND" = neo ]; then ref="$NEO_REF"; fi
image="$(docker image inspect "$ref" --format '{{.Id}}')"
started="$(.venv/bin/python qualification_runtime.py start --backend "$BACKEND" \
  --profile "$PROFILE" --image-id "$image")"
printf '%s\n' "$started" > "data/runs/start-$BACKEND-$PROFILE.json"
name="$(printf '%s' "$started" | jq -r .container_name)"
port="$(printf '%s' "$started" | jq -r .host_port)"
data="$(printf '%s' "$started" | jq -r .data_dir)"
measure --backend "$BACKEND" --port "$port" --image-id "$image" \
  --container-name "$name" --data-dir "$data" \
  --output "data/runs/measure-$BACKEND-$PROFILE.json" || status=$?
.venv/bin/python qualification_recovery.py --backend "$BACKEND" --container-name "$name" \
  --image-id "$image" --data-dir "$data" --port "$port" --fixture "$fixture" \
  --output "data/runs/recovery-$BACKEND-$PROFILE.json" || status=$?
docker stats --no-stream --format '{{json .}}' "$name" > "data/runs/final-stats-$BACKEND-$PROFILE.json" || true
exit "$status"
