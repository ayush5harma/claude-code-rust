# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Launch, prompt round trip, resume paths and launch flags.

# The shared session most checks run in. bypassPermissions keeps tool checks
# free of prompts; the permission checks use their own default-mode session.
MAIN_ARGS=(--model haiku --permission-mode bypassPermissions)
ensure_main() { ensure main "${MAIN_ARGS[@]}"; }

# main_turn <prompt> <ERE> [timeout]: a model turn in the shared session.
main_turn() { ensure_main && turn main "$@"; }

def launch.new launch "New session starts" \
  "Starts at the prompt; the session is registered"
chk_launch_new() {
  if ! ensure_main; then
    res FAIL "no composer within 60s: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
    return
  fi
  REG=$(registry_entry "$(cwd_of main)")
  res PASS "composer ready; registry: $(jq -c '{kind, sessionId, status}' <<<"$REG")"
}

def launch.roundtrip launch "Prompt round trip" "Answer streamed into the transcript"
chk_launch_roundtrip() {
  if main_turn "Reply with exactly: PARITY_PONG" '^ *PARITY_PONG *$' 60; then
    res PASS "reply line: $(first_match '^ *PARITY_PONG *$')"
  else
    shot
    res FAIL "no PARITY_PONG reply within 60s; composer: $(composer_line)"
  fi
}

def launch.busy_idle launch "Registry busy during a turn, idle after" \
  "\`claude agents --json\` status busy, then idle"
chk_launch_busy_idle() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local cwd seen_busy=""
  cwd=$(cwd_of main)
  submit main "Run the bash command: sleep 4; echo PARITY_SLEPT" || { res FAIL "prompt not submitted"; return; }
  if wait_registry "$cwd" '.status == "busy"' 20; then seen_busy=1; fi
  # The Bash row may show the command's description instead of its output,
  # so the end of the turn is read from the registry, not the screen.
  if ! wait_idle main 60; then
    shot
    res FAIL "turn did not finish; busy seen: ${seen_busy:-no}"
    return
  fi
  if [[ -n $seen_busy ]]; then
    res PASS "busy while the turn ran, then $(jq -c '{status, name}' <<<"$REG")"
  else
    res FAIL "registry never reported busy; final $(jq -c '{status}' <<<"$REG")"
  fi
}

# The resume checks share one seeded session in the "resume" cwd, started
# with an initial prompt on the command line.
RESUME_SEED_ID=
seed_resume() {
  [[ -n $RESUME_SEED_ID ]] && return 0
  rs_start resume --model haiku "Reply with exactly: PARITY_RESUME_SEED" || return 1
  wait_for resume '^ *PARITY_RESUME_SEED *$' 60 || return 1
  wait_idle resume 30 || true
  RESUME_SEED_ID=$(jq -r .sessionId <<<"$(registry_entry "$(cwd_of resume)")")
  rs_stop resume
  [[ -n $RESUME_SEED_ID && $RESUME_SEED_ID != null ]]
}

def launch.initial_prompt launch "Initial prompt argument" \
  "\`claude \"prompt\"\` sends it once ready"
chk_launch_initial_prompt() {
  if seed_resume; then
    res PASS "argv prompt answered: PARITY_RESUME_SEED (session $RESUME_SEED_ID)"
  else
    shot
    res FAIL "no reply to the argv prompt; last line: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
  fi
}

# resumed_shows_seed <sess>: the resumed transcript replays the seed turn.
resumed_shows_seed() {
  wait_for "$1" '^ *PARITY_RESUME_SEED *$' 20
}

def launch.continue launch "Continue latest session (-c)" "Reopens the latest conversation in the cwd"
chk_launch_continue() {
  seed_resume || { res FAIL "could not seed a session to continue"; return; }
  if ! rs_start resume -c; then
    res FAIL "-c did not reach the composer"
    return
  fi
  if resumed_shows_seed resume; then
    res PASS "history replayed; registry id $(jq -r .sessionId <<<"$REG"), seed $RESUME_SEED_ID"
  else
    shot
    res FAIL "-c opened without the seed turn; composer: $(composer_line)"
  fi
  rs_stop resume
}

def launch.resume_id launch "Resume by id (-r <id>)" "Reopens that conversation"
chk_launch_resume_id() {
  seed_resume || { res FAIL "could not seed a session to resume"; return; }
  if ! rs_start resume -r "$RESUME_SEED_ID"; then
    res FAIL "-r $RESUME_SEED_ID did not reach the composer"
    return
  fi
  if resumed_shows_seed resume; then
    res PASS "-r $RESUME_SEED_ID replayed the seed turn"
  else
    shot
    res FAIL "-r opened without the seed turn; composer: $(composer_line)"
  fi
  rs_stop resume
}

def launch.resume_picker launch "Resume picker (-r without id)" "Lists recent sessions to pick from"
chk_launch_resume_picker() {
  seed_resume || { res FAIL "could not seed a session to list"; return; }
  local cwd
  cwd=$(cwd_of resume)
  session_exists resume && tm kill-session -t resume
  rs_cmd resume
  tm new-session -d -s resume -x "$COLS" -y "$ROWS" -c "$cwd" "${RS_CMD[@]}" -r
  if wait_for resume 'PARITY_RESUME_SEED|Reply with exactly' 30; then
    res PASS "picker lists the seed: $(first_match 'PARITY_RESUME_SEED|Reply with exactly')"
  else
    shot
    res FAIL "no picker row for the seed session: $(grep -v '^ *$' <<<"$SCREEN" | head -3 | tr '\n' ' ')"
  fi
  # Leave without picking: the list may also hold the owner's sessions.
  keys resume Escape
  sleep 0.5
  rs_stop resume
}

def launch.model_flag launch "--model" "Starts on the given model"
chk_launch_model_flag() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if wait_for main '^\[[A-Za-z ]+\] +\[Haiku' 10; then
    res PASS "--model haiku: $(footer | head -1)"
  else
    res FAIL "footer does not show Haiku: $(footer | head -1)"
  fi
}

def launch.permission_mode_flag launch "--permission-mode" "Starts in the given mode"
chk_launch_permission_mode_flag() {
  ensure plan --model haiku --permission-mode plan || { res FAIL "plan session did not start"; return; }
  if wait_for plan '^\[Plan\]' 10; then
    res PASS "--permission-mode plan: $(footer | head -1)"
  else
    res FAIL "footer does not show Plan: $(footer | head -1)"
  fi
}

ensure_agentflag() { ensure agentflag --model haiku --effort low --agent parity-agent; }

def launch.effort_flag launch "--effort" "Starts at the given effort"
chk_launch_effort_flag() {
  ensure_agentflag || { res FAIL "session did not start"; return; }
  if wait_for agentflag '/Low\]' 10; then
    res PASS "--effort low: $(footer | head -1)"
  else
    res FAIL "footer does not show Low effort: $(footer | head -1)"
  fi
}

def launch.agent_flag launch "--agent" "Main thread runs as the named agent"
chk_launch_agent_flag() {
  ensure_agentflag || { res FAIL "session did not start"; return; }
  if turn agentflag "hello" '^ *PARITY_AGENT_OK *$' 60; then
    res PASS "parity-agent's fixed reply: PARITY_AGENT_OK"
  else
    shot
    res FAIL "reply was not the agent's: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[' | tail -4 | head -1)"
  fi
  rs_stop agentflag
}
