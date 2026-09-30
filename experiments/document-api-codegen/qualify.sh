#!/usr/bin/env bash
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

npm install --prefix "$scratch" --no-package-lock --no-save openapi-typescript@7.13.0 json-schema-to-typescript@16.0.0 >/dev/null
node "$repo/experiments/document-api-codegen/prepare-fixture.mjs" "$scratch/contract-fixture.json" >/dev/null
node "$scratch/node_modules/openapi-typescript/bin/cli.js" "$repo/spec/api/openapi.yaml" -o "$scratch/openapi.d.ts" >/dev/null
node "$scratch/node_modules/json-schema-to-typescript/dist/src/cli.js" -i "$scratch/contract-fixture.json" -o "$scratch/schema.d.ts"
node "$scratch/node_modules/typescript/bin/tsc" --noEmit --strict "$scratch/openapi.d.ts" "$scratch/schema.d.ts"
node "$repo/experiments/document-api-codegen/check-shapes.mjs" "$scratch/openapi.d.ts" "$scratch/schema.d.ts"

cp "$scratch/contract-fixture.json" "$repo/experiments/document-api-codegen/rust-poc/schema.json"
if cargo check --manifest-path "$repo/experiments/document-api-codegen/rust-poc/Cargo.toml" >"$scratch/typify.log" 2>&1; then
  printf 'typify unexpectedly compiled the full actual-contract fixture; review promotion gate.\n' >&2
  exit 1
fi
if ! grep -q 'FieldError.*defined multiple times' "$scratch/typify.log"; then
  cat "$scratch/typify.log" >&2
  exit 1
fi
printf 'typify 0.7.0 failed to compile the actual-contract fixture as expected; no production promotion.\n'
