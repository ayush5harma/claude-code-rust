# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Session name (/rename) and the /color command, live and after a resume.
# Checked: /rename prints "Session renamed to: <name>" in the chat (local
# command replies), the name reaches the registry, the transcript and the
# Status tab, and survives a resume; /color runs inside Claude Code, its
# replies show in the chat and the transcript gets its agent-color record.
# Not checked, because claude-rs no longer has them: a name or colour drawn on
# the rule above the prompt, and the agent view.

ID_SESS=identity
ID_COLOR=blue
ID_TITLE=parity-rn-$(date +%H%M%S)
ID_STARTED='' ID_RENAMED='' ID_COLORED='' ID_RESUMED='' ID_SID=''

# id_live: the live session, started once.
id_live() {
  [[ -n $ID_STARTED ]] && return 0
  rs_start "$ID_SESS" --model "$MODEL" || return 1
  ID_STARTED=1
  ID_SID=$(jq -r .sessionId <<<"$REG")
}

id_renamed() {
  id_live || return 1
  [[ -n $ID_RENAMED ]] && return 0
  submit "$ID_SESS" "/rename $ID_TITLE" || return 1
  ID_RENAMED=1
}

id_colored() {
  id_live || return 1
  [[ -n $ID_COLORED ]] && return 0
  submit "$ID_SESS" "/color $ID_COLOR" || return 1
  ID_COLORED=1
}

# id_resumed: quit the renamed, coloured session with Ctrl+Q and resume it
# by id.
id_resumed() {
  [[ -n $ID_RESUMED ]] && return 0
  id_renamed || return 1
  id_colored || return 1
  wait_registry "$(cwd_of "$ID_SESS")" ".name == \"$ID_TITLE\"" 10 || true
  rs_stop "$ID_SESS"
  rs_start "$ID_SESS" --model "$MODEL" -r "$ID_SID" || return 1
  ID_RESUMED=1
  wait_for "$ID_SESS" "$EMPTY_COMPOSER" 10 || true
  sleep 1
  SCREEN=$(screen "$ID_SESS")
}

# id_guard <rc>: turn a setup failure into the check's reading.
id_guard() {
  ((${1} == 0)) && return 0
  shot
  res FAIL "session setup failed: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)"
  return 1
}

id_shot() {
  SCREEN=$(screen "$ID_SESS")
  shot
}

def rename.live_output identity "/rename output, live" "Prints \`Session renamed to: <name>\`"
chk_rename_live_output() {
  id_renamed
  id_guard $? || return
  if wait_for "$ID_SESS" "Session renamed to: $ID_TITLE" 10; then
    id_shot
    res PASS "$(first_match 'Session renamed to:')"
  else
    shot
    res FAIL "no 'Session renamed to: $ID_TITLE' after /rename; last chat row: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[\|^Loc\|❯' | tail -1)"
  fi
}

def rename.persisted identity "/rename persisted" "sessions/<pid>.json, transcript custom-title and \`claude agents --json\` carry the name"
chk_rename_persisted() {
  local pid reg_file reg_name sess_name title transcript
  id_renamed
  id_guard $? || return
  wait_registry "$(cwd_of "$ID_SESS")" ".name == \"$ID_TITLE\"" 10
  reg_name=$(jq -r .name <<<"$REG")
  pid=$(jq -r .pid <<<"$REG")
  reg_file=$CONFIG_DIR/sessions/$pid.json
  sess_name=$(jq -r .name "$reg_file" 2>/dev/null)
  transcript=$(transcript_of "$ID_SESS" "$(jq -r .sessionId <<<"$REG")")
  title=$(grep '"type":"custom-title"' "$transcript" 2>/dev/null | tail -1 | jq -r .customTitle 2>/dev/null)
  if [[ $reg_name == "$ID_TITLE" && $sess_name == "$ID_TITLE" && $title == "$ID_TITLE" ]]; then
    id_shot
    res PASS "sessions/$pid.json name, transcript custom-title and \`claude agents --json\` all read $ID_TITLE"
  else
    res FAIL "expected $ID_TITLE: sessions/$pid.json '$sess_name', custom-title '$title', agents --json '$reg_name'"
  fi
}

def rename.status_tab identity "Session name in /status" "Status shows the session name"
chk_rename_status_tab() {
  id_renamed
  id_guard $? || return
  keys "$ID_SESS" Escape
  enter_cmd "$ID_SESS" /status
  if wait_for "$ID_SESS" 'Session name:' 10; then
    if grep -q "Session name: $ID_TITLE" <<<"$SCREEN"; then
      id_shot
      res PASS "$(first_match 'Session name:')"
    else
      res FAIL "Status tab '$(first_match 'Session name:')' while the registry name is $ID_TITLE"
    fi
  else
    res FAIL "Status tab did not open"
  fi
  keys "$ID_SESS" Escape
  wait_for "$ID_SESS" "$EMPTY_COMPOSER" 5 || true
}

def color.replies identity "/color replies" "Set, reset-to-default and invalid-colour lines"
chk_color_replies() {
  local got=() missing=()
  id_live
  id_guard $? || return
  submit "$ID_SESS" "/color mauve"
  if wait_for "$ID_SESS" 'Invalid color "mauve"' 8; then got+=(invalid); else missing+=(invalid); fi
  submit "$ID_SESS" "/color default"
  if wait_for "$ID_SESS" 'Session color reset to default' 8; then got+=(reset); else missing+=(reset); fi
  id_colored
  if wait_for "$ID_SESS" "Session color set to: $ID_COLOR" 8; then got+=(set); else missing+=(set); fi
  if ((${#missing[@]} == 0)); then
    id_shot
    res PASS "invalid, reset and 'Session color set to: $ID_COLOR' replies shown"
  else
    shot
    res FAIL "replies shown: ${got[*]:-none}; missing: ${missing[*]}"
  fi
}

def color.persisted identity "/color persisted" "Transcript \`agent-color\` entry"
chk_color_persisted() {
  local transcript c
  id_colored
  id_guard $? || return
  transcript=$(transcript_of "$ID_SESS" "$(jq -r .sessionId <<<"$(registry_entry "$(cwd_of "$ID_SESS")")")")
  c=$(grep '"type":"agent-color"' "$transcript" 2>/dev/null | tail -1 | jq -r .agentColor 2>/dev/null)
  if [[ $c == "$ID_COLOR" ]]; then
    id_shot
    res PASS "transcript agent-color: $c"
  else
    res FAIL "transcript agent-color '${c:-none}', expected $ID_COLOR"
  fi
}

# Stock keeps the name across a resume. Under claude-rs the registry entry of
# the resumed session reads whatever Claude Code assigns it; the bridge passes
# no --name on resume (the fork's resume handling went with 7bcd788), so a
# missing name reads GAP, not FAIL. The evidence states only what was read.
def rename.resume_name identity "Name kept after Ctrl+Q and resume" "Registry keeps the name"
chk_rename_resume_name() {
  id_resumed
  id_guard $? || return
  if wait_registry "$(cwd_of "$ID_SESS")" ".name == \"$ID_TITLE\"" 15; then
    id_shot
    res PASS "after Ctrl+Q and resume the registry name is $ID_TITLE (session $(jq -r .sessionId <<<"$REG"))"
  else
    res GAP "registry name after resume: '$(jq -r .name <<<"$REG")' (session $(jq -r .sessionId <<<"$REG")); it was $ID_TITLE before Ctrl+Q, which stock keeps"
  fi
}

def rename.resume_history identity "/rename in the restored history" "Shows \`/rename <name>\` and its output"
chk_rename_resume_history() {
  id_resumed
  id_guard $? || return
  SCREEN=$(screen "$ID_SESS")
  shot
  if grep -q '<command-name>' <<<"$SCREEN"; then
    res FAIL "restored history shows the raw record: $(first_match '<command-name>')"
  elif grep -q "/rename $ID_TITLE" <<<"$SCREEN"; then
    res PASS "restored as '$(first_match "/rename $ID_TITLE")'$(grep -q "Session renamed to: $ID_TITLE" <<<"$SCREEN" && echo ' with its output')"
  else
    res FAIL "the /rename turn is missing from the restored history"
  fi
  # The last check that uses the resumed session.
  rs_stop "$ID_SESS"
}
