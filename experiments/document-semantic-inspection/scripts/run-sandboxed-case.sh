#!/usr/bin/env bash
set -euo pipefail

: "${DSI_POC_CPU_SECONDS:=8}"
: "${DSI_POC_FILE_BLOCKS:=2048}"
: "${DSI_POC_VMEM_KIB:=2097152}"

ulimit -t "${DSI_POC_CPU_SECONDS}"
ulimit -f "${DSI_POC_FILE_BLOCKS}"

if [[ "$(uname -s)" == "Linux" ]]; then
  ulimit -v "${DSI_POC_VMEM_KIB}"
fi

exec "$@"
