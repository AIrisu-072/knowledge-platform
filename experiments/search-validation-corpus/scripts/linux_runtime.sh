#!/usr/bin/env bash
# Disposable validation runtime inside the OrbStack Linux machine.
#   linux_runtime.sh init     create the disposable DB, dirs, migrations, bootstrap
#   linux_runtime.sh start    start document-server (human), Search worker, validation host
#   linux_runtime.sh stop     stop the three processes
#   linux_runtime.sh status   pids and health
# The database password is generated locally into $ROOT/secrets/db.env (0600)
# and never printed. Nothing here touches a non-disposable database.
set -euo pipefail

REPO=${REPO:-/Users/airisu/.claude/worktrees/kp-search-validation}
ROOT=${ROOT:-$HOME/kpval}
BIN=${BIN:-$HOME/kp-target/release}
HOST_BIN=${HOST_BIN:-$HOME/kp-target-host/release/search-validation-host}
MODEL=${MODEL:-/Users/airisu/.cache/knowledge-platform-vector/e5}
SOURCE_ID=${SOURCE_ID:-0192f4a0-7a5e-7c10-8b6e-5a1d0c0ffee1}

secrets_file="$ROOT/secrets/db.env"

load_env() {
  # shellcheck disable=SC1090
  source "$secrets_file"
  export KP_RUNTIME_MODE=poc
  export KP_DATABASE_URL="$DATABASE_URL"
  export KP_STORAGE_ROOT="$ROOT/storage"
  export KP_DSI_WORKER="$BIN/document-semantic-inspection-worker"
  export KP_DIFF_WORKER="$BIN/document-diff-worker"
  # The GUI is not under test: a placeholder dist satisfies the human profile.
  mkdir -p "$ROOT/web-dist"
  printf '<!doctype html><title>validation</title><script type="module" src="/validation.js"></script>\n' > "$ROOT/web-dist/index.html"
  printf '// GUI not under validation\n' > "$ROOT/web-dist/validation.js"
  export KP_WEB_DIST="$ROOT/web-dist"
  export SEARCH_DELIVERY_DATABASE_URL="$DATABASE_URL"
  export SEARCH_COMPLETION_DATABASE_URL="$DATABASE_URL"
  export SEARCH_API_DATABASE_URL="$DATABASE_URL"
  export SEARCH_WORKER_CONFIG="$ROOT/config/source.json"
  export SEARCH_VALIDATION_ACTORS="$ROOT/config/actors.json"
}

init() {
  mkdir -p "$ROOT"/{secrets,storage,lexical,logs,config,run}
  chmod 700 "$ROOT/secrets"
  if [[ ! -f "$secrets_file" ]]; then
    local password
    password=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
    sudo -u postgres psql -q -v ON_ERROR_STOP=1 -c "CREATE ROLE kpval LOGIN SUPERUSER PASSWORD '$password'" >/dev/null
    sudo -u postgres psql -q -v ON_ERROR_STOP=1 -c "CREATE DATABASE kpval OWNER kpval" >/dev/null
    umask 077
    printf 'DATABASE_URL=postgres://kpval:%s@127.0.0.1:5432/kpval\n' "$password" > "$secrets_file"
  fi
  python3 "$REPO/experiments/search-validation-corpus/scripts/runtime_config.py" \
    --root "$ROOT" --worker "$BIN/search-extraction-worker" --source-id "$SOURCE_ID" \
    --formats text --vector-model "$MODEL" --analyzer "${ANALYZER:-tantivy-default-0.26.2}"
  load_env
  "$BIN/document-server" migrate
  KP_IDENTITY_PROFILE=poc-human "$BIN/document-server" bootstrap-poc
  "$HOST_BIN" migrate
  apply_roles
}

# The Search and Graph capability roles the guards check (as the tests do).
# The disposable owner is a superuser, so it holds every capability; the
# per-role separation of production principals is not exercised here.
apply_roles() {
  load_env
  sudo -u postgres psql -q -v ON_ERROR_STOP=1 -d kpval -f "$REPO/crates/search-runtime/sql/roles.sql" >/dev/null
  sudo -u postgres psql -q -v ON_ERROR_STOP=1 -d kpval -f "$REPO/crates/search-graph/sql/roles.sql" >/dev/null
  echo "roles applied"
}

start_one() {
  local name=$1; shift
  nohup "$@" > "$ROOT/logs/$name.log" 2>&1 &
  echo $! > "$ROOT/run/$name.pid"
}

start() {
  load_env
  KP_IDENTITY_PROFILE=poc-human start_one document-server "$BIN/document-server" serve
  start_one search-worker "$BIN/search_outbox_worker"
  start_one search-host "$HOST_BIN"
  sleep 3
  status
}

stop() {
  for name in search-host search-worker document-server; do
    if [[ -f "$ROOT/run/$name.pid" ]]; then
      local pid
      pid=$(cat "$ROOT/run/$name.pid")
      kill "$pid" 2>/dev/null || true
      # Observed: a worker in a running Vector build ignores SIGTERM until the
      # build ends. Record it and force the stop after 20 s.
      for _ in $(seq 20); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
      if kill -0 "$pid" 2>/dev/null; then
        echo "$(date -Is) $name did not stop within 20 s of SIGTERM; killed" >> "$ROOT/logs/stop-events.log"
        kill -9 "$pid" 2>/dev/null || true
      fi
      rm -f "$ROOT/run/$name.pid"
    fi
  done
}

status() {
  for name in document-server search-worker search-host; do
    local pid=""
    [[ -f "$ROOT/run/$name.pid" ]] && pid=$(cat "$ROOT/run/$name.pid")
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then echo "$name running ($pid)"; else echo "$name stopped"; tail -3 "$ROOT/logs/$name.log" 2>/dev/null || true; fi
  done
  curl -s -o /dev/null -w 'document-server ready: %{http_code}\n' http://127.0.0.1:8080/health/ready || true
}

# Rewrites only the config files (e.g. ANALYZER=... linux_runtime.sh configure).
configure() {
  python3 "$REPO/experiments/search-validation-corpus/scripts/runtime_config.py" \
    --root "$ROOT" --worker "$BIN/search-extraction-worker" --source-id "$SOURCE_ID" \
    --formats text --vector-model "$MODEL" --analyzer "${ANALYZER:-tantivy-default-0.26.2}"
}

# One full rebuild of the Source (manual retry), timed, with the worker config.
rebuild() {
  load_env
  local start
  start=$(date +%s)
  "$BIN/search_outbox_worker" rebuild
  echo "rebuild elapsed $(( $(date +%s) - start ))s"
}

"${1:-status}"
