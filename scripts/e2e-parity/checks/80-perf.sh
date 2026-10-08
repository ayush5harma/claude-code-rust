# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Performance, measured the same way for three targets: the claude-rs binary
# under test ("test"), the installed release as the regression baseline
# ("base"), and the stock Claude Code TUI run directly ("stock"). No model
# turns. claude-rs always runs the stock binary as its session engine, so its
# process tree is claude-rs + the Bun bridge + the stock child (and the MCP
# servers that child starts, as stock's own tree does); its rendering and
# input path are its own.

PERF_TARGETS=(test base stock)
PERF_LAUNCHES=3
PERF_IDLE_SECONDS=${PARITY_PERF_IDLE:-60}
PERF_TYPED=perfinput0123456789abcdefghijklmnopqrst
PERF_DONE=
declare -A PERF=()

ms_now() {
  local t=$EPOCHREALTIME
  echo $((${t/./} / 1000))
}

median() { sort -n | awk '{v[NR] = $1} END {if (NR) print v[int((NR + 1) / 2)]}'; }

# tree_pids <pid>: the pid and all its descendants.
tree_pids() {
  local p
  echo "$1"
  for p in $(pgrep -P "$1" 2>/dev/null); do tree_pids "$p"; done
}

# cpu_by_pid <pids...>: "pid seconds" of cumulative CPU (ps time is [H:]M:SS.ss).
cpu_by_pid() {
  (($#)) || return 0
  ps -o pid=,time= -p "$(paste -sd, - <<<"$(printf '%s\n' "$@")")" 2>/dev/null |
    awk '{n = split($2, a, ":"); s = 0; for (i = 1; i <= n; i++) s = s * 60 + a[i]; print $1, s}'
}

rss_mb() {
  (($#)) || { echo 0; return; }
  ps -o rss= -p "$(paste -sd, - <<<"$(printf '%s\n' "$@")")" 2>/dev/null | awk '{s += $1} END {printf "%.0f", s / 1024}'
}

perf_cwd() { cwd_of "perf-$1"; }

perf_cmd() {
  case $1 in
    test)
      rs_env
      PERF_CMD=(env "${RS_ENV[@]}" "$BIN" --no-update-check --model haiku)
      ;;
    base)
      PERF_CMD=(env CLAUDE_CONFIG_DIR="$CONFIG_DIR" CLAUDE_CODE_EXECUTABLE="$CLAUDE_BIN"
        CLAUDE_RUST_NO_UPDATE_CHECK=1 "$BASE_BIN" --no-update-check --model haiku)
      ;;
    stock)
      PERF_CMD=(env CLAUDE_CONFIG_DIR="$CONFIG_DIR" "$CLAUDE_BIN" --model haiku)
      ;;
  esac
}

perf_ready_re() {
  # Stock's prompt row starts at column 0 with ❯ and a no-break space; its
  # dialogs indent their ❯.
  if [[ $1 == stock ]]; then echo '^❯'; else echo "$EMPTY_COMPOSER"; fi
}

# perf_launch <target>: start it and print "ready_ms registered_ms".
# The first launch in a fresh cwd answers the trust prompt (not timed).
perf_launch() {
  local t=$1 s=perf-$1 cwd t0 ready="" reg="" re end
  cwd=$(perf_cwd "$t")
  mkdir -p "$cwd"
  perf_cmd "$t"
  re=$(perf_ready_re "$t")
  session_exists "$s" && perf_kill "$s"
  t0=$(ms_now)
  tm new-session -d -s "$s" -x "$COLS" -y "$ROWS" -c "$cwd" "${PERF_CMD[@]}"
  end=$(($(now) + 90))
  while (($(now) < end)) && [[ -z $ready || -z $reg ]]; do
    SCREEN=$(screen "$s")
    if grep -q 'Trust this project directory' <<<"$SCREEN"; then
      keys "$s" Enter
      sleep 0.5
      continue
    fi
    if grep -q 'Yes, I trust this folder' <<<"$SCREEN"; then
      keys "$s" Down
      sleep 0.2
      keys "$s" Enter
      sleep 0.5
      continue
    fi
    [[ -z $ready ]] && grep -Eq -- "$re" <<<"$SCREEN" && ready=$(($(ms_now) - t0))
    if [[ -z $reg ]]; then
      REG=$(registry_entry "$cwd")
      [[ -n $REG ]] && reg=$(($(ms_now) - t0)) && note_session "$(jq -r .sessionId <<<"$REG")"
    fi
    sleep 0.05
  done
  echo "${ready:-NA} ${reg:-NA}"
}

# perf_kill <sess>: end the session and wait for its whole tree to exit.
perf_kill() {
  local s=$1 pid pids i
  pid=$(tm display-message -p -t "$s" '#{pane_pid}' 2>/dev/null)
  pids=$( [[ -n $pid ]] && tree_pids "$pid")
  tm kill-session -t "$s" 2>/dev/null
  for ((i = 0; i < 40; i++)); do
    # shellcheck disable=SC2086 # a pid list
    [[ -z $pids ]] || ! ps -p "$(paste -sd, - <<<"$pids")" >/dev/null 2>&1 && break
    sleep 0.25
  done
}

# perf_measure <target>: launches, idle CPU and RSS, input latency.
perf_measure() {
  local t=$1 s=perf-$1 i r ready=() regd=() pid before after cpu0 cpu1 lat=() t0 rss
  perf_launch "$t" >/dev/null # warm-up: trust prompt, disk cache
  for ((i = 0; i < PERF_LAUNCHES; i++)); do
    r=$(perf_launch "$t")
    ready+=("${r% *}")
    regd+=("${r#* }")
    ((i < PERF_LAUNCHES - 1)) && perf_kill "$s"
  done
  PERF[launch_ready.$t]=$(printf '%s\n' "${ready[@]}" | grep -v NA | median)
  PERF[launch_registered.$t]=$(printf '%s\n' "${regd[@]}" | grep -v NA | median)

  # Idle: CPU seconds the whole tree burns while the prompt sits there.
  pid=$(tm display-message -p -t "$s" '#{pane_pid}')
  local -A c0=()
  local p sec tree=()
  mapfile -t tree < <(tree_pids "$pid")
  while read -r p sec; do c0[$p]=$sec; done < <(cpu_by_pid "${tree[@]}")
  sleep "$PERF_IDLE_SECONDS"
  cpu1=0
  mapfile -t tree < <(tree_pids "$pid")
  while read -r p sec; do
    cpu1=$(awk -v a="$cpu1" -v b="$sec" -v z="${c0[$p]:-0}" 'BEGIN {printf "%.2f", a + b - z}')
  done < <(cpu_by_pid "${tree[@]}")
  PERF[idle_cpu.$t]=$cpu1
  PERF[rss.$t]=$(rss_mb "${tree[@]}")
  PERF[procs.$t]=${#tree[@]}

  # Input latency: a 40-character send-keys until it shows on screen.
  for ((i = 0; i < 5; i++)); do
    t0=$(ms_now)
    tm send-keys -t "$s" -l -- "$PERF_TYPED"
    while ! grep -q "$PERF_TYPED" <<<"$(screen "$s")"; do
      (($(ms_now) - t0 > 10000)) && break
    done
    lat+=($(($(ms_now) - t0)))
    keys "$s" C-c
    wait_gone "$s" "$PERF_TYPED" 5 || true
    sleep 0.3
  done
  PERF[input_latency.$t]=$(printf '%s\n' "${lat[@]}" | median)
  perf_kill "$s"
}

# perf_resume <target> <session id> <cwd> <marker>: time a resume of a long
# transcript until its last prompt shows; median of 3.
perf_resume() {
  local t=$1 id=$2 cwd=$3 marker=$4 s=perf-resume-$1 i t0 times=() re
  perf_cmd "$t"
  re=$(perf_ready_re "$t")
  for ((i = 0; i < 3; i++)); do
    t0=$(ms_now)
    tm new-session -d -s "$s" -x "$COLS" -y "$ROWS" -c "$cwd" "${PERF_CMD[@]}" -r "$id"
    if wait_for "$s" "$(ere_escape "$marker")" 60 && wait_for "$s" "$re" 30; then
      times+=($(($(ms_now) - t0)))
    fi
    perf_kill "$s"
  done
  printf '%s\n' "${times[@]}" | median
}

# perf_long_transcript: the longest transcript the suite has written under
# this WORK (normally the main session's), as "id cwd marker lines"; marker
# is the start of its last plain-text prompt. Compacted transcripts are left
# out: a resume shows only what follows the compaction.
perf_long_transcript() {
  local best="" best_n=0 f n cwd prefix marker
  prefix="$CONFIG_DIR/projects/$(sed -E 's/[^A-Za-z0-9]/-/g' <<<"$WORK")-"
  for f in "$prefix"*/*.jsonl; do
    [[ -f $f ]] || continue
    grep -q '"compact_boundary"' "$f" && continue
    n=$(wc -l <"$f")
    ((n > best_n)) && best_n=$n && best=$f
  done
  [[ -n $best ]] || return 1
  f=$best
  cwd=$(jq -r 'select(.cwd != null) | .cwd' "$f" 2>/dev/null | head -1)
  [[ $cwd == "$WORK"/* ]] || return 1
  mkdir -p "$cwd"
  # SDK sessions store a prompt as text blocks, stock ones as a string.
  marker=$(jq -r 'select(.type == "user" and .isMeta != true and .isCompactSummary != true)
      | .message.content
      | if type == "string" then . else ([.[]? | select(.type == "text") | .text] | join(" ")) end
      | select(length > 0 and (startswith("<") | not))' "$f" 2>/dev/null |
    tail -1 | cut -c1-30)
  [[ -n $marker ]] || return 1
  printf '%s\t%s\t%s\t%s\n' "$(basename "$f" .jsonl)" "$cwd" "$marker" "$best_n"
}

perf_collect() {
  [[ -n $PERF_DONE ]] && return 0
  PERF_DONE=1
  PERF[load]=$(sysctl -n vm.loadavg 2>/dev/null | awk '{print $2}')
  local t same=""
  [[ $(readlink -f "$BIN") == "$(readlink -f "$BASE_BIN")" && -z $BRIDGE ]] && same=1
  for t in "${PERF_TARGETS[@]}"; do
    if [[ $t == base && -n $same ]]; then
      local k
      for k in launch_ready launch_registered idle_cpu rss procs input_latency; do
        PERF[$k.base]=${PERF[$k.test]}
      done
      PERF[same]=1
      continue
    fi
    perf_measure "$t"
  done
  PERF[load_after]=$(sysctl -n vm.loadavg 2>/dev/null | awk '{print $2}')
  local long id cwd marker lines
  if long=$(perf_long_transcript); then
    IFS=$'\t' read -r id cwd marker lines <<<"$long"
    # The session that wrote it must not be running while it is resumed.
    for t in $(tm list-sessions -F '#{session_name}' 2>/dev/null); do
      [[ $(cwd_of "$t") == "$cwd" ]] && rs_stop "$t"
    done
    PERF[resume_lines]=$lines
    for t in "${PERF_TARGETS[@]}"; do
      if [[ $t == base && -n $same ]]; then PERF[resume_render.base]=${PERF[resume_render.test]}; continue; fi
      PERF[resume_render.$t]=$(perf_resume "$t" "$id" "$cwd" "$marker")
    done
  fi
}

# perf_row <metric> <unit> <slack> <label>: record the three values and read
# the result: FAIL when the build under test is worse than stock by more than
# 10% plus the slack; regressions against the baseline are named either way.
perf_row() {
  local m=$1 unit=$2 slack=$3 tv bv sv verdict note=""
  perf_collect
  tv=${PERF[$m.test]:-}
  bv=${PERF[$m.base]:-}
  sv=${PERF[$m.stock]:-}
  printf '%s\t%s\t%s\t%s\t%s\n' "$m" "${tv:-n/a}" "${bv:-n/a}" "${sv:-n/a}" "$unit" >>"$WORK/perf.tsv"
  if [[ -z $tv || -z $sv ]]; then
    res SKIP "not measured (test '${tv:-}', stock '${sv:-}')"
    return
  fi
  worse() { awk -v a="$1" -v b="$2" -v s="$slack" 'BEGIN {exit !(a > b * 1.10 + s)}'; }
  if [[ -n $bv && -z ${PERF[same]:-} ]] && worse "$tv" "$bv"; then
    note="; regressed against the installed release ($bv $unit)"
  fi
  if worse "$tv" "$sv"; then
    verdict=FAIL
    note="claude-rs is worse than stock: $tv vs $sv $unit$note"
  else
    verdict=PASS
    note="claude-rs $tv vs stock $sv $unit$note"
  fi
  [[ $m == resume_render ]] && note+="; ${PERF[resume_lines]}-line transcript"
  [[ $m == rss || $m == idle_cpu ]] && note+="; processes: rs ${PERF[procs.test]}, stock ${PERF[procs.stock]}"
  res "$verdict" "$note (load avg ${PERF[load]})"
}

def perf.launch_ready perf "Launch to an input-ready prompt" "Median of 3 launches, ms"
chk_perf_launch_ready() { perf_row launch_ready ms 150; }

def perf.launch_registered perf "Launch to the session engine registered" "Median of 3, ms (entry in \`claude agents --json\`)"
chk_perf_launch_registered() { perf_row launch_registered ms 150; }

def perf.idle_cpu perf "Idle CPU, whole process tree" "CPU seconds over ${PERF_IDLE_SECONDS}s at the prompt"
chk_perf_idle_cpu() { perf_row idle_cpu "CPU s" 0.5; }

def perf.rss perf "Resident memory, whole process tree" "MB at the end of the idle window"
chk_perf_rss() { perf_row rss MB 20; }

def perf.input_latency perf "Input latency" "40 characters from send-keys to the screen, median of 5, ms"
chk_perf_input_latency() { perf_row input_latency ms 15; }

def perf.resume_render perf "Resume a long transcript" "Launch with -r until its last prompt shows, median of 3, ms"
chk_perf_resume_render() {
  perf_collect
  if [[ -z ${PERF[resume_lines]:-} ]]; then
    printf '%s\t%s\t%s\t%s\t%s\n' resume_render n/a n/a n/a ms >>"$WORK/perf.tsv"
    res SKIP "no transcript from this run to resume (run with the other groups)"
    return
  fi
  perf_row resume_render ms 150
}
