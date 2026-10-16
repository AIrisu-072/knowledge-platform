#!/usr/bin/env bash
# The compose stack's operations, called from mise (`mise run stack:<command>`).
#   secrets  create the secret files once (database password, Search tokens)
#   build    build the images (KP_STACK_PLATFORM=linux/amd64 to force amd64)
#   up       start everything (creates secrets if missing)
#   down     stop and remove containers; data volumes stay
#   reset    stop and remove containers and data volumes
#   smoke    acceptance check: register, search, ACL, 401, restart, same result
#   model    download the pinned Vector embedding model (hash-checked)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export KP_STACK_DIR=${KP_STACK_DIR:-$HOME/.knowledge-platform/stack}
compose=(docker compose -f "$repo/compose.yaml")
# The Vector model directory on the host, mounted read-only into the Search
# services; without the model files Vector stays off.
export KP_SEARCH_VECTOR_MODEL_DIR=${KP_SEARCH_VECTOR_MODEL_DIR:-$KP_STACK_DIR/models/multilingual-e5-small}

random_secret() {
  # 32 random bytes as hex; never printed.
  od -An -tx1 -N32 /dev/urandom | tr -d ' \n'
}

secrets() {
  local dir="$KP_STACK_DIR/secrets"
  mkdir -p "$dir"
  chmod 700 "$dir"
  for name in db_password db_document_password db_search_delivery_password \
      db_search_worker_password db_search_server_password search_human_token \
      search_agent_token; do
    if [[ ! -s "$dir/$name" ]]; then
      (umask 077 && random_secret > "$dir/$name")
      echo "stack: created $dir/$name"
    fi
    # Compose bind-mounts a file secret with its host owner and mode, and the
    # services run as uid 10001 (Linux keeps the host uid; Mac file sharing
    # hides it). The file is readable inside the 0700 directory, which keeps
    # other host users out; a container sees only the secrets it lists.
    chmod 644 "$dir/$name"
  done
  # The single shared role of the first stack version is gone.
  rm -f "$dir/db_app_password"
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
  mkdir -p "$KP_SEARCH_VECTOR_MODEL_DIR"
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

# The pinned revision and digests of spec/selection/library-tool-selection-v0.md
# (Embedding model); crates/search-vector-adapter checks the same digests
# again before it loads the files.
model() {
  local base=https://huggingface.co/intfloat/multilingual-e5-small/resolve/614241f622f53c4eeff9890bdc4f31cfecc418b3
  local dir=$KP_SEARCH_VECTOR_MODEL_DIR tmp
  mkdir -p "$dir"
  tmp=$(mktemp -d "$dir/.download.XXXXXX")
  trap 'rm -rf "$tmp"' RETURN
  while read -r name digest; do
    if [[ -f "$dir/$name" ]] && [[ $(sha256 "$dir/$name") == "$digest" ]]; then
      continue
    fi
    echo "stack: downloading $name"
    curl -fsSL --retry 3 -o "$tmp/$name" "$base/$name"
    if [[ $(sha256 "$tmp/$name") != "$digest" ]]; then
      echo "stack: $name does not match its pinned digest" >&2
      return 1
    fi
  done <<'FILES'
config.json 69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959
tokenizer.json 0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39
model.safetensors 1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477
FILES
  # The weights go last, so a partial directory never looks complete.
  for name in config.json tokenizer.json model.safetensors; do
    if [[ -f "$tmp/$name" ]]; then mv "$tmp/$name" "$dir/$name"; fi
  done
  chmod 755 "$dir"
  chmod 644 "$dir"/config.json "$dir"/tokenizer.json "$dir"/model.safetensors
  echo "stack: model ready in $dir"
}

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

"${1:?usage: stack.sh secrets|build|up|down|reset|smoke|model}"
