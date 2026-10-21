#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# ccbench entry point.
#   scripts/bench/run.sh --claude-rs <path> --claude <path> --runs N --out <dir> [--engine official|bundled|<path>]
#                [--mcp user|none] [--scenarios main,typing,resume] [--session-file F] [--idle-secs 30] [--skip-setup]
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY="$(command -v python3 || echo /usr/bin/python3)"
OUT=""; args=("$@")
for ((i=0;i<${#args[@]};i++)); do [ "${args[i]}" = "--out" ] && OUT="${args[i+1]}"; done
[ -n "$OUT" ] || { echo "usage: run.sh --claude-rs P --claude P --runs N --out DIR [options]" >&2; exit 2; }
mkdir -p "$OUT"
"$PY" -I "$HERE/bench.py" "$@"
"$PY" -I "$HERE/summarize.py" "$OUT"
echo "summary: $OUT/summary.md"
