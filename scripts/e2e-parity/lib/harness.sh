# shellcheck shell=bash
# shellcheck disable=SC2034 # globals here are read by run.sh and the checks
# Harness for the parity suite: a private tmux server, screen polling,
# claude-rs session lifecycle, and the result log. Sourced by run.sh, which
# sets BIN, CLAUDE_BIN, CONFIG_DIR, WORK, SESSIONS_FILE, RESULTS and friends.

SOCKET=parity-$$-$RANDOM-$RANDOM
COLS=${PARITY_COLS:-120}
ROWS=${PARITY_ROWS:-45}
# Shift+Enter as a kitty-protocol terminal sends it. tmux re-encodes its own
# S-Enter to a bare CR once the app has pushed only the "disambiguate" flag,
# so the raw bytes are the only way to deliver the chord through tmux.
SHIFT_ENTER_HEX=(1b 5b 31 33 3b 32 75)
EMPTY_COMPOSER='❯ Type a message'

tm() { tmux -L "$SOCKET" -f "$WORK/tmux.conf" "$@"; }

now() { date +%s; }

screen() { tm capture-pane -p -t "$1" 2>/dev/null; }
screen_e() { tm capture-pane -p -e -t "$1" 2>/dev/null; }

pane_dead() { [[ $(tm display-message -p -t "$1" '#{pane_dead}' 2>/dev/null) == 1 ]]; }
session_exists() { tm has-session -t "$1" 2>/dev/null; }

# wait_for <sess> <ERE> [timeout]: poll the screen until the pattern shows.
# SCREEN holds the last capture either way, so a failure can quote it.
wait_for() {
  local s=$1 re=$2 t=${3:-20} end
  end=$(($(now) + t))
  while :; do
    SCREEN=$(screen "$s")
    grep -Eq -- "$re" <<<"$SCREEN" && return 0
    (($(now) >= end)) && return 1
    sleep 0.25
  done
}

# wait_gone <sess> <ERE> [timeout]: poll until the pattern is no longer shown.
wait_gone() {
  local s=$1 re=$2 t=${3:-20} end
  end=$(($(now) + t))
  while :; do
    SCREEN=$(screen "$s")
    grep -Eq -- "$re" <<<"$SCREEN" || return 0
    (($(now) >= end)) && return 1
    sleep 0.25
  done
}

# first_match <ERE> [text]: first matching line of SCREEN (or text), trimmed.
first_match() {
  local text=${2-$SCREEN} line
  line=$(grep -E -m1 -- "$1" <<<"$text") || return 1
  sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' <<<"$line"
}

# tool_row <ERE>: the first tool-call row (status glyph first) matching the
# pattern, so evidence never quotes the echoed prompt instead.
tool_row() {
  first_match "^ +[✓✗↗⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] .*($1)"
}

composer_line() { grep -E '^ ?❯ ' <<<"${1-$SCREEN}" | tail -1 | sed -E 's/[[:space:]]+$//'; }

# Footer rows: the mode/model row and the Loc row are the last two non-blank
# lines while chat is shown.
# The status row ("[Mode]  [Model/Effort]  [FAST:..]", which wraps on a
# narrow or busy footer) and the "Loc:" row.
footer() {
  local t=${1-$SCREEN}
  grep -E '^\[[A-Za-z ]+\] ' <<<"$t" | tail -1 | sed -E 's/ {2,}/ /g; s/ +$//'
  grep -E '^Loc: ' <<<"$t" | tail -1
}

keys() { tm send-keys -t "$1" "${@:2}"; }

shift_enter() { tm send-keys -t "$1" -H "${SHIFT_ENTER_HEX[@]}"; }

# 2026-10-09: a raw tmux send-keys -l burst reordered /fast, /btw and
# /rename drafts. tmux paste-buffer -p also corrupted /rename because the
# app did not advertise bracketed paste; explicit 200~/201~ wrappers delivered
# /rename TRANSPORT_CHECK exactly in a fresh TUI.
composer_rows_text() {
  awk '
    /^ ?❯ / { on = 1; sub(/^ ?❯ /, ""); print; next }
    on && (/^\[[A-Za-z ]+\] / || /^Loc: / || /^$/) { exit }
    on { sub(/^   /, ""); print }
  ' <<<"$1"
}

# 2026-10-09: tmux wraps at both spaces (the Write prompt) and within words
# (the Glob/Grep prompt). Compare each visual row to the intended byte span;
# consume a boundary space only when it exists in the intended prompt.
composer_matches_exact() {
  local capture=$1 expected=$2 line offset=0
  local -a rows=()
  mapfile -t rows < <(composer_rows_text "$capture")
  ((${#rows[@]} > 0)) || return 1
  for line in "${rows[@]}"; do
    [[ ${expected:offset:${#line}} == "$line" ]] || return 1
    offset=$((offset + ${#line}))
    ((offset == ${#expected})) && return 0
    [[ ${expected:offset:1} == ' ' ]] && offset=$((offset + 1))
  done
  return 1
}

wait_composer_text() {
  local s=$1 expected=${2//$'\n'/} mode=$3 end
  end=$(($(now) + 8))
  while :; do
    SCREEN=$(screen "$s")
    if composer_matches_exact "$SCREEN" "$expected" ||
      { [[ $mode == suffix ]] && [[ $(composer_rows_text "$SCREEN" | tail -1) == *"$expected" ]]; }; then
      return 0
    fi
    (($(now) >= end)) && return 1
    sleep 0.1
  done
}

# A transport mismatch leaves an unknown draft. Stop the run before another
# check can append to it; the saved screen makes the failure reviewable.
require_composer_text() {
  local s=$1 expected=$2 mode=$3
  wait_composer_text "$s" "$expected" "$mode" && return 0
  SCREEN=$(screen "$s")
  printf '%s\n' "$SCREEN" >"$WORK/screens/${CUR:-input}.transport.txt"
  printf 'input transport mismatch in %s (%s, %s chars); screen: %s\n' \
    "${CUR:-input}" "$mode" "${#expected}" "$WORK/screens/${CUR:-input}.transport.txt" >&2
  exit 3
}

# type_text <sess> <text>: paste literally and confirm it reached the composer.
type_text() {
  local s=$1 text=$2
  # A preceding standalone Escape must expire before the paste ESC prefix.
  # Without this separation, a post-agent-view probe displayed literal [200~.
  sleep 0.1
  tm send-keys -t "$s" -H 1b 5b 32 30 30 7e || exit 3
  tm send-keys -t "$s" -l -- "$text" || exit 3
  tm send-keys -t "$s" -H 1b 5b 32 30 31 7e || exit 3
  require_composer_text "$s" "$text" suffix
}

# type_slow <sess> <text>: one key at a time, for popups that only refresh
# on typed (not pasted) input, such as @-mention and slash completion.
type_slow() {
  local s=$1 text=$2 i
  for ((i = 0; i < ${#text}; i++)); do
    tm send-keys -t "$s" -l -- "${text:i:1}"
    sleep 0.06
  done
  sleep 0.6
}

ere_escape() { sed -E 's/[][\.^$*+?(){}|/]/\\&/g' <<<"$1"; }

# enter_cmd <sess> <text>: type and press Enter once, for commands that open
# a fullscreen view (the composer is hidden, so submit cannot confirm).
enter_cmd() {
  type_text "$1" "$2"
  require_composer_text "$1" "$2" exact
  keys "$1" Enter
}

# submit <sess> <text>: type, Enter, and confirm the draft left the composer.
submit() {
  local s=$1 text=$2
  type_text "$s" "$text"
  require_composer_text "$s" "$text" exact
  keys "$s" Enter
  wait_for "$s" "$EMPTY_COMPOSER" 4
}

# ---------------------------------------------------------------- registry

agents_json() {
  timeout 10 env CLAUDE_CONFIG_DIR="$CONFIG_DIR" "$CLAUDE_BIN" agents --json 2>/dev/null
}

# registry_entry <cwd>: the interactive registry entry whose cwd is <cwd>.
registry_entry() {
  agents_json | jq -c --arg cwd "$1" \
    '[.[] | select(.kind == "interactive" and .cwd == $cwd)] | last // empty'
}

# wait_registry <cwd> <jq-filter> [timeout]: poll until the entry matches.
wait_registry() {
  local cwd=$1 filter=$2 t=${3:-20} end
  end=$(($(now) + t))
  while :; do
    REG=$(registry_entry "$cwd")
    [[ -n $REG ]] && jq -e "$filter" <<<"$REG" >/dev/null 2>&1 && return 0
    (($(now) >= end)) && return 1
    sleep 0.5
  done
}

# wait_idle <sess> [timeout]: the turn is over once the child's registry
# entry reports idle and no spinner row is left on screen.
wait_idle() {
  local s=$1 t=${2:-90}
  wait_registry "$(cwd_of "$s")" '.status == "idle"' "$t" || return 1
  wait_gone "$s" '^[[:space:]]*│? ?[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] ' 10
}

# ---------------------------------------------------------------- sessions

cwd_of() { printf '%s/%s\n' "$WORK" "$1"; }

# prepare_cwd <name>: a fresh scratch project with the fixture copied in.
prepare_cwd() {
  local cwd
  cwd=$(cwd_of "$1")
  if [[ -e $cwd || -L $cwd ]]; then
    [[ -d $cwd && ! -L $cwd && -f $cwd/.parity-fixture && ! -L $cwd/.parity-fixture && $(cat "$cwd/.parity-fixture") == parity-fixture-v1 ]] || {
      echo "refusing to replace unowned fixture cwd: $cwd" >&2
      return 1
    }
    rm -rf "$cwd"
  fi
  mkdir -p "$cwd"
  cp -R "$SUITE_DIR/fixtures/project/." "$cwd/"
  # Stored as dot-claude because the repository ignores .claude/.
  mv "$cwd/dot-claude" "$cwd/.claude"
  # .mcp.json needs the absolute path of the echo server.
  jq -n --arg srv "$SUITE_DIR/fixtures/mcp-echo.mjs" \
    '{mcpServers: {parity: {command: "node", args: [$srv]}}}' >"$cwd/.mcp.json"
  printf 'parity-fixture-v1\n' >"$cwd/.parity-fixture"
}

rs_env() {
  RS_ENV=(
    CLAUDE_CONFIG_DIR="$CONFIG_DIR"
    CLAUDE_CODE_EXECUTABLE="$CLAUDE_BIN"
    CLAUDE_RUST_NO_UPDATE_CHECK=1
    EDITOR="$SUITE_DIR/fixtures/editor.sh"
    VISUAL="$SUITE_DIR/fixtures/editor.sh"
  )
  if [[ -n ${BRIDGE:-} ]]; then
    RS_ENV+=(CLAUDE_RS_AGENT_BRIDGE="$BRIDGE" CLAUDE_RS_AGENT_BRIDGE_RUNTIME="$BRIDGE_RUNTIME")
  fi
}

rs_cmd() {
  rs_env
  RS_CMD=(env "${RS_ENV[@]}" "$BIN" --no-update-check
    --enable-logs --diagnostics-preset bridge --log-file "$WORK/logs/$1.log" --log-append)
}

# rs_start <sess> [claude-rs args...]: start claude-rs in its scratch cwd and
# wait for the composer. Answers claude-rs's own trust prompt (Yes is the
# default) because every cwd is a throwaway directory under WORK.
rs_start() {
  local s=$1
  shift
  local cwd
  cwd=$(cwd_of "$s")
  [[ -d $cwd ]] || prepare_cwd "$s"
  session_exists "$s" && tm kill-session -t "$s"
  rs_cmd "$s"
  tm new-session -d -s "$s" -x "$COLS" -y "$ROWS" -c "$cwd" "${RS_CMD[@]}" "$@"
  rs_wait_ready "$s"
}

rs_wait_ready() {
  local s=$1
  if ! wait_for "$s" "$EMPTY_COMPOSER|Trust this project directory|❯ " 60; then
    return 1
  fi
  if grep -q 'Trust this project directory' <<<"$SCREEN"; then
    keys "$s" Enter
  fi
  wait_for "$s" "Loc: " 60 || return 1
  wait_registry "$(cwd_of "$s")" '.sessionId != null' 30 || return 1
  note_session "$(jq -r .sessionId <<<"$REG")"
}

note_session() {
  [[ -n $1 && $1 != null ]] || return 0
  grep -qx -- "$1" "$SESSIONS_FILE" 2>/dev/null || printf '%s\n' "$1" >>"$SESSIONS_FILE"
  grep -qx -- "$1" "$WORK/run-sessions.txt" 2>/dev/null || printf '%s\n' "$1" >>"$WORK/run-sessions.txt"
}

# rs_cost <sess>: read the session's cost from the Usage tab and log it.
rs_cost() {
  local s=$1 cost
  pane_dead "$s" && return 0
  keys "$s" Escape
  # A leftover draft would be submitted with "/usage" appended; Ctrl+C
  # clears it (only when non-empty: on an empty prompt it quits).
  if ! wait_for "$s" "$EMPTY_COMPOSER" 3; then
    grep -Eq '^ ?❯ ' <<<"$SCREEN" || return 0
    keys "$s" C-c
    wait_for "$s" "$EMPTY_COMPOSER" 3 || return 0
  fi
  enter_cmd "$s" "/usage"
  if wait_for "$s" '\$[0-9.]+ cost' 15; then
    cost=$(grep -Eo '\$[0-9.]+ cost' <<<"$SCREEN" | head -1 | tr -dc '0-9.')
    printf '%s\t%s\n' "$s" "$cost" >>"$WORK/costs.tsv"
  fi
  keys "$s" Escape
  wait_for "$s" "$EMPTY_COMPOSER" 5 || true
}

# rs_stop <sess>: log the cost, quit with Ctrl+Q, and drop the tmux session.
rs_stop() {
  local s=$1 i
  session_exists "$s" || return 0
  if ! pane_dead "$s"; then
    rs_cost "$s"
    keys "$s" C-q
    for ((i = 0; i < 40; i++)); do
      pane_dead "$s" && break
      sleep 0.25
    done
  fi
  tm kill-session -t "$s" 2>/dev/null || true
}

# ------------------------------------------------------- launcher path

# launcher_bin_dir: a directory to put first on PATH so that claude-launch's
# `command -v claude-rs` finds the binary under test. The launcher only runs
# claude-rs from a release layout (package.json, the claude-rs-bridge-bun
# runtime, agent-sdk/dist with the EAGAIN fix's scWriteSync marker,
# node_modules), so a source build is staged into one: its binary and dist
# copied, the runtime and node_modules linked from the installed release.
launcher_bin_dir() {
  local dir=$WORK/launcher-bin real root stage release
  if [[ -e $dir/claude-rs ]]; then
    printf '%s\n' "$dir"
    return 0
  fi
  mkdir -p "$dir"
  real=$(readlink -f "$BIN")
  root=$(dirname "$real")
  if [[ -f $root/package.json && -x $root/claude-rs-bridge-bun && -f $root/agent-sdk/dist/bridge.js ]]; then
    ln -s "$real" "$dir/claude-rs"
  else
    [[ -n $BRIDGE && -x $BRIDGE_RUNTIME ]] || return 1
    stage=$WORK/stage
    release=$(dirname "$(readlink -f "$BRIDGE_RUNTIME")")
    mkdir -p "$stage/agent-sdk"
    cp "$real" "$stage/claude-rs"
    ln -s "$BRIDGE_RUNTIME" "$stage/claude-rs-bridge-bun"
    cp "$release/package.json" "$stage/package.json"
    cp -R "$(dirname "$BRIDGE")" "$stage/agent-sdk/dist"
    cp "$release/agent-sdk/package.json" "$stage/agent-sdk/package.json"
    ln -s "$release/node_modules" "$stage/node_modules"
    ln -s "$stage/claude-rs" "$dir/claude-rs"
  fi
  printf '%s\n' "$dir"
}

# launch_start <sess> [claude-launch args...]: start a session the way the
# owner does, through system-config's claude-launch.
launch_start() {
  local s=$1 dir cwd
  shift
  [[ -n $LAUNCHER && -x $LAUNCHER ]] || return 2
  dir=$(launcher_bin_dir) || return 2
  cwd=$(cwd_of "$s")
  [[ -d $cwd ]] || prepare_cwd "$s"
  session_exists "$s" && tm kill-session -t "$s"
  tm new-session -d -s "$s" -x "$COLS" -y "$ROWS" -c "$cwd" \
    env CLAUDE_CONFIG_DIR="$CONFIG_DIR" PATH="$dir:$PATH" "$LAUNCHER" "$@"
  rs_wait_ready "$s"
}

# ensure <sess> [args...]: reuse a live shared session or start it.
ensure() {
  local s=$1
  shift
  if session_exists "$s" && ! pane_dead "$s"; then
    keys "$s" Escape
    return 0
  fi
  rs_start "$s" "$@"
}

# turn <sess> <prompt> <ERE> [timeout]: one model turn; succeed when the
# pattern shows and the session is idle again.
turn() {
  local s=$1 prompt=$2 re=$3 t=${4:-90}
  submit "$s" "$prompt" || return 1
  wait_for "$s" "$re" "$t" || return 1
  wait_idle "$s" "$t" || true
  SCREEN=$(screen "$s")
  grep -Eq -- "$re" <<<"$SCREEN"
}

# ---------------------------------------------------------------- results

declare -a CHECK_IDS=()
declare -A CHECK_GROUP=() CHECK_NAME=() CHECK_STOCK=()
CUR=

# def <id> <group> <functionality> <stock behaviour>
def() {
  CHECK_IDS+=("$1")
  CHECK_GROUP[$1]=$2
  CHECK_NAME[$1]=$3
  CHECK_STOCK[$1]=$4
}

# res <PASS|FAIL|GAP|SKIP> <evidence>: record the current check's reading.
res() {
  local status=$1 ev=$2
  ev=$(tr '\n\t' '  ' <<<"$ev" | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//' | cut -c1-220)
  printf '%-5s %-28s %s\n' "$status" "$CUR" "$ev"
  printf '%s\t%s\t%s\n' "$CUR" "$status" "$ev" >>"$RESULTS"
  RESULT_SET=1
}

# shot <label>: keep the screen a check judged by, for a human to re-read.
shot() { printf '%s\n' "$SCREEN" >"$WORK/screens/$CUR${1:+.$1}.txt"; }
