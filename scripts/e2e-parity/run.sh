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
  --claude PATH          stock Claude Code executable. Never a launcher or
                         wrapper script. Default: \$PARITY_CLAUDE_BIN, else the
                         installed ClaudeCode.app binary, else ~/.local/bin/claude
  --baseline-bin PATH    claude-rs to compare performance against (default: the
                         newest installed release, else ~/.local/bin/claude-rs)
  --model ALIAS          model for every session (haiku, sonnet or opus;
                         default haiku, or \$PARITY_MODEL)
  --config-dir DIR       isolated CLAUDE_CONFIG_DIR (or set \$PARITY_CONFIG_DIR);
                         it must already be logged in
  --owner-profile        use \$CLAUDE_CONFIG_DIR (else ~/.claude) as the config
                         directory, for a machine whose login lives in the
                         owner's profile (see README "Which profile")
  --work DIR             scratch root for session cwds, logs and screens
                         (default: a unique new directory under \$TMPDIR)
  --sessions-file PATH   append every created session id here
                         (default: WORK/test-sessions.txt)
  --src DIR              claude-rs source tree, for the app-owned command
                         catalog (default: this repository)
  --only ID|GLOB ...     run only matching checks: an id, a group, or a glob over
                         ids or check files (e.g. --only 'agents.*' cmd.model 10-*)
  --report PATH          write the Markdown parity table (e.g. docs/src/parity.md)
  --keep                 reuse a suite-marked --work directory and replace only
                         the selected checks' rows (with --only, to refresh part
                         of a report without re-running everything)
  --list                 list the checks and exit
EOF
}

BIN=$HOME/.local/bin/claude-rs
BRIDGE=
BRIDGE_RUNTIME=
CLAUDE_BIN=${PARITY_CLAUDE_BIN:-}
BASE_BIN=$(find "$HOME/.local/share/claude-rs" -maxdepth 2 -name claude-rs -type f 2>/dev/null | sort -V | tail -1)
[[ -n $BASE_BIN ]] || BASE_BIN=$HOME/.local/bin/claude-rs
MODEL=${PARITY_MODEL:-haiku}
OWNER_PROFILE=0
CONFIG_DIR=${PARITY_CONFIG_DIR:-}
WORK=
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
    --baseline-bin) BASE_BIN=$2; shift 2 ;;
    --model) MODEL=$2; shift 2 ;;
    --config-dir) CONFIG_DIR=$2; shift 2 ;;
    --owner-profile) OWNER_PROFILE=1; shift ;;
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
  CUR_FILE=$(basename "$f" .sh)
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
    [[ $1 == $pat || ${CHECK_GROUP[$1]:-} == "$pat" || ${CHECK_FILE[$1]:-} == $pat ]] && return 0
  done
  return 1
}

# ------------------------------------------------------------ preflight

# Stock Claude Code is always started as the binary itself. On a machine with a
# launcher that can route to claude-rs (system-config's claude-launch, usually
# behind a `claude` alias) the bare name `claude` may not be stock, so this
# looks for the installed binary the launcher itself runs, then the native
# installer's symlink, and only then PATH. The launcher's own kill switch,
# SC_CLAUDE_TUI=stock, is not needed because the launcher is never invoked.
if [[ -z $CLAUDE_BIN ]]; then
  for CLAUDE_BIN in "$HOME/.local/share/claude/ClaudeCode.app/Contents/MacOS/claude" \
    "$HOME/.local/bin/claude" "$(command -v claude || true)"; do
    [[ -x $CLAUDE_BIN ]] && break
  done
fi
if [[ -n $BRIDGE && -z $BRIDGE_RUNTIME ]]; then
  BRIDGE_RUNTIME=$(find "$HOME/.local/share/claude-rs" -maxdepth 2 -name claude-rs-bridge-bun 2>/dev/null | sort -V | tail -1)
fi
# The footer shows the model by its family name, which the checks match, so
# only the aliases are accepted. /model needs a second model to switch to.
case $MODEL in
  haiku) OTHER_MODEL=sonnet ;;
  sonnet | opus) OTHER_MODEL=haiku ;;
  *) echo "--model takes haiku, sonnet or opus (the footer badge is matched by name): $MODEL" >&2; exit 2 ;;
esac
MODEL_LABEL=${MODEL^}
OTHER_LABEL=${OTHER_MODEL^}
for tool in tmux jq node; do
  command -v "$tool" >/dev/null || { echo "missing dependency: $tool" >&2; exit 2; }
done
[[ -x $BIN ]] || { echo "claude-rs binary not executable: $BIN" >&2; exit 2; }
[[ -x $CLAUDE_BIN ]] || { echo "stock Claude Code not found; pass --claude or set PARITY_CLAUDE_BIN" >&2; exit 2; }
# --version names the product, which a wrapper or another CLI would not.
[[ $("$CLAUDE_BIN" --version 2>/dev/null) == *"(Claude Code)"* ]] || {
  echo "refusing $CLAUDE_BIN: --version does not say (Claude Code)" >&2; exit 2;
}
# A script (shebang) is a launcher or wrapper, not stock: it could route the
# "stock" side of every comparison to claude-rs.
[[ $(head -c 2 "$CLAUDE_BIN") != '#!' ]] || {
  echo "refusing $CLAUDE_BIN: a script is not stock Claude Code; pass the real binary with --claude" >&2; exit 2;
}
[[ -z $BRIDGE || -f $BRIDGE ]] || { echo "bridge script not found: $BRIDGE" >&2; exit 2; }
[[ -z $BRIDGE || -x $BRIDGE_RUNTIME ]] || { echo "bridge runtime not found; pass --bridge-runtime" >&2; exit 2; }
[[ -x $BASE_BIN ]] || { echo "baseline binary not executable: $BASE_BIN" >&2; exit 2; }
[[ -d $SRC_DIR ]] || { echo "source directory not found: $SRC_DIR" >&2; exit 2; }

# 2026-10-09: target processes start in scratch cwd; relative launch paths
# otherwise resolve there instead of the caller's checkout.
absolute_path() { (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd -P)" "$(basename "$1")"); }
BIN=$(absolute_path "$BIN")
CLAUDE_BIN=$(absolute_path "$CLAUDE_BIN")
BASE_BIN=$(absolute_path "$BASE_BIN")
[[ -z $BRIDGE ]] || BRIDGE=$(absolute_path "$BRIDGE")
[[ -z $BRIDGE ]] || BRIDGE_RUNTIME=$(absolute_path "$BRIDGE_RUNTIME")
SRC_DIR=$(cd "$SRC_DIR" && pwd -P)
# CONFIG_ENV is what every child gets to select the profile. With
# --owner-profile and no CLAUDE_CONFIG_DIR set, the default profile is
# ~/.claude and nothing is exported: setting the variable to that same path
# selects a different keychain item and config file path than leaving it unset.
CONFIG_ENV=()
if ((OWNER_PROFILE)); then
  [[ -z $CONFIG_DIR ]] || { echo "--owner-profile and --config-dir are exclusive" >&2; exit 2; }
  ((KEEP == 0)) || { echo "--keep skips the exit cleanup, which --owner-profile needs; use an isolated --config-dir" >&2; exit 2; }
  CONFIG_DIR=${CLAUDE_CONFIG_DIR:-$HOME/.claude}
  [[ -z ${CLAUDE_CONFIG_DIR:-} ]] || CONFIG_ENV=(CLAUDE_CONFIG_DIR="$CONFIG_DIR")
else
  CONFIG_ENV=(CLAUDE_CONFIG_DIR="$CONFIG_DIR")
fi
[[ -n $CONFIG_DIR && -d $CONFIG_DIR ]] || { echo "pass an existing logged-in --config-dir or PARITY_CONFIG_DIR, or --owner-profile" >&2; exit 2; }
CONFIG_DIR=$(cd "$CONFIG_DIR" && pwd -P)
if ((OWNER_PROFILE == 0)); then
  REAL_CLAUDE=$(cd "$HOME/.claude" 2>/dev/null && pwd -P || true)
  REAL_PERSONAL=$(cd "$HOME/.claude-personal" 2>/dev/null && pwd -P || true)
  for real_config in "$REAL_CLAUDE" "$REAL_PERSONAL"; do
    [[ -z $real_config || $CONFIG_DIR != "$real_config" && $CONFIG_DIR != "$real_config"/* ]] || {
      echo "refusing the owner's real Claude config without --owner-profile: $CONFIG_DIR" >&2; exit 2;
    }
  done
fi
if [[ -e $CONFIG_DIR/settings.json ]] &&
  ! jq -e -s 'length == 1 and (.[0] | type == "object")' "$CONFIG_DIR/settings.json" >/dev/null 2>&1; then
  echo "isolated settings.json must contain exactly one JSON object: $CONFIG_DIR/settings.json" >&2
  exit 2
fi
if [[ -z $WORK ]]; then
  ((KEEP == 0)) || { echo "--keep requires an explicit --work" >&2; exit 2; }
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/claude-rs-parity.XXXXXX") || exit 2
else
  [[ ! -L $WORK ]] || { echo "refusing symlinked --work: $WORK" >&2; exit 2; }
  [[ -e $WORK ]] || mkdir "$WORK" || exit 2
  [[ -d $WORK ]] || { echo "work path is not a directory: $WORK" >&2; exit 2; }
fi
WORK=$(cd "$WORK" && pwd -P)
case $WORK in
  "$REPO_DIR" | "$REPO_DIR"/* | "$HOME") echo "refusing WORK inside a real tree: $WORK" >&2; exit 2 ;;
esac
if git -C "$WORK" rev-parse --show-toplevel >/dev/null 2>&1; then
  echo "refusing WORK inside a Git checkout: $WORK" >&2
  exit 2
fi
case $WORK in
  "$CONFIG_DIR" | "$CONFIG_DIR"/*) echo "refusing WORK inside config: $WORK" >&2; exit 2 ;;
esac
case $CONFIG_DIR in
  "$WORK"/*) echo "refusing config inside WORK: $CONFIG_DIR" >&2; exit 2 ;;
esac
if ((KEEP)); then
  [[ -f $WORK/.parity-suite && ! -L $WORK/.parity-suite && $(cat "$WORK/.parity-suite") == parity-suite-v1 ]] || {
    echo "--keep requires a suite-marked work directory: $WORK" >&2; exit 2;
  }
else
  [[ -z $(find "$WORK" -mindepth 1 -maxdepth 1 -print -quit) ]] || {
    echo "work directory is not empty; use a new path or --keep on a suite-marked directory: $WORK" >&2; exit 2;
  }
  printf 'parity-suite-v1\n' >"$WORK/.parity-suite"
fi
SESSIONS_FILE=${SESSIONS_FILE:-$WORK/test-sessions.txt}
RESULTS=$WORK/results.tsv
# A pattern that selects no check would write an empty report over the real one.
n_selected=0
for id in "${CHECK_IDS[@]}"; do selected "$id" && n_selected=$((n_selected + 1)); done
((n_selected > 0)) || { echo "no check matches --only ${ONLY[*]}" >&2; exit 2; }
if ((KEEP)); then
  # Re-run only the selected checks on top of an earlier run's readings:
  # their old rows go, everything else stays. The exit cleanup is skipped
  # (see remove_scratch), so the earlier scratch cwds stay as well.
  for f in "$RESULTS" "$WORK/timings.tsv"; do
    [[ -f $f ]] || continue
    while IFS=$'\t' read -r id rest; do
      selected "$id" || printf '%s\t%s\n' "$id" "$rest"
    done <"$f" >"$f.keep"
    mv "$f.keep" "$f"
  done
else
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
# Only the rows this run adds count toward its cost (a --keep run starts with
# the earlier run's rows in the file).
COST_BASE=$(wc -l <"$WORK/costs.tsv")

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
  export -n "${var?}"
done < <(compgen -e | grep -E '^(CLAUDE|CLAUDECODE|AI_AGENT|ANTHROPIC_)' || true)
unset NO_COLOR
export TERM=xterm-256color COLORTERM=truecolor
# Test sessions are throwaway. A SessionEnd hook that archives transcripts
# shipped 74 of them to the owner's archive on the first live runs
# (2026-10-12); hooks that honour this opt-out exit early. Set after the scrub
# above, which would otherwise remove it.
export CLAUDE_ARCHIVE_SKIP=1

cleanup() {
  local status=$? s
  trap - EXIT INT TERM
  # A failed transport cannot safely drive /usage while unwinding.
  if ((status == 0)) && tm has-session 2>/dev/null; then
    for s in $(tm list-sessions -F '#{session_name}' 2>/dev/null); do
      [[ $s == _keep ]] || rs_stop "$s"
    done
  fi
  tm kill-server 2>/dev/null || true
  # kill-server can leave the socket file behind (seen 2026-10-12).
  rm -f -- "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$SOCKET" "/private/tmp/tmux-$(id -u)/$SOCKET"
  # A claude-rs that outlived its pane is found by the log file it was given.
  pkill -f -- "--log-file $WORK/" 2>/dev/null || true
  collect_session_ids
  ((KEEP)) || remove_scratch
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# remove_scratch: delete what this run created and nothing else. Every
# session ran in a copy of the fixture project under WORK (marked by
# .parity-fixture), so the copy goes, and with it the transcripts and session
# state Claude Code wrote for that cwd and those ids under the config
# directory, which matters when that directory is the owner's profile. WORK
# itself stays: results.tsv, screens/ and logs/ are the evidence.
remove_scratch() {
  local d mangled id i
  # The owner's SessionEnd hooks (a conversation archiver here) read each
  # transcript after its session ends; take it away only once they are done.
  mangled=$(project_dir "$WORK") && mangled=${mangled##*/}
  for ((i = 0; i < 60; i++)); do
    [[ -n $mangled ]] && pgrep -f -- "$mangled" >/dev/null 2>&1 || break
    sleep 0.5
  done
  for d in "$WORK"/*/; do
    [[ -f $d/.parity-fixture ]] || continue
    mangled=$(project_dir "${d%/}") && rm -rf "$mangled"
    rm -rf "${d%/}"
  done
  # Per-session state Claude Code keeps beside the transcripts. Plan files are
  # not here: the fixture's settings send them to .parity/plans in the cwd.
  while IFS= read -r id; do
    [[ $id =~ ^[0-9a-f-]{36}$ ]] || continue
    rm -rf "${CONFIG_DIR:?}/file-history/$id" "${CONFIG_DIR:?}/session-env/$id" "${CONFIG_DIR:?}/tasks/$id"
  done <"$WORK/run-sessions.txt"
}

# collect_session_ids: every transcript written under WORK during this run,
# which also catches ids born from /clear or a resume fork.
collect_session_ids() {
  local d f dir
  for d in "$WORK"/*/; do
    dir=$(project_dir "${d%/}") || continue
    for f in "$dir"/*.jsonl; do
      [[ -f $f && $f -nt $WORK/.run-start ]] && note_session "$(basename "$f" .jsonl)"
    done
  done
}

# A placeholder session keeps the server up between claude-rs sessions.
tm new-session -d -s _keep -x 20 -y 5 'sleep 86400'

RS_VERSION=$("$BIN" --version 2>/dev/null | awk '{print $2}')
BASE_VERSION=$("$BASE_BIN" --version 2>/dev/null | awk '{print $2}')
CC_VERSION=$("$CLAUDE_BIN" --version 2>/dev/null | awk '{print $1}')
SRC_REV=$(git -C "$SRC_DIR" describe --always --dirty 2>/dev/null || echo unknown)
STARTED=$(now)
echo "claude-rs $RS_VERSION ($BIN) vs Claude Code $CC_VERSION ($CLAUDE_BIN); model $MODEL; work $WORK"

# ------------------------------------------------------------ run

for id in "${CHECK_IDS[@]}"; do
  selected "$id" || continue
  CUR=$id
  RESULT_SET=0
  SCREEN=
  ENV_NOTE=
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
COST=$(awk -F'\t' -v n="$COST_BASE" 'NR > n {s += $2} END {printf "%.4f", s}' "$WORK/costs.tsv")

echo
awk -F'\t' '{n[$2]++} END {for (k in n) printf "%s %d  ", k, n[k]; print ""}' "$RESULTS"
echo "model $MODEL; elapsed ${ELAPSED}s; cost \$$COST, summed from the /usage readout of each claude-rs session"
echo "  (the performance checks run no model turns; a session killed before its readout is not counted)"
tail -n +"$((COST_BASE + 1))" "$WORK/costs.tsv" | sort -t$'\t' -k2 -nr | head -5 | awk -F'\t' '{printf "  %-12s $%s\n", $1, $2}'
if awk -v c="$COST" -v b="${PARITY_COST_BUDGET:-0.60}" 'BEGIN {exit !(c > b)}'; then
  echo "WARNING: cost \$$COST is over the \$${PARITY_COST_BUDGET:-0.60} a full run should stay under"
fi
echo "slowest checks: $(sort -t$'\t' -k2 -nr "$WORK/timings.tsv" | head -5 | awk -F'\t' '{printf "%s %ss  ", $1, $2}')"
echo "sessions created this run (also appended to $SESSIONS_FILE):"
sed 's/^/  /' "$WORK/run-sessions.txt"

if [[ -n $REPORT ]]; then
  write_report "$REPORT"
  echo "report: $REPORT"
fi
