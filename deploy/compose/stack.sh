#!/usr/bin/env bash
# The compose stack's operations, called from mise (`mise run stack:<command>`).
#   secrets  create the secret files once (database password, Search tokens)
#   build    build the images (KP_STACK_PLATFORM=linux/amd64 to force amd64)
#   up       start everything (creates secrets if missing)
#   down     stop and remove containers; data volumes stay
#   reset    stop and remove containers and data volumes
#   smoke    acceptance check: register, search, ACL, 401, restart, same result
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export KP_STACK_DIR=${KP_STACK_DIR:-$HOME/.knowledge-platform/stack}
compose=(docker compose -f "$repo/compose.yaml")

random_secret() {
  # 32 random bytes as hex; never printed.
  od -An -tx1 -N32 /dev/urandom | tr -d ' \n'
}

secrets() {
  local dir="$KP_STACK_DIR/secrets"
  mkdir -p "$dir"
  chmod 700 "$dir"
  for name in db_password search_human_token search_agent_token; do
    if [[ ! -s "$dir/$name" ]]; then
      (umask 077 && random_secret > "$dir/$name")
      echo "stack: created $dir/$name"
    fi
    chmod 600 "$dir/$name"
  done
}

build() {
  if [[ -n "${KP_STACK_PLATFORM:-}" ]]; then
    DOCKER_DEFAULT_PLATFORM="$KP_STACK_PLATFORM" "${compose[@]}" build
  else
    "${compose[@]}" build
  fi
}

up() {
  secrets
  "${compose[@]}" up -d --wait --wait-timeout 600 \
    postgres document-human document-agent search-worker search-human search-agent
  "${compose[@]}" ps
}

down() {
  "${compose[@]}" down
}

reset() {
  "${compose[@]}" down --volumes
}

smoke() {
  local state
  state=$(mktemp)
  trap 'rm -f "$state"' RETURN
  python3 "$repo/deploy/compose/smoke.py" --state "$state"
  echo "stack: restarting the Search servers"
  "${compose[@]}" restart search-human search-agent
  python3 "$repo/deploy/compose/smoke.py" --state "$state" --check-only
  echo "stack: smoke passed, including after a Search server restart"
}

"${1:?usage: stack.sh secrets|build|up|down|reset|smoke}"
