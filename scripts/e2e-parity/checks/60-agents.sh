# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# The session registry and the agent view. Stock (2.1.293): the footer's last
# row ends with a dim "← N agents" while the input is empty, N counting the
# background sessions of `claude agents --json`; Left on an empty prompt opens
# the agent view, whose standalone form (`claude agents`) exits on Esc.
#
# One background job is created under the suite's isolated config only when a
# hint check needs it. View checks only read rows and leave with Esc.

PARITY_BG_ID=
PARITY_BG_ATTEMPTED=
PARITY_BG_ERROR=
PARITY_BG_NAME=
PARITY_BG_CWD=

owned_background_entry() {
  [[ -n $PARITY_BG_NAME && -f $WORK/agent-hint-before.json ]] || return 1
  agents_json | jq -c --arg name "$PARITY_BG_NAME" --arg cwd "$PARITY_BG_CWD" \
    --slurpfile before "$WORK/agent-hint-before.json" '
      [.[] | select(.kind == "background" and .name == $name and .cwd == $cwd)
        | select(.id as $id | ($before[0] | any(.[]; .id == $id) | not))]
      | if length == 1 then .[0] else empty end'
}

# 2026-10-09: stock --background refused the isolated fixture cwd as
# untrusted. Accept its own trust dialog in a prompt-only stock TUI first.
trust_background_cwd() {
  local s=agent-hint-trust end
  tm new-session -d -s "$s" -x "$COLS" -y "$ROWS" -c "$PARITY_BG_CWD" \
    env CLAUDE_CONFIG_DIR="$CONFIG_DIR" "$CLAUDE_BIN" --model haiku || return 1
  end=$(($(now) + 60))
  while (($(now) < end)); do
    SCREEN=$(screen "$s")
    if grep -q 'Yes, I trust this folder' <<<"$SCREEN"; then
      keys "$s" Down
      sleep 0.2
      keys "$s" Enter
      wait_gone "$s" 'Yes, I trust this folder' 5 || break
    elif grep -q '^❯' <<<"$SCREEN"; then
      tm kill-session -t "$s" || return 1
      ! session_exists "$s"
      return
    fi
    pane_dead "$s" && break
    sleep 0.2
  done
  printf '%s\n' "$SCREEN" >"$WORK/screens/agent-hint-trust.txt"
  tm kill-session -t "$s" 2>/dev/null || true
  return 1
}

start_background_fixture() {
  if [[ -n $PARITY_BG_ATTEMPTED ]]; then
    [[ -n $PARITY_BG_ID && -z $PARITY_BG_ERROR ]]
    return
  fi
  PARITY_BG_ATTEMPTED=1
  local before output entry id end state
  PARITY_BG_CWD=$(cwd_of agent-hint)
  prepare_cwd agent-hint || { PARITY_BG_ERROR='fixture cwd is not owned by the suite'; return 1; }
  trust_background_cwd || { PARITY_BG_ERROR='stock fixture cwd trust did not reach its composer'; return 1; }
  PARITY_BG_NAME="parity-hint-$(basename "$WORK" | tr -cd 'A-Za-z0-9')-$$"
  before=$WORK/agent-hint-before.json
  agents_json >"$before" && jq -e 'type == "array"' "$before" >/dev/null || {
    PARITY_BG_ERROR='isolated agents --json did not return an array before launch'; return 1;
  }
  # Stock --help on 2026-10-09 accepts manual for its default approval mode.
  output=$(cd "$PARITY_BG_CWD" && timeout 30 env CLAUDE_CONFIG_DIR="$CONFIG_DIR" "$CLAUDE_BIN" \
    --background --model haiku --name "$PARITY_BG_NAME" --permission-mode manual \
    'Use AskUserQuestion to ask whether to continue, with Yes and No options. Wait for the answer.' 2>&1) || {
    PARITY_BG_ERROR="background launch failed: $(tr '\n' ' ' <<<"$output" | cut -c1-120)"
    return 1
  }
  end=$(($(now) + 90))
  while (($(now) < end)); do
    entry=$(owned_background_entry 2>/dev/null) || entry=
    id=$(jq -r '.id // empty' <<<"$entry" 2>/dev/null)
    if [[ $id =~ ^[A-Za-z0-9-]+$ ]]; then
      PARITY_BG_ID=$id
      state=$(jq -r '.state // empty' <<<"$entry")
      # Stock derives blocked from the job block metadata; raw state can
      # remain working (measured on 2.1.295). Use its registry verdict.
      if [[ -f $CONFIG_DIR/jobs/$id/state.json && $state == blocked ]] &&
        grep -Fq -- "$id" <<<"$output"; then
        return 0
      fi
    fi
    sleep 0.5
  done
  PARITY_BG_ERROR="owned job did not reach blocked AskUserQuestion state (id ${PARITY_BG_ID:-missing})"
  return 1
}

stop_background_fixture() {
  if [[ -z $PARITY_BG_ID && -n $PARITY_BG_ATTEMPTED ]]; then
    local entry
    entry=$(owned_background_entry 2>/dev/null) || entry=
    PARITY_BG_ID=$(jq -r '.id // empty' <<<"$entry" 2>/dev/null)
  fi
  [[ $PARITY_BG_ID =~ ^[A-Za-z0-9-]+$ ]] || return 0
  timeout 20 env CLAUDE_CONFIG_DIR="$CONFIG_DIR" "$CLAUDE_BIN" stop "$PARITY_BG_ID" >/dev/null 2>&1 ||
    echo "could not stop owned background job $PARITY_BG_ID" >&2
}

def agents.registry agents "Session in \`claude agents --json\`" "Interactive entry with name and status"
chk_agents_registry() {
  ensure_main || { res FAIL "main session did not start"; return; }
  REG=$(registry_entry "$(cwd_of main)")
  if [[ -n $REG ]] && jq -e '.name and .status and .pid' <<<"$REG" >/dev/null; then
    res PASS "$(jq -c '{kind, name, status, pid}' <<<"$REG")"
  else
    res FAIL "no interactive registry entry for the main cwd"
  fi
}

# background_counts: "N K W" = background sessions, how many await input
# (state blocked), how many are working.
background_counts() {
  agents_json | jq -r '[.[] | select(.kind == "background")] |
    "\(length) \([.[] | select(.state == "blocked")] | length) \([.[] | select(.state == "working" or .state == "running" or .state == "busy")] | length)"'
}

def agents.hint agents "\`← N agents\` in the footer" "Dim hint while the input is empty, from the background sessions"
chk_agents_hint() {
  ensure_main || { res FAIL "main session did not start"; return; }
  start_background_fixture || { res FAIL "$PARITY_BG_ERROR"; return; }
  local n k w want noun
  read -r n k w <<<"$(background_counts)"
  if [[ -z $n ]]; then
    res FAIL "\`claude agents --json\` did not answer"
    return
  fi
  ((n > 0)) || { res FAIL "owned background fixture is missing from isolated agents --json"; return; }
  noun=agents
  ((n == 1)) && noun=agent
  want="← $n $noun"
  wait_for main "$EMPTY_COMPOSER" 5 || true
  if wait_for main "$want" 15; then
    local row
    row=$(first_match "$want")
    if ((k > 0)) && ! grep -q "$k awaiting input" <<<"$row"; then
      res FAIL "'$row' lacks '· $k awaiting input' ($k blocked)"
    else
      res PASS "'$row' ($n background, $k blocked, $w working)"
    fi
  else
    res GAP "no '$want' in the footer ($n background, $k blocked): $(footer | tail -1 | sed -E 's/ {2,}/ … /')"
  fi
}

def agents.hint_typing agents "Hint hides while typing" "Only shown while the input is empty"
chk_agents_hint_typing() {
  ensure_main || { res FAIL "main session did not start"; return; }
  start_background_fixture || { res FAIL "$PARITY_BG_ERROR"; return; }
  local n
  read -r n _ _ <<<"$(background_counts)"
  ((${n:-0} > 0)) || { res FAIL "owned background fixture is missing from isolated agents --json"; return; }
  if ! wait_for main '← [0-9]+ agent' 5; then
    res GAP "no agent hint to begin with"
    return
  fi
  type_text main "pm_hint_draft"
  if wait_gone main '← [0-9]+ agent' 3; then
    res PASS "hint hidden while the draft is non-empty"
  else
    res FAIL "hint still shown with text in the composer"
  fi
  keys main C-c
  wait_for main "$EMPTY_COMPOSER" 3 || true
}

AGENT_VIEW_RE='^❯ describe a task for a new session'

def agents.left_opens agents "← on an empty prompt opens the agent view" "Agent view (list, peek, attach, dispatch)"
chk_agents_left_opens() {
  ensure_main || { res FAIL "main session did not start"; return; }
  wait_for main "$EMPTY_COMPOSER" 5 || true
  keys main Left
  if wait_for main "$AGENT_VIEW_RE" 15; then
    AGENT_VIEW_OPENED=1
    shot opened
    res PASS "agent view: $(first_match "$AGENT_VIEW_RE")"
  else
    AGENT_VIEW_OPENED=
    res GAP "Left on the empty prompt did nothing visible: $(composer_line)"
  fi
}

def agents.view_returns agents "Leaving the agent view returns to the session" "Esc returns to the conversation"
chk_agents_view_returns() {
  if [[ -z ${AGENT_VIEW_OPENED:-} ]]; then
    ensure_main || { res FAIL "main session did not start"; return; }
    keys main Left
    if ! wait_for main "$AGENT_VIEW_RE" 15; then
      res GAP "the agent view cannot be opened, so there is nothing to return from"
      return
    fi
    shot opened
  fi
  AGENT_VIEW_OPENED=
  keys main Escape
  if ! wait_for main "$EMPTY_COMPOSER" 10; then
    # Stock's own way out of a standalone agent view: Ctrl+C twice.
    keys main C-c
    sleep 0.3
    keys main C-c
  fi
  if wait_for main "$EMPTY_COMPOSER" 10 && wait_for main 'Loc: ' 3; then
    REG=$(registry_entry "$(cwd_of main)")
    shot returned
    res PASS "back in claude-rs; session still registered: $(jq -c '{status}' <<<"$REG")"
  else
    shot
    res FAIL "claude-rs did not come back after leaving the agent view"
  fi
}
