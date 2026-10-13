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
  if [[ -z $row ]]; then
    res FAIL "no statusLine row below the footer; last row: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
    return
  fi
  # Specific, not just "an unrecognised last row": run the command once with
  # the documented input and require the row to start the way its output does
  # (digits masked), and to be unchanged across a second capture.
  local id once want got later
  id=$(jq -r '.sessionId // empty' <<<"$(registry_entry "$(cwd_of main)")")
  # With the profile selected as the sessions select it (CONFIG_ENV), and the
  # colour sequences the command prints removed, as claude-rs removes them.
  once=$(cd "$(cwd_of main)" && jq -nc --arg id "$id" --arg cwd "$(cwd_of main)" --arg m "$MODEL_LABEL" \
    '{session_id: $id, cwd: $cwd, workspace: {current_dir: $cwd, project_dir: $cwd}, model: {id: $m, display_name: $m}}' |
    timeout 5 env ${CONFIG_ENV[@]+"${CONFIG_ENV[@]}"} bash -c "$cmd" 2>/dev/null | head -1 | perl -pe 's/\e\[[0-9;]*m//g')
  want=$(sed -E 's/[0-9]+/N/g' <<<"${once:0:12}")
  got=$(sed -E 's/[0-9]+/N/g' <<<"$row")
  sleep 2
  later=$(grep -v '^ *$' <<<"$(screen main)" | tail -1)
  if [[ -z $once || ${#want} -lt 4 ]]; then
    res FAIL "the profile's statusLine command printed nothing when run once with the documented input, so the row '$row' cannot be tied to it"
  elif [[ $got == "$want"* && $later == "$row" ]]; then
    res PASS "the extra row '$row' starts like the command's own output ('${once:0:40}') and was unchanged two seconds later"
  else
    res FAIL "the extra row '$row' does not match the command's output ('${once:0:40}') or changed ('$later')"
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
