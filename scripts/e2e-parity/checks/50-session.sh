# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Session info in the footer: context left, the statusLine setting, cost.

def session.context session "Context left in the footer" "Context indicator near the prompt"
chk_session_context() {
  ensure_main || { res FAIL "main session did not start"; return; }
  # Before the first turn the footer's right side shows MCP status instead.
  grep -q 'PARITY_PONG' <<<"$(screen main)" ||
    turn main "Reply with exactly: PARITY_PONG" '^ *PARITY_PONG *$' 60
  wait_for main 'Loc: .*[0-9]+%$' 10
  local loc
  loc=$(footer | tail -1)
  if grep -Eq '[0-9]+%$' <<<"$loc"; then
    res PASS "$(sed -E 's/ {2,}/ … /' <<<"$loc")"
  else
    res FAIL "no percentage on the footer row: $loc"
  fi
}

def session.statusline session "statusLine setting (user level)" "Runs the command and shows its output under the prompt"
chk_session_statusline() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local cmd row i
  cmd=$(jq -r '.statusLine.command // empty' "$CONFIG_DIR/settings.json" 2>/dev/null)
  if [[ -z $cmd ]]; then
    res SKIP "the profile's user settings define no statusLine command (read-only check: the owner's settings are never edited)"
    return
  fi
  ENV_NOTE="the statusLine command is the profile's own"
  # The command runs after the session connects and again after each result.
  grep -q 'PARITY_PONG' <<<"$(screen main)" ||
    turn main "Reply with exactly: PARITY_PONG" '^ *PARITY_PONG *$' 60
  for ((i = 0; i < 20; i++)); do
    SCREEN=$(screen main)
    row=$(grep -v '^ *$' <<<"$SCREEN" | tail -1)
    # The footer's own rows are the mode row, Loc, Rate, Agents and the plan row.
    [[ -n $row && ! $row =~ ^(\[|Loc:|Rate:|Agents:|[▓░]|Resume) ]] && break
    row=""
    sleep 0.5
  done
  shot
  if [[ -n $row ]]; then
    res PASS "extra footer row from the profile's statusLine command: $row"
  else
    res FAIL "no statusLine row below the footer; last row: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
  fi
}

def session.statusline_project session "statusLine setting (project level)" "Runs the project's command"
chk_session_statusline_project() {
  ensure_main || { res FAIL "main session did not start"; return; }
  # The fixture's project settings set statusLine to printf PARITY_STATUSLINE.
  # claude-rs does not run a command that project or local settings select.
  if wait_for main 'PARITY_STATUSLINE' 5; then
    res FAIL "the project's statusLine command ran: $(first_match 'PARITY_STATUSLINE'); project and local settings must not select an executable command"
  else
    res GAP "not run, by design: project and local settings cannot select an executable command (docs/src/settings.md); stock runs the fixture's printf PARITY_STATUSLINE"
  fi
}

def session.cost session "Session cost" "Cost of the session on request"
chk_session_cost() {
  ensure_main || { res FAIL "main session did not start"; return; }
  keys main Escape
  enter_cmd main /usage
  if wait_for main '\$[0-9.]+ cost' 15; then
    res PASS "Usage tab: $(first_match '\$[0-9.]+ cost' | tr -d '│' | sed -E 's/ +$//')"
  else
    res FAIL "no session cost on the Usage tab"
  fi
  keys main Escape
  wait_for main "$EMPTY_COMPOSER" 5 || true
}
