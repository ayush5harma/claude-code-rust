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

def session.statusline session "statusLine setting" "Runs the command and shows its output under the prompt"
chk_session_statusline() {
  ensure_main || { res FAIL "main session did not start"; return; }
  # The fixture's project settings set statusLine to printf PARITY_STATUSLINE;
  # stock refreshes it after each message, so give it a turn to run after.
  grep -q 'PARITY_PONG' <<<"$(screen main)" ||
    turn main "Reply with exactly: PARITY_PONG" '^ *PARITY_PONG *$' 60
  if wait_for main 'PARITY_STATUSLINE' 10; then
    res PASS "$(first_match 'PARITY_STATUSLINE')"
  else
    res GAP "project statusLine (printf PARITY_STATUSLINE) not shown; footer: $(footer | head -1)"
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
