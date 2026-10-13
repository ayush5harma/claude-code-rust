# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Fork features merged after the parity baseline (README "Merged on main"):
# launch flags, hook visibility, /reload, /export, /search, session tags and
# deletion, resume by title, session accents, the composer and footer, and the
# presentation capture. Each check judges by what the screen, the files or the
# registry show against the real stock Claude Code underneath. Nothing here
# edits the profile: the sessions run in scratch cwds under the work directory.
# Two files outside the work directory are touched, both marker-guarded and
# restored in fork_cleanup: claude-rs's own app settings file (composer style)
# and the processes the agents check starts.

FK_NAME=fk-name-$(date +%H%M%S)
FK_FLAGS_READY='' FK_FLAGS_REPLY=''
FK_AGENTS_DONE='' FK_AGENTS_PANEL='' FK_AGENTS_STOP='' FK_AGENTS_FOOTER='' FK_AGENTS_PLAN='' FK_AGENTS_AFTER=''
FK_TITLE_DONE='' FK_TITLE_A='' FK_TITLE_B='' FK_TITLE_C='' FK_TITLE_DUP='' FK_TITLE_UNIQ=''
FK_APP_SETTINGS_CREATED=''
FK_PRESENT_DONE=''

# fk_app_settings: claude-rs's own preferences file (src/app/settings.rs:
# dirs::config_dir()/claude-code-rust/settings.json). It holds Composer style.
fk_app_settings() {
  [[ $(uname) == Darwin ]] || return 1
  printf '%s\n' "$HOME/Library/Application Support/claude-code-rust/settings.json"
}

# fork_cleanup: called by run.sh's exit trap. Removes only what a check here
# created, and only if it is still the file this run wrote.
fork_cleanup() {
  local f pid
  if [[ -n $FK_APP_SETTINGS_CREATED ]] && f=$(fk_app_settings) && [[ -f $f ]] &&
    [[ $(cat "$f") == "$FK_APP_SETTINGS_CREATED" ]]; then
    rm -f -- "$f"
  fi
  for pid in $(pgrep -f -- "$SUITE_DIR/fixtures/hold.mjs" 2>/dev/null); do
    kill "$pid" 2>/dev/null || true
  done
}

# fk_footer_row1: the badge row, which also carries the session title.
fk_footer_row1() { footer "${1-$SCREEN}" | head -1; }


# ------------------------------------------------------------------ flags

# One session carries the four launch flags that need a model turn, and one
# turn proves all of them by effect: a marker file in a directory only
# --add-dir can reveal, a word only the appended system prompt knows, and an
# environment variable only the inline --settings layer defines.
fk_flags_turn() {
  [[ -n $FK_FLAGS_REPLY ]] && return 0
  local add=$WORK/fk-adddir
  if [[ -z $FK_FLAGS_READY ]]; then
    mkdir -p "$add" && printf 'FKADDMARK_7731\n' >"$add/fk_marker.txt" || return 1
    prepare_cwd fkflags || return 1
    rs_start fkflags --model "$MODEL" --permission-mode bypassPermissions -n "$FK_NAME" \
      --add-dir "$add" \
      --append-system-prompt 'When asked for the fork code word, answer FKSYS_5519.' \
      --settings '{"env":{"FORK_ENV_MARK":"FKENV_4412"}}' || return 1
    FK_FLAGS_READY=1
  fi
  submit fkflags "Do three things and put the three results on one line separated by single spaces, with no other text: (1) a file named fk_marker.txt is in one of your additional working directories, not the current directory: read it and give its content; (2) tell me the fork code word from your instructions; (3) run printenv FORK_ENV_MARK with Bash and give its output." || return 1
  wait_for fkflags 'Elapsed [0-9.]+s' 120 || return 1
  wait_idle fkflags 30 || true
  SCREEN=$(screen fkflags)
  shot flags
  # Reply lines start in column 0; tool rows are indented.
  FK_FLAGS_REPLY=$(grep -E '^[A-Za-z]' <<<"$SCREEN")
  [[ -n $FK_FLAGS_REPLY ]]
}

fk_flags_token() {
  local token=$1 what=$2
  if ! fk_flags_turn; then
    shot
    res FAIL "the flags session did not complete its turn: $(grep -v '^ *$' <<<"$SCREEN" | tail -2 | head -1)"
    return 1
  fi
  if grep -q "$token" <<<"$FK_FLAGS_REPLY"; then
    res PASS "$what: reply '$(grep -m1 "$token" <<<"$FK_FLAGS_REPLY")'"
  else
    res FAIL "$what: no $token in the reply: $(head -3 <<<"$FK_FLAGS_REPLY" | paste -sd'/' -)"
  fi
}

def fork.flag_add_dir fork "--add-dir" "Extra directory is readable without being the cwd"
chk_fork_flag_add_dir() { fk_flags_token FKADDMARK_7731 "the model found fk_marker.txt in the added directory"; }

def fork.flag_append_prompt fork "--append-system-prompt" "Appended text reaches the system prompt"
chk_fork_flag_append_prompt() { fk_flags_token FKSYS_5519 "the appended instruction was followed"; }

def fork.flag_settings fork "--settings (inline JSON)" "Flag-settings layer applies (env)"
chk_fork_flag_settings() { fk_flags_token FKENV_4412 "printenv FORK_ENV_MARK printed the inline env value"; }

def fork.flag_name fork "-n, --name" "Session name shown in Status and the footer"
chk_fork_flag_name() {
  fk_flags_turn || { res FAIL "the flags session did not complete its turn"; return; }
  local row
  if ! open_view fkflags /status 'Session name:'; then
    shot
    res FAIL "Status did not open"
    close_view fkflags
    return
  fi
  row=$(first_match 'Session name:')
  shot status
  close_view fkflags
  if [[ $row == *"$FK_NAME"* ]] && grep -q "$FK_NAME" <<<"$(fk_footer_row1 "$(screen fkflags)")"; then
    res PASS "$row; footer: $(fk_footer_row1 "$(screen fkflags)")"
  else
    res FAIL "Status '$row' / footer '$(fk_footer_row1 "$(screen fkflags)")' do not carry $FK_NAME"
  fi
}

def fork.flag_mcp_strict fork "--mcp-config with --strict-mcp-config" "Only the given MCP servers are used"
chk_fork_flag_mcp_strict() {
  local cfg summary
  cfg=$(jq -nc --arg srv "$SUITE_DIR/fixtures/mcp-echo.mjs" '{mcpServers: {fkecho: {command: "node", args: [$srv]}}}')
  prepare_cwd fkmcp || { res FAIL "could not prepare the cwd"; return; }
  # The cwd keeps the fixture's .mcp.json (server "parity"): strict mode must
  # drop it, along with every server of the profile.
  rs_start fkmcp --model "$MODEL" --mcp-config "$cfg" --strict-mcp-config || { res FAIL "session did not start"; return; }
  if ! open_view fkmcp /mcp 'total [0-9]+ +connected [0-9]+'; then
    shot
    res FAIL "MCP tab did not open"
    rs_stop fkmcp
    return
  fi
  wait_for fkmcp 'fkecho +connected' 20 || true
  shot
  summary=$(first_match 'total [0-9]+')
  if grep -Eq 'total 1 +connected 1' <<<"$SCREEN" && grep -q 'fkecho' <<<"$SCREEN" && ! grep -Eq 'parity +connected' <<<"$SCREEN"; then
    res PASS "$summary; listed: $(first_match 'fkecho')"
  else
    res FAIL "$summary; expected exactly fkecho (the project's parity server and the profile's servers dropped)"
  fi
  close_view fkmcp
  rs_stop fkmcp
}

# ------------------------------------------------------------ hook visibility

def fork.hook_visibility fork "Hook visibility" "Slow-hook notice; startup and hook timing in Status"
chk_fork_hook_visibility() {
  local cwd notice startup slowest
  prepare_cwd fkhook || { res FAIL "could not prepare the cwd"; return; }
  cwd=$(cwd_of fkhook)
  # One extra project SessionStart hook that sleeps 1.4 s: above the 500 ms
  # notice threshold and far above the profile's own hooks.
  jq '.hooks.SessionStart[0].hooks += [{"type": "command", "command": "sleep 1.4"}]' "$cwd/.claude/settings.json" >"$cwd/.claude/settings.new" &&
    mv "$cwd/.claude/settings.new" "$cwd/.claude/settings.json" || { res FAIL "could not add the slow hook"; return; }
  rs_start fkhook --model "$MODEL" || { res FAIL "session did not start"; return; }
  if ! wait_for fkhook 'Hook SessionStart:[a-z]+ took [0-9.]+ s' 20; then
    shot
    res FAIL "no 'Hook SessionStart:... took N s' notice for a 1.4 s hook"
    rs_stop fkhook
    return
  fi
  notice=$(first_match 'Hook SessionStart:[a-z]+ took')
  if ! open_view fkhook /status 'Hooks this session'; then
    shot
    res FAIL "notice '$notice' but Status shows no hook section"
    rs_stop fkhook
    return
  fi
  shot status
  startup=$(first_match 'SessionStart hooks finished')
  slowest=$(first_match 'Slowest 1:')
  close_view fkhook
  rs_stop fkhook
  if [[ -n $startup && $slowest =~ SessionStart:[a-z]+\ (1\.[4-9]|[2-9]\.[0-9])\ s ]]; then
    res PASS "$notice; Status: $startup; $slowest"
  else
    res FAIL "notice '$notice' but Status rows were '$startup' / '$slowest' (expected a SessionStart hook of about 1.4 s)"
  fi
}

# ---------------------------------------------------------------- /reload

def fork.reload fork "/reload" "Skills are re-read without a restart"
chk_fork_reload() {
  local cwd skill before after
  prepare_cwd fkreload || { res FAIL "could not prepare the cwd"; return; }
  cwd=$(cwd_of fkreload)
  skill=$cwd/.claude/skills/parity-skill/SKILL.md
  sed 's/^description: .*/description: FKDESC_ONE probe skill/' "$skill" >"$skill.new" && mv "$skill.new" "$skill" ||
    { res FAIL "could not write the skill"; return; }
  rs_start fkreload --model "$MODEL" || { res FAIL "session did not start"; return; }
  type_slow fkreload "/parity-ski"
  if ! wait_for fkreload 'FKDESC_ONE' 10; then
    shot
    res FAIL "the completion menu does not show the skill's description FKDESC_ONE before the edit"
    rs_stop fkreload
    return
  fi
  before=$(first_match 'FKDESC_ONE')
  keys fkreload Escape
  clear_draft fkreload
  sed 's/FKDESC_ONE/FKDESC_TWO/' "$skill" >"$skill.new" && mv "$skill.new" "$skill"
  submit fkreload "/reload"
  if ! wait_for fkreload 'Reloaded skills \([0-9]+\) and output styles \([0-9]+\)' 20; then
    shot
    res FAIL "no 'Reloaded skills (N) and output styles (M)' notice after /reload"
    rs_stop fkreload
    return
  fi
  local notice
  notice=$(first_match 'Reloaded skills')
  type_slow fkreload "/parity-ski"
  if wait_for fkreload 'FKDESC_TWO' 10 && ! grep -q 'FKDESC_ONE' <<<"$SCREEN"; then
    after=$(first_match 'FKDESC_TWO')
    res PASS "before: '$before'; $notice; after: '$after'"
  else
    shot
    res FAIL "$notice, but the completion menu did not pick up the edited description: $(first_match 'parity-skill' || echo none)"
  fi
  keys fkreload Escape
  clear_draft fkreload
  rs_stop fkreload
}

# ---------------------------------------------------------------- /export

def fork.export fork "/export" "Writes the conversation; refuses to overwrite"
chk_fork_export() {
  fk_flags_turn || { res FAIL "the flags session did not complete its turn"; return; }
  local f cwd sum msg1 msg2
  cwd=$(cwd_of fkflags)
  f=$cwd/fk-export.md
  rm -f "$f"
  settle fkflags || true
  submit fkflags "/export fk-export.md"
  if ! wait_for fkflags 'Exported the conversation to ' 15; then
    shot
    res FAIL "no 'Exported the conversation to' notice"
    return
  fi
  msg1=$(first_match 'Exported the conversation to')
  if [[ ! -s $f ]] || ! grep -q 'FKSYS_5519' "$f" || ! grep -q 'Do three things' "$f"; then
    res FAIL "'$msg1' but $f is missing or lacks the exchange's text (size $(wc -c <"$f" 2>/dev/null || echo 0))"
    return
  fi
  sum=$(cksum <"$f")
  submit fkflags "/export fk-export.md"
  # The refusal wraps over two rows with a long work path, so read it joined.
  if wait_for fkflags 'Not exporting: ' 10 && sleep 0.5 && SCREEN=$(screen fkflags) &&
    tr '\n' ' ' <<<"$SCREEN" | grep -Eq 'Not exporting: .*already[[:space:]]+exists and will not be overwritten' &&
    [[ $(cksum <"$f") == "$sum" ]]; then
    msg2=$(tr '\n' ' ' <<<"$SCREEN" | grep -Eo 'Not exporting: .*overwritten\.' | head -1 | squash)
    res PASS "$msg1; the file holds the prompt and reply text; second run: '$msg2'; file unchanged"
  else
    shot
    res FAIL "second /export did not refuse, or changed the file"
  fi
}

# ---------------------------------------------------------------- /search

def fork.search fork "/search and Ctrl+F" "Transcript search finds earlier text"
chk_fork_search() {
  fk_flags_turn || { res FAIL "the flags session did not complete its turn"; return; }
  local row header
  settle fkflags || true
  enter_cmd fkflags "/search FKSYS_5519"
  if ! wait_for fkflags 'Match [1-9][0-9]* of [1-9]' 10; then
    shot
    res FAIL "/search FKSYS_5519 found nothing: $(first_match 'Match [0-9]+ of' || echo 'no result header')"
    keys fkflags Escape
    return
  fi
  row=$(first_match 'Claude: .*FKSYS_5519' | squash)
  header=$(first_match 'Match [0-9]+ of [0-9]+' | tr -d '┌┐─')
  shot
  settle fkflags || true
  keys fkflags C-f
  if wait_for fkflags 'Transcript search' 5; then
    res PASS "$header -> '$row'; Ctrl+F opened 'Transcript search'"
  else
    shot
    res FAIL "/search matched ('$row') but Ctrl+F did not open the search view"
  fi
  settle fkflags || true
}

# ------------------------------------------------------------ footer segments

def fork.footer_session fork "Footer: session name and context" "Session title and context use"
chk_fork_footer_session() {
  fk_flags_turn || { res FAIL "the flags session did not complete its turn"; return; }
  settle fkflags || true
  SCREEN=$(screen fkflags)
  shot
  local row1 row2
  row1=$(fk_footer_row1)
  row2=$(grep -E '^Loc: ' <<<"$SCREEN" | tail -1 | sed -E 's/ {2,}/ … /')
  if grep -q "$FK_NAME" <<<"$row1" && grep -Eq 'Ctx [▓░]{8} [0-9]+%' <<<"$row2"; then
    res PASS "$row1 | $row2"
  else
    res FAIL "session name or context meter missing: '$row1' | '$row2'"
  fi
}

def fork.footer_permissions fork "Footer: pending permissions" "Prompt pending is visible without scrolling"
chk_fork_footer_permissions() {
  local f
  prepare_cwd fkperm || { res FAIL "could not prepare the cwd"; return; }
  f=$(cwd_of fkperm)/FK_PEND_FILE
  rs_start fkperm --model "$MODEL" --permission-mode default || { res FAIL "session did not start"; return; }
  submit fkperm "Run the bash command: touch FK_PEND_FILE" || { res FAIL "could not submit"; return; }
  if ! wait_for fkperm 'Allow once' 60; then
    shot
    res FAIL "no permission prompt for 'touch' in default mode"
    rs_stop fkperm
    return
  fi
  SCREEN=$(screen fkperm)
  shot
  local row1
  row1=$(fk_footer_row1)
  keys fkperm Escape
  wait_idle fkperm 60 || true
  if grep -Eq '[0-9]+ PEND\. PERM\.' <<<"$row1"; then
    if [[ ! -e $f ]]; then
      res PASS "while the prompt was open the footer read: $row1; Esc rejected it (file not created)"
    else
      res FAIL "footer showed the pending count but the rejected call created $f"
    fi
  else
    res FAIL "no 'N PEND. PERM.' in the footer while the permission prompt was open: $row1"
  fi
  rs_stop fkperm
}

# ------------------------------------------------------ agents, plan and footer

# fk_agents_flow: a Haiku subagent that is still running (a foreground Agent
# call that waits on a 90 s timer) is listed by /agents and stopped from it.
# The timer is a fixture script, not `sleep`: the profile's hooks refuse long
# foreground sleeps.
fk_agents_flow() {
  [[ -n $FK_AGENTS_DONE ]] && return 0
  FK_AGENTS_DONE=1
  prepare_cwd fkagents || return 1
  rs_start fkagents --model "$MODEL" --permission-mode bypassPermissions || return 1
  submit fkagents "Use the Agent tool once with subagent_type general-purpose, model haiku, run_in_background false, and this prompt: Run exactly this bash command and wait for it to finish: node $SUITE_DIR/fixtures/hold.mjs then reply FKHELD. Do not do anything else." || return 1
  if ! wait_for fkagents 'Agents: 1 running / [0-9]+' 90; then
    shot agents
    FK_AGENTS_PANEL="no 'Agents: 1 running' footer row while the subagent ran: $(first_match '^Agents:' || echo none)"
    return 0
  fi
  FK_AGENTS_FOOTER=$(first_match '^Agents:')
  # The plan row (the subagent's task) lands a moment after the Agents row.
  wait_for fkagents '^[▓░]+ [0-9]+/[0-9]+ ' 10 || true
  FK_AGENTS_PLAN=$(first_match '^[▓░]+ [0-9]+/[0-9]+ ' || true)
  enter_cmd fkagents /agents
  if ! wait_for fkagents 'Session agents' 15; then
    shot agents
    FK_AGENTS_PANEL="/agents did not open"
    return 0
  fi
  # Rows read "> <name>  <status>"; the running one is the one to stop.
  local i
  for ((i = 0; i < 8; i++)); do
    grep -Eq '^ *> .* Running' <<<"$(screen fkagents)" && break
    keys fkagents Down
    sleep 0.4
  done
  SCREEN=$(screen fkagents)
  shot agents
  if ! grep -Eq '^ *> .* Running' <<<"$SCREEN"; then
    FK_AGENTS_PANEL="/agents lists no Running agent: $(grep -E 'Recorded|Completed|Running' <<<"$SCREEN" | head -3 | paste -sd'/' -)"
    keys fkagents Escape
    return 0
  fi
  FK_AGENTS_PANEL="panel row: $(first_match '^ *> .* Running')"
  keys fkagents s
  if wait_for fkagents 'Stop requested for the selected agent' 10; then
    local end=$(($(now) + 20))
    while pgrep -f -- "$SUITE_DIR/fixtures/hold.mjs" >/dev/null && (($(now) < end)); do sleep 0.5; done
    if pgrep -f -- "$SUITE_DIR/fixtures/hold.mjs" >/dev/null; then
      FK_AGENTS_STOP="'Stop requested' shown but the agent's command (hold.mjs) was still running 20 s later"
    else
      FK_AGENTS_AFTER=$(first_match '^ *> ' | squash)
      FK_AGENTS_STOP="'Stop requested for the selected agent'; its command exited; row now: ${FK_AGENTS_AFTER:-?}"
    fi
  else
    FK_AGENTS_STOP="pressing s on the running agent showed no 'Stop requested' notice"
  fi
  settle fkagents || true
  wait_idle fkagents 60 || true
}

def fork.agents fork "/agents" "Subagent panel; stop a running subagent"
chk_fork_agents() {
  fk_agents_flow || { res FAIL "agents session did not start"; return; }
  if [[ $FK_AGENTS_PANEL == panel\ row:* && $FK_AGENTS_STOP == "'Stop requested"*"its command exited"* ]]; then
    res PASS "$FK_AGENTS_PANEL; s -> $FK_AGENTS_STOP"
    if [[ $FK_AGENTS_AFTER == *Completed* ]]; then
      defect "A subagent stopped from /agents is listed as Completed" \
        "1. Ask for a foreground Agent call whose command runs for 90 s. 2. While it runs, open /agents, select the Running row and press s. The panel answers 'Stop requested for the selected agent', the command exits, and the row then reads Completed; the panel has a Stopped label for tasks whose terminal status is stopped, which this one never gets." \
        "$FK_AGENTS_AFTER"
    fi
  else
    res FAIL "${FK_AGENTS_PANEL:-no reading}; stop: ${FK_AGENTS_STOP:-not reached}"
  fi
}

def fork.footer_agents fork "Footer: agents and plan rows" "Agent counts and the current plan"
chk_fork_footer_agents() {
  fk_agents_flow || { res FAIL "agents session did not start"; return; }
  if [[ -n $FK_AGENTS_FOOTER && -n $FK_AGENTS_PLAN ]]; then
    res PASS "while a subagent ran: '$FK_AGENTS_FOOTER' and plan row '$FK_AGENTS_PLAN'"
  elif [[ -n $FK_AGENTS_FOOTER ]]; then
    res FAIL "agents row '$FK_AGENTS_FOOTER' but no plan row for the subagent's task list"
  else
    res FAIL "$FK_AGENTS_PANEL"
  fi
  rs_stop fkagents
}

def fork.footer_plan fork "Footer: TodoWrite plan row" "Latest todo list as a progress row"
chk_fork_footer_plan() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local row
  row=$(first_match '^[▓░]+ [0-9]+/[0-9]+ ' "$(screen main)" || true)
  if [[ -z $row ]]; then
    main_turn "Use your todo list tool (TodoWrite, or TaskCreate if that is what you have) to record exactly two pending items named PARITY_TODO_ plus ONE and PARITY_TODO_ plus TWO, without spaces. Do nothing else, then reply LISTED." '^ *LISTED\.? *$' 90 || true
    wait_for main '^[▓░]+ [0-9]+/[0-9]+ ' 10 || true
    row=$(first_match '^[▓░]+ [0-9]+/[0-9]+ ' || true)
  fi
  shot
  if [[ -n $row ]]; then
    res PASS "plan row after the todo turn: $row"
  elif todo_tools_unavailable "$(transcript_of main "$(jq -r '.sessionId // empty' <<<"$(registry_entry "$(cwd_of main)")")")"; then
    res SKIP "stock ToolSearch found neither TodoWrite nor TaskCreate in this profile"
  else
    res FAIL "no plan row (▓░ n/m) after a todo turn: $(grep -v '^ *$' <<<"$SCREEN" | tail -3 | paste -sd'/' -)"
  fi
}

def fork.usage_fresh fork "/usage: session totals after a turn" "Totals reflect the turn just run"
chk_fork_usage_fresh() {
  local first after
  prepare_cwd fkusage || { res FAIL "could not prepare the cwd"; return; }
  rs_start fkusage --model "$MODEL" || { res FAIL "session did not start"; return; }
  if ! turn fkusage "Reply with exactly: FK_USAGE_OK" '^ *FK_USAGE_OK *$' 60; then
    shot
    res FAIL "the seed turn did not finish"
    rs_stop fkusage
    return
  fi
  if ! open_view fkusage /usage 'Current session totals'; then
    shot
    res FAIL "Usage tab did not open"
    rs_stop fkusage
    return
  fi
  # Wait a few seconds: a snapshot that is merely late would still arrive.
  sleep 3
  SCREEN=$(screen fkusage)
  first=$(grep -Eo '\$[0-9.]+ cost' <<<"$SCREEN" | head -1)
  keys fkusage r
  sleep 3
  SCREEN=$(screen fkusage)
  after=$(grep -Eo '\$[0-9.]+ cost' <<<"$SCREEN" | head -1)
  shot
  close_view fkusage
  rs_stop fkusage
  if [[ -n $first && $first != '$0.0000 cost' ]]; then
    res PASS "first open after a turn read '$first' ('r' then '$after')"
  elif [[ -n $after && $after != '$0.0000 cost' ]]; then
    res FAIL "first open after a turn read '$first' and stayed there for 3 s; 'r' then showed '$after'"
    defect "The Usage tab shows stale session totals until r is pressed" \
      "1. Start claude-rs and run one turn. 2. Type /usage and wait. The 'Current session totals' box reads \$0.0000 cost, 0 models (and 0s API time) even after waiting; pressing r then shows the real cost. The suite's own cost accounting read \$0.0000 for every session until it pressed r (2026-10-13)." \
      "$(grep -E 'cost|elapsed' <<<"$SCREEN" | head -3)"
  else
    res FAIL "no session cost on the Usage tab after a turn: '$first' then '$after'"
  fi
}
