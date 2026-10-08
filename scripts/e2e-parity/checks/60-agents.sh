# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# The session registry and the agent view. Stock (2.1.293): the footer's last
# row ends with a dim "← N agents" while the input is empty, N counting the
# background sessions of `claude agents --json`; Left on an empty prompt opens
# the agent view, whose standalone form (`claude agents`) exits on Esc.
#
# Safety: the agent view lists the owner's real background sessions. These
# checks only read it and leave with Esc; they never press Enter, Space or
# Ctrl+X there.

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

# background_counts: "N K W" = background sessions, how many need input
# (state blocked), how many are working.
background_counts() {
  agents_json | jq -r '[.[] | select(.kind == "background")] |
    "\(length) \([.[] | select(.state == "blocked")] | length) \([.[] | select(.state == "working" or .state == "running" or .state == "busy")] | length)"'
}

def agents.hint agents "\`← N agents\` in the footer" "Dim hint while the input is empty, from the background sessions"
chk_agents_hint() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local n k w want noun
  read -r n k w <<<"$(background_counts)"
  if [[ -z $n ]]; then
    res FAIL "\`claude agents --json\` did not answer"
    return
  fi
  if ((n == 0)); then
    res SKIP "no background sessions right now, so stock shows no hint either"
    return
  fi
  noun=agents
  ((n == 1)) && noun=agent
  want="← $n $noun"
  wait_for main "$EMPTY_COMPOSER" 5 || true
  if wait_for main "$want" 15; then
    local row
    row=$(first_match "$want")
    if ((k > 0)) && ! grep -q "$k need input" <<<"$row"; then
      res FAIL "'$row' lacks '· $k need input' ($k blocked)"
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
  local n
  read -r n _ _ <<<"$(background_counts)"
  if ((${n:-0} == 0)); then
    res SKIP "no background sessions, no hint to hide"
    return
  fi
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

AGENT_VIEW_RE='[0-9]+ awaiting input · [0-9]+ working|describe a task for a new session'

def agents.left_opens agents "← on an empty prompt opens the agent view" "Agent view (list, peek, attach, dispatch)"
chk_agents_left_opens() {
  ensure_main || { res FAIL "main session did not start"; return; }
  wait_for main "$EMPTY_COMPOSER" 5 || true
  keys main Left
  if wait_for main "$AGENT_VIEW_RE" 15; then
    AGENT_VIEW_OPENED=1
    res PASS "agent view: $(first_match '[0-9]+ awaiting input' || first_match 'describe a task')"
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
    res PASS "back in claude-rs; session still registered: $(jq -c '{status}' <<<"$REG")"
  else
    shot
    res FAIL "claude-rs did not come back after leaving the agent view"
  fi
}
