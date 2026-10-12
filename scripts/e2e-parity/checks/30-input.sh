# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Composer input: multi-line entry, mentions, bash mode, history, cancel,
# mode cycling and the control keys stock Claude Code binds.

# clear_draft <sess>: Ctrl+C on a non-empty draft clears it.
clear_draft() {
  if ! grep -q "$EMPTY_COMPOSER" <<<"$(screen "$1")"; then
    keys "$1" C-c
    wait_for "$1" "$EMPTY_COMPOSER" 3 || true
  fi
}

# composer_rows <sess>: the composer's rows, from the ❯ line to the footer.
composer_rows() {
  screen "$1" | awk '/^ ?❯ /{on=1} on && /^\[/{exit} on' | sed -E 's/[[:space:]]+$//' | grep -v '^$'
}

# stable_screen <sess>: a capture that did not change for a second, without
# the prompt-suggestion row (it appears on its own after a turn).
stable_screen() {
  local prev cur i
  prev=$(screen "$1" | grep -v '^Suggestion:')
  for ((i = 0; i < 10; i++)); do
    sleep 1
    cur=$(screen "$1" | grep -v '^Suggestion:')
    [[ $cur == "$prev" ]] && break
    prev=$cur
  done
  printf '%s\n' "$cur"
}

# same_screen_after <sess> <keys...>: press keys and report whether the
# screen is unchanged a moment later (DIFF_LINE holds the first change).
same_screen_after() {
  local s=$1 before after
  shift
  before=$(stable_screen "$s")
  keys "$s" "$@"
  sleep 1.2
  after=$(screen "$s" | grep -v '^Suggestion:')
  DIFF_LINE=$(diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") | grep -m1 '^>' | sed -E 's/^> *//')
  [[ $before == "$after" ]]
}

def input.shift_enter input "Multi-line input: Shift+Enter" "Inserts a newline"
chk_input_shift_enter() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  type_text main "pm_line_one"
  shift_enter main
  sleep 0.3
  type_text main "pm_line_two"
  local rows
  rows=$(composer_rows main)
  if grep -q 'pm_line_one' <<<"$(head -1 <<<"$rows")" && grep -q 'pm_line_two' <<<"$(sed -n 2p <<<"$rows")"; then
    res PASS "two composer rows: $(paste -sd'/' - <<<"$rows")"
  else
    res FAIL "composer rows: $(paste -sd'/' - <<<"$rows")"
  fi
  clear_draft main
}

def input.backslash_enter input "Multi-line input: backslash then Enter" \
  "\`\\\\\` at the end of a line plus Enter inserts a newline"
chk_input_backslash_enter() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  type_text main 'pm_bs_one\'
  keys main Enter
  sleep 1.5
  local rows
  rows=$(composer_rows main)
  if grep -q 'pm_bs_one' <<<"$rows"; then
    type_text main "pm_bs_two"
    rows=$(composer_rows main)
    res PASS "newline inserted: $(paste -sd'/' - <<<"$rows")"
    clear_draft main
  else
    SCREEN=$(screen main)
    res GAP "Enter submitted 'pm_bs_one\\' as a prompt instead of continuing the line"
    wait_idle main 60 || true
  fi
}

def input.at_mention input "@file mention autocomplete" "Suggests matching files"
chk_input_at_mention() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  type_slow main "see @parity_men"
  if wait_for main 'parity_mention\.txt' 5; then
    res PASS "suggestion: $(first_match 'parity_mention\.txt')"
  else
    res FAIL "no parity_mention.txt suggestion: $(composer_line)"
  fi
  keys main Escape
  clear_draft main
}

def input.bang input "! bash mode" "Runs the command locally, no model turn"
chk_input_bang() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  submit main "!echo PARITY_BANG_7" || { res FAIL "could not submit"; return; }
  wait_for main '^ +PARITY_BANG_7|PARITY_BANG_7\.' 60 || true
  wait_idle main 60 || true
  local transcript sid
  sid=$(jq -r .sessionId <<<"$(registry_entry "$(cwd_of main)")")
  transcript=$(transcript_of main "$sid")
  if [[ -f $transcript ]] && grep -q 'bash-stdout>PARITY_BANG_7' "$transcript"; then
    res PASS "ran locally: transcript has <bash-stdout>PARITY_BANG_7"
  elif [[ -f $transcript ]] && grep -q '"name":"Bash".*echo PARITY_BANG_7' "$transcript"; then
    res GAP "sent to the model as a prompt; the model then ran Bash itself (transcript tool_use Bash 'echo PARITY_BANG_7')"
  else
    SCREEN=$(screen main)
    shot
    res FAIL "no local run and no model Bash call found ($transcript)"
  fi
}

# transcript_of <sess> <session id>: the session's JSONL transcript.
transcript_of() {
  local mangled
  mangled=$(sed -E 's/[^A-Za-z0-9]/-/g' <<<"$(cwd_of "$1")")
  printf '%s/projects/%s/%s.jsonl\n' "$CONFIG_DIR" "$mangled" "$2"
}

def input.up_history input "Up recalls the previous prompt" "Previous prompt into the empty composer"
chk_input_up_history() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  submit main "Reply with exactly: PARITY_HISTORY" || { res FAIL "could not submit"; return; }
  wait_idle main 60 || true
  keys main Up
  if wait_for main '❯ Reply with exactly: PARITY_HISTORY' 3; then
    res PASS "$(composer_line)"
  else
    res FAIL "composer after Up: $(composer_line)"
  fi
  clear_draft main
}

def input.esc_cancel input "Esc cancels a running turn" "Interrupts the turn"
chk_input_esc_cancel() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  submit main "Count from 1 to 1500, one number per line, no other text." || { res FAIL "could not submit"; return; }
  if ! wait_for main '^ *1[0-9]$' 30; then
    res FAIL "the counting turn never streamed"
    return
  fi
  keys main Escape
  local t0=$SECONDS
  if wait_idle main 15; then
    local last
    last=$(grep -E '^ *[0-9]+ *$' <<<"$(screen main)" | tail -1 | tr -d ' ')
    if ((${last:-1500} < 1500)); then
      res PASS "idle $((SECONDS - t0))s after Esc, stopped at $last; $(first_match '[Cc]ancel|[Ii]nterrupt' || echo 'no notice')"
    else
      res FAIL "turn ran to the end despite Esc"
    fi
  else
    shot
    res FAIL "still busy 15s after Esc"
  fi
}

def input.esc_esc input "Esc Esc on an empty prompt" "Opens the rewind / message picker"
chk_input_esc_esc() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  wait_for main "$EMPTY_COMPOSER" 5 || true
  if same_screen_after main Escape Escape; then
    res GAP "nothing happens (stock opens the rewind picker); /rewind with arguments is the claude-rs route"
  elif grep -Eqi 'rewind|restore|previous message' <<<"$(screen main)"; then
    res PASS "picker: $DIFF_LINE"
    keys main Escape
  else
    res FAIL "screen changed but no picker: $DIFF_LINE"
  fi
}

def input.shift_tab input "Shift+Tab cycles the mode" "Cycles permission modes"
chk_input_shift_tab() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  local start cur seen=() i
  start=$(grep -Eo '^\[[A-Za-z ]+\]' <<<"$(footer "$(screen main)")" | head -1)
  for ((i = 0; i < 7; i++)); do
    keys main BTab
    sleep 1.2
    cur=$(grep -Eo '^\[[A-Za-z ]+\]' <<<"$(footer "$(screen main)")" | head -1)
    seen+=("$cur")
    [[ $cur == "$start" ]] && break
  done
  if ((${#seen[@]} > 1)) && [[ $cur == "$start" ]]; then
    res PASS "cycle from $start: ${seen[*]}"
  elif ((${#seen[@]} >= 1)) && [[ ${seen[0]} != "$start" ]]; then
    res FAIL "mode changed but did not cycle back to $start: ${seen[*]}"
  else
    res FAIL "mode did not change from $start"
  fi
}

def input.ctrl_l input "Ctrl+L redraws" "Redraws the screen"
chk_input_ctrl_l() {
  ensure_main || { res FAIL "main session did not start"; return; }
  keys main C-l
  if wait_for main "$EMPTY_COMPOSER" 5 && wait_for main 'Loc: ' 2; then
    res PASS "screen intact after redraw: $(composer_line)"
  else
    res FAIL "composer or footer missing after Ctrl+L"
  fi
}

def input.ctrl_g input "Ctrl+G edits the draft in \$EDITOR" "Opens the draft in the external editor"
chk_input_ctrl_g() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  type_text main "pm_editor_seed"
  keys main C-g
  if wait_for main 'PARITY_EDITOR_TEXT' 8; then
    res PASS "draft replaced by the editor's text: $(composer_line)"
  else
    res GAP "Ctrl+G does nothing (composer: $(composer_line)); EDITOR was a script that writes PARITY_EDITOR_TEXT"
  fi
  clear_draft main
}

def input.ctrl_v_image input "Ctrl+V pastes an image" "Attaches the clipboard image"
chk_input_ctrl_v_image() {
  res SKIP "needs an image on the system clipboard; the suite never overwrites the owner's clipboard with an image (covered by the fork's own Ctrl+V tests)"
}

def input.tab_complete input "Tab completes a slash command" "Completes the highlighted command"
chk_input_tab_complete() {
  ensure_main || { res FAIL "main session did not start"; return; }
  clear_draft main
  # /stat also matches /stats; use an unambiguous completion prefix.
  type_slow main "/statu"
  local top
  wait_for main '> /[a-z]' 3 || true
  top=$(first_match '> /[a-z:-]+' | grep -Eo '/[a-z:-]+' | head -1)
  keys main Tab
  if wait_for main '❯ /status' 3; then
    res PASS "$(composer_line)"
  else
    res FAIL "Tab did not complete /statu to /status; highlighted '$top'; composer: $(composer_line)"
  fi
  keys main Escape
  clear_draft main
}

def input.ctrl_z input "Ctrl+Z suspends, fg resumes" "Suspends to the shell"
chk_input_ctrl_z() {
  local cwd line
  cwd=$(cwd_of shell)
  [[ -d $cwd ]] || prepare_cwd shell
  session_exists shell && tm kill-session -t shell
  tm new-session -d -s shell -x "$COLS" -y "$ROWS" -c "$cwd" env PS1='PARITY_SHELL$ ' bash --norc --noprofile -i
  wait_for shell 'PARITY_SHELL\$' 10 || { res FAIL "shell did not start"; return; }
  rs_cmd shell
  line=$(printf '%q ' "${RS_CMD[@]}" --model haiku)
  tm send-keys -t shell -l -- "$line"
  keys shell Enter
  if ! rs_wait_ready shell; then
    res FAIL "claude-rs did not start in the shell"
    return
  fi
  keys shell C-z
  if ! wait_for shell 'Stopped|suspended' 8; then
    shot
    res FAIL "no job-control stop after Ctrl+Z: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
    rs_stop shell
    return
  fi
  local stopped
  stopped=$(first_match 'Stopped|suspended')
  tm send-keys -t shell -l -- "fg"
  keys shell Enter
  if wait_for shell "$EMPTY_COMPOSER" 10; then
    res PASS "'$stopped', then fg restored the composer"
  else
    shot
    res FAIL "'$stopped' but fg did not restore the TUI"
  fi
  keys shell C-q
  wait_for shell 'PARITY_SHELL\$' 10 || true
  tm kill-session -t shell
}

# The quit keys end the session they run in, so they get their own.
def input.ctrl_d input "Ctrl+D on an empty prompt exits" "Exits (press twice)"
chk_input_ctrl_d() {
  rs_start quit --model haiku || { res FAIL "quit session did not start"; return; }
  keys quit C-d
  sleep 0.4
  keys quit C-d
  sleep 2
  if pane_dead quit; then
    res PASS "Ctrl+D twice exited"
  else
    res GAP "Ctrl+D twice on an empty prompt does not exit (it is delete-after-cursor in claude-rs)"
  fi
}

def input.ctrl_c input "Ctrl+C clears the draft, then quits" "First clears input; twice on empty exits"
chk_input_ctrl_c() {
  if ! session_exists quit || pane_dead quit; then
    rs_start quit --model haiku || { res FAIL "quit session did not start"; return; }
  fi
  type_text quit "pm_ctrl_c_draft"
  keys quit C-c
  if ! wait_for quit "$EMPTY_COMPOSER" 3; then
    res FAIL "Ctrl+C did not clear the draft: $(composer_line)"
    rs_stop quit
    return
  fi
  # Shutdown runs the SessionEnd hooks, so give one press time to finish
  # before sending another (a second press forces cleanup to stop).
  local presses=1 i
  keys quit C-c
  for ((i = 0; i < 24 && presses < 3; i++)); do
    pane_dead quit && break
    if ((i == 11)); then presses=2; keys quit C-c; fi
    sleep 0.5
  done
  if pane_dead quit; then
    res PASS "first Ctrl+C cleared the draft; $presses press(es) on the empty prompt quit (stock asks for two)"
  else
    res FAIL "Ctrl+C on an empty prompt did not quit"
  fi
  rs_stop quit
}
