#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Claude Code parity suite: drives claude-rs in a private tmux server against
# the real, logged-in stock Claude Code and records, per functionality, whether
# claude-rs does what stock does. Local and opt-in only; never run in CI.
#
#   scripts/e2e-parity/run.sh [--bin PATH] [--bridge PATH] [--only CHECK...]
#                             [--report docs/src/parity.md] [--list]
#
# See scripts/e2e-parity/README.md for what it needs and what it costs.
# shellcheck disable=SC2034 # globals set here are read by the sourced checks

set -uo pipefail

if ((BASH_VERSINFO[0] < 4)); then
  echo "run.sh needs bash 4 or later (associative arrays)" >&2
  exit 2
fi

SUITE_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
REPO_DIR=$(cd "$SUITE_DIR/../.." && pwd -P)

usage() {
  cat <<EOF
Usage: $0 [options]

  --bin PATH             claude-rs binary to test (default: ~/.local/bin/claude-rs)
  --bridge PATH          bridge.js for a source build (e.g. agent-sdk/dist/bridge.js);
                         omit for an installed release, which carries its own
  --bridge-runtime PATH  Bun runtime for --bridge (default: newest installed
                         ~/.local/share/claude-rs/*/claude-rs-bridge-bun)
  --claude PATH          stock Claude Code executable (default: the installed
                         ClaudeCode.app binary, else \`claude\` on PATH)
  --baseline-bin PATH    claude-rs to compare performance against (default: the
                         installed 0.15.1-fork.1, else ~/.local/bin/claude-rs)
  --launcher PATH        system-config claude-launch for the launcher-path checks
                         (default: ~/.local/bin/claude-launch; "" skips them)
  --config-dir DIR       CLAUDE_CONFIG_DIR (default: \$PARITY_CONFIG_DIR or ~/.claude-personal)
  --work DIR             scratch root for session cwds, logs and screens
                         (default: \$TMPDIR/claude-rs-parity); never a real repo
  --sessions-file PATH   append every created session id here
                         (default: WORK/test-sessions.txt)
  --src DIR              claude-rs source tree, for the app-owned command
                         catalog (default: this repository)
  --only ID|GLOB ...     run only matching checks (e.g. --only 'agents.*' cmd.rename)
  --report PATH          write the Markdown parity table (e.g. docs/src/parity.md)
  --keep                 keep the previous run's readings in WORK and replace only
                         the selected checks' rows (with --only, to refresh part
                         of a report without re-running everything)
  --list                 list the checks and exit
EOF
}

BIN=$HOME/.local/bin/claude-rs
BRIDGE=
BRIDGE_RUNTIME=
CLAUDE_BIN=
LAUNCHER=$HOME/.local/bin/claude-launch
BASE_BIN=$HOME/.local/share/claude-rs/0.15.1-fork.1/claude-rs
[[ -x $BASE_BIN ]] || BASE_BIN=$HOME/.local/bin/claude-rs
CONFIG_DIR=${PARITY_CONFIG_DIR:-$HOME/.claude-personal}
WORK=${TMPDIR:-/tmp}/claude-rs-parity
SESSIONS_FILE=
SRC_DIR=$REPO_DIR
REPORT=
LIST=0
KEEP=0
ONLY=()

while (($#)); do
  case $1 in
    --bin) BIN=$2; shift 2 ;;
    --bridge) BRIDGE=$2; shift 2 ;;
    --bridge-runtime) BRIDGE_RUNTIME=$2; shift 2 ;;
    --claude) CLAUDE_BIN=$2; shift 2 ;;
    --launcher) LAUNCHER=$2; shift 2 ;;
    --baseline-bin) BASE_BIN=$2; shift 2 ;;
    --config-dir) CONFIG_DIR=$2; shift 2 ;;
    --work) WORK=$2; shift 2 ;;
    --sessions-file) SESSIONS_FILE=$2; shift 2 ;;
    --src) SRC_DIR=$2; shift 2 ;;
    --report) REPORT=$2; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --list) LIST=1; shift ;;
    --only)
      shift
      while (($#)) && [[ $1 != --* ]]; do ONLY+=("$1"); shift; done
      ;;
    -h | --help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# shellcheck source=lib/harness.sh
source "$SUITE_DIR/lib/harness.sh"
# shellcheck source=lib/report.sh
source "$SUITE_DIR/lib/report.sh"
for f in "$SUITE_DIR"/checks/*.sh; do
  # shellcheck source=/dev/null
  source "$f"
done

if ((LIST)); then
  for id in "${CHECK_IDS[@]}"; do printf '%-28s %-9s %s\n' "$id" "${CHECK_GROUP[$id]}" "${CHECK_NAME[$id]}"; done
  exit 0
fi

selected() {
  ((${#ONLY[@]} == 0)) && return 0
  local pat
  for pat in "${ONLY[@]}"; do
    # shellcheck disable=SC2053 # the pattern is a glob on purpose
    [[ $1 == $pat || ${CHECK_GROUP[$1]:-} == "$pat" ]] && return 0
  done
  return 1
}

# ------------------------------------------------------------ preflight

if [[ -z $CLAUDE_BIN ]]; then
  CLAUDE_BIN=$HOME/.local/share/claude/ClaudeCode.app/Contents/MacOS/claude
  [[ -x $CLAUDE_BIN ]] || CLAUDE_BIN=$(command -v claude || true)
fi
if [[ -n $BRIDGE && -z $BRIDGE_RUNTIME ]]; then
  BRIDGE_RUNTIME=$(find "$HOME/.local/share/claude-rs" -maxdepth 2 -name claude-rs-bridge-bun 2>/dev/null | sort -V | tail -1)
fi
for tool in tmux jq node; do
  command -v "$tool" >/dev/null || { echo "missing dependency: $tool" >&2; exit 2; }
done
[[ -x $BIN ]] || { echo "claude-rs binary not executable: $BIN" >&2; exit 2; }
[[ -x $CLAUDE_BIN ]] || { echo "stock Claude Code not found; pass --claude" >&2; exit 2; }
[[ -z $BRIDGE || -f $BRIDGE ]] || { echo "bridge script not found: $BRIDGE" >&2; exit 2; }
[[ -z $BRIDGE || -x $BRIDGE_RUNTIME ]] || { echo "bridge runtime not found; pass --bridge-runtime" >&2; exit 2; }
[[ -d $CONFIG_DIR ]] || { echo "config dir not found: $CONFIG_DIR" >&2; exit 2; }

mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd -P)
case $WORK in
  "$REPO_DIR" | "$REPO_DIR"/* | "$HOME") echo "refusing WORK inside a real tree: $WORK" >&2; exit 2 ;;
esac
SESSIONS_FILE=${SESSIONS_FILE:-$WORK/test-sessions.txt}
RESULTS=$WORK/results.tsv
if ((KEEP)); then
  # Re-run only the selected checks on top of an earlier run's readings:
  # their old rows go, everything else (and the scratch cwds) stays.
  for f in "$RESULTS" "$WORK/timings.tsv"; do
    [[ -f $f ]] || continue
    while IFS=$'\t' read -r id rest; do
      selected "$id" || printf '%s\t%s\n' "$id" "$rest"
    done <"$f" >"$f.keep"
    mv "$f.keep" "$f"
  done
else
  # Every run starts from fresh fixture copies: checks edit files in their
  # cwd. Trust is recorded per path, so recreated directories stay trusted.
  find "$WORK" -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} +
  : >"$RESULTS"
  : >"$WORK/costs.tsv"
  : >"$WORK/timings.tsv"
  : >"$WORK/perf.tsv"
  : >"$WORK/run-sessions.txt"
  touch "$WORK/.run-start"
fi
mkdir -p "$WORK/logs" "$WORK/screens"
touch "$RESULTS" "$WORK/costs.tsv" "$WORK/timings.tsv" "$WORK/perf.tsv" "$WORK/run-sessions.txt" "$WORK/.run-start"
touch "$SESSIONS_FILE"

cat >"$WORK/tmux.conf" <<'EOF'
set -g default-terminal tmux-256color
set -g remain-on-exit on
set -g history-limit 5000
set -s escape-time 10
EOF

# The suite is often launched from inside a Claude Code session; its
# per-session variables (CLAUDECODE, CLAUDE_CODE_SESSION_ID, the messaging
# socket, ...) must not leak into the sessions under test. The tmux server
# inherits this environment, so scrub it before the first tmux call.
while IFS= read -r var; do
  unset "$var"
done < <(compgen -e | grep -E '^(CLAUDE|CLAUDECODE|AI_AGENT|ANTHROPIC_)' || true)
export TERM=${TERM:-xterm-256color}

cleanup() {
  local s
  if tm has-session 2>/dev/null; then
    for s in $(tm list-sessions -F '#{session_name}' 2>/dev/null); do
      [[ $s == _keep ]] || rs_stop "$s"
    done
  fi
  tm kill-server 2>/dev/null || true
  collect_session_ids
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# collect_session_ids: every transcript written under WORK during this run,
# which also catches ids born from /clear or a resume fork.
collect_session_ids() {
  local d f mangled
  for d in "$WORK"/*/; do
    mangled=$(sed -E 's/[^A-Za-z0-9]/-/g' <<<"${d%/}")
    for f in "$CONFIG_DIR/projects/$mangled"/*.jsonl; do
      [[ -f $f && $f -nt $WORK/.run-start ]] && note_session "$(basename "$f" .jsonl)"
    done
  done
}

# A placeholder session keeps the server up between claude-rs sessions.
tm new-session -d -s _keep -x 20 -y 5 'sleep 86400'

RS_VERSION=$("$BIN" --version 2>/dev/null | awk '{print $2}')
BASE_VERSION=$("$BASE_BIN" --version 2>/dev/null | awk '{print $2}')
CC_VERSION=$("$CLAUDE_BIN" --version 2>/dev/null | awk '{print $1}')
STARTED=$(now)
echo "claude-rs $RS_VERSION ($BIN) vs Claude Code $CC_VERSION; work $WORK"

# ------------------------------------------------------------ run

for id in "${CHECK_IDS[@]}"; do
  selected "$id" || continue
  CUR=$id
  RESULT_SET=0
  SCREEN=
  t0=$(now)
  "chk_${id//./_}"
  printf "%s\t%s\n" "$id" $(($(now) - t0)) >>"$WORK/timings.tsv"
  if ((RESULT_SET == 0)); then
    res FAIL "harness: check returned without a reading (last screen: $(composer_line))"
  fi
done

for s in $(tm list-sessions -F '#{session_name}' 2>/dev/null); do
  [[ $s == _keep ]] || rs_stop "$s"
done
collect_session_ids
ELAPSED=$(($(now) - STARTED))
# For the report: totals over every reading kept in WORK (see --keep).
TOTAL_ELAPSED=$(awk -F'\t' '{s += $2} END {print s + 0}' "$WORK/timings.tsv")
COST=$(awk -F'\t' '{s += $2} END {printf "%.4f", s}' "$WORK/costs.tsv")

echo
awk -F'\t' '{n[$2]++} END {for (k in n) printf "%s %d  ", k, n[k]; print ""}' "$RESULTS"
echo "elapsed ${ELAPSED}s; cost \$$COST (sum of each session's Usage tab)"
echo "slowest checks: $(sort -t$'\t' -k2 -nr "$WORK/timings.tsv" | head -5 | awk -F'\t' '{printf "%s %ss  ", $1, $2}')"
echo "sessions created this run (also appended to $SESSIONS_FILE):"
sed 's/^/  /' "$WORK/run-sessions.txt"

if [[ -n $REPORT ]]; then
  write_report "$REPORT"
  echo "report: $REPORT"
fi
