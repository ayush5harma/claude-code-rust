# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Fork features around sessions: the /resume picker (current marker, tags,
# deletion, compact-first), resume by title, --fork-session, and the session
# accent and composer style. Every session here is created by the suite in its
# own scratch cwds, so tagging and deleting never reach another session. The
# titled sessions cost nothing: /rename is a local command and needs no turn.

# fk_title_sessions: three sessions in one cwd. A and B share a title, C has
# its own, and C is left running (the active session).
fk_title_sessions() {
  [[ -n $FK_TITLE_DONE ]] && return 0
  FK_TITLE_DONE=1
  local stamp which title id
  stamp=$(date +%H%M%S)
  FK_TITLE_DUP=FK_DUP_$stamp
  FK_TITLE_UNIQ=FK_UNIQ_$stamp
  prepare_cwd fktitle || return 1
  for which in A B C; do
    title=$FK_TITLE_DUP
    [[ $which == C ]] && title=$FK_TITLE_UNIQ
    rs_start fktitle --model "$MODEL" --permission-mode bypassPermissions || return 1
    id=$(jq -r .sessionId <<<"$REG")
    submit fktitle "/rename $title" || return 1
    wait_registry "$(cwd_of fktitle)" ".name == \"$title\"" 15 || return 1
    printf -v "FK_TITLE_$which" '%s' "$id"
    [[ $which == C ]] || rs_stop fktitle
  done
}

def fork.picker_current fork "/resume picker: current session" "Active session listed and marked"
chk_fork_picker_current() {
  fk_title_sessions || { res FAIL "could not create the three titled sessions"; return; }
  open_view fktitle /resume 'Select a session to resume' || { shot; res FAIL "no session picker"; return; }
  if wait_for fktitle '\[current\]' 8; then
    shot
    res PASS "picker rows: $(grep -E 'ago - |just now - ' <<<"$SCREEN" | squash | paste -sd'|' -)"
  else
    shot
    res FAIL "the active session (renamed $FK_TITLE_UNIQ, transcript on disk) is not listed or marked [current]: $(grep -E 'ago - |just now - |No recent sessions' <<<"$SCREEN" | squash | paste -sd'|' -)"
    defect "The /resume picker opened inside a running session shows a stale list" \
      "1. Start claude-rs in a directory that already holds two sessions. 2. Rename the session with /rename (a transcript is written). 3. Type /resume." \
      "$(grep -v '^ *$' <<<"$SCREEN" | head -12)"
  fi
  close_view fktitle
}

def fork.resume_title_ambiguous fork "/resume <ambiguous title>" "Reports the ambiguity and the session ids"
chk_fork_resume_title_ambiguous() {
  fk_title_sessions || { res FAIL "could not create the three titled sessions"; return; }
  settle fktitle || true
  enter_cmd fktitle "/resume $FK_TITLE_DUP"
  if wait_for fktitle 'ambiguous session title' 12; then
    res PASS "$(first_match 'ambiguous session title')"
  else
    sleep 3
    SCREEN=$(screen fktitle)
    shot
    res FAIL "two sessions are titled $FK_TITLE_DUP: no ambiguity message; $(grep -c 'Type a message' <<<"$SCREEN" | sed 's/^0$/composer gone/; s/^1$/composer present/'), last row: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[\|^Loc\|^personal' | tail -1)"
    defect "/resume <title> that matches two sessions shows no error and leaves the UI stuck on \"Resuming session\"" \
      "1. Have two sessions in one project with the same /rename title. 2. In a third session type /resume <that title> and press Enter. The bridge answers with a slash_error (ambiguous session title, logged as bridge_event slash_error), but the screen only shows the spinner row and the composer does not come back; only Ctrl+Q works. The same happens for /resume <unknown title> on the installed 0.15.2-fork.2, and for claude-rs -r <ambiguous title> at launch (an empty composer, no mode or model in the footer)." \
      "$(grep -v '^ *$' <<<"$SCREEN" | tail -8)"
  fi
  # The session may be wedged: leave it by Ctrl+Q (rs_stop copes with that).
  rs_stop fktitle
}

def fork.resume_title fork "-r <exact title>" "Resumes the one session with that title"
chk_fork_resume_title() {
  fk_title_sessions || { res FAIL "could not create the three titled sessions"; return; }
  session_exists fktitle && rs_stop fktitle
  if ! rs_start fktitle --model "$MODEL" -r "$FK_TITLE_UNIQ"; then
    shot
    res FAIL "-r $FK_TITLE_UNIQ did not reach the composer"
    return
  fi
  wait_for fktitle '/rename|FK_UNIQ' 15 || true
  shot
  if [[ $(jq -r .sessionId <<<"$REG") == "$FK_TITLE_C" ]] && grep -q "$FK_TITLE_UNIQ" <<<"$(fk_footer_row1)"; then
    res PASS "-r $FK_TITLE_UNIQ resumed session $FK_TITLE_C; footer: $(fk_footer_row1)"
  else
    res FAIL "registry session $(jq -r .sessionId <<<"$REG") (wanted $FK_TITLE_C); footer: $(fk_footer_row1)"
  fi
}

# The previous check leaves C running; the fork starts from it.
def fork.flag_fork_session fork "--fork-session" "Resumes into a new session id; the original is untouched"
chk_fork_flag_fork_session() {
  fk_title_sessions || { res FAIL "could not create the sessions to resume"; return; }
  local orig=$FK_TITLE_C file before after id
  file=$(transcript_of fktitle "$orig")
  # A session writes to its transcript when it ends, so measure after that.
  session_exists fktitle && rs_stop fktitle
  before=$(cksum <"$file")
  rs_start fktitle --model "$MODEL" -r "$orig" --fork-session || { shot; res FAIL "-r --fork-session did not reach the composer"; return; }
  id=$(jq -r .sessionId <<<"$REG")
  wait_for fktitle '/rename' 15 || true
  shot
  after=$(cksum <"$file")
  if [[ -n $id && $id != null && $id != "$orig" && $before == "$after" ]]; then
    res PASS "forked as $id from $orig; the original transcript is byte-identical (cksum $before); history replayed: $(first_match '/rename' || echo none)"
  else
    res FAIL "forked id '$id' vs original $orig; original transcript unchanged: $([[ $before == "$after" ]] && echo yes || echo no)"
  fi
}

# --------------------------------------------------------- picker actions

# fk_picker_row_count: rows of the open picker.
fk_picker_row_count() { grep -Ec '(ago|just now) - ' <<<"${1-$SCREEN}" || true; }

# fk_picker_manage <sess> <key> <ids...>: move down the picker until <key> (t
# or d) opens the manage overlay of one of the listed ids, and print that id.
# The overlay stays open. Anything else (the active session, a session the
# suite did not create) is backed out of with Esc and never acted on.
fk_picker_manage() {
  local s=$1 key=$2 id i a
  shift 2
  for ((i = 0; i < 8; i++)); do keys "$s" Up; done
  for ((i = 0; i < 8; i++)); do
    keys "$s" "$key"
    if wait_for "$s" '(Session: |Delete session )[0-9a-f-]{36}' 2; then
      id=$(grep -Eo '(Session: |Delete session )[0-9a-f-]{36}' <<<"$SCREEN" | head -1 | grep -Eo '[0-9a-f-]{36}')
      for a in "$@"; do
        [[ $a == "$id" ]] && { printf '%s\n' "$id"; return 0; }
      done
      keys "$s" Escape
      wait_for "$s" 'Select a session to resume' 3 || true
    fi
    keys "$s" Down
    sleep 0.3
  done
  return 1
}

# fk_picker_open <sess>: the /resume picker of the running session, after a
# /rename so that the session has a transcript and can be listed itself.
fk_picker_open() {
  local s=$1
  settle "$s" || true
  grep -q 'Session renamed to: FK_ACT' <<<"$(screen "$s")" ||
    { submit "$s" "/rename FK_ACT_$(date +%H%M%S)" && wait_for "$s" 'Session renamed to: FK_ACT' 10; } || true
  open_view "$s" /resume 'Select a session to resume'
}

def fork.session_tag_paste fork "Session tag: pasted text" "A paste lands in the tag field"
chk_fork_session_tag_paste() {
  fk_title_sessions || { res FAIL "could not create the sessions"; return; }
  session_exists fktitle && ! pane_dead fktitle || rs_start fktitle --model "$MODEL" -r "$FK_TITLE_C" --fork-session || { res FAIL "no session"; return; }
  fk_picker_open fktitle || { shot; res FAIL "no session picker"; return; }
  wait_for fktitle 'ago - |just now - ' 8 || true
  local id
  if ! id=$(fk_picker_manage fktitle t "$FK_TITLE_A" "$FK_TITLE_B" "$FK_TITLE_C"); then
    shot
    res FAIL "no taggable suite session in the picker"
    close_view fktitle
    return
  fi
  type_text_raw fktitle "fkpaste"
  sleep 0.5
  SCREEN=$(screen fktitle)
  shot
  if grep -q 'Tag: fkpaste' <<<"$SCREEN"; then
    res PASS "pasting into the tag field of $id shows: $(first_match 'Tag:')"
  else
    res FAIL "pasting fkpaste into the tag field of $id left it empty: '$(first_match 'Tag:')' (typed keys do work)"
    defect "Pasted text is ignored in the session tag field" \
      "1. Open /resume. 2. Press t on a session. 3. Paste text (bracketed paste, as a terminal sends it). The Tag: field stays empty and Enter then saves an empty tag, clearing the session's existing tag (transcript record {\"type\":\"tag\",\"tag\":\"\"}); typing the same characters works." \
      "$(grep -v '^ *$' <<<"$SCREEN" | head -8)"
  fi
  keys fktitle Escape
  wait_for fktitle 'Select a session to resume' 3 || true
  close_view fktitle
}

def fork.session_tag fork "Session tag" "Tag saved through the SDK and shown in the picker"
chk_fork_session_tag() {
  fk_title_sessions || { res FAIL "could not create the sessions"; return; }
  session_exists fktitle && ! pane_dead fktitle || rs_start fktitle --model "$MODEL" -r "$FK_TITLE_C" --fork-session || { res FAIL "no session"; return; }
  fk_picker_open fktitle || { shot; res FAIL "no session picker"; return; }
  wait_for fktitle 'ago - |just now - ' 8 || true
  local id
  if ! id=$(fk_picker_manage fktitle t "$FK_TITLE_A" "$FK_TITLE_B"); then
    shot
    res FAIL "no taggable suite session in the picker"
    close_view fktitle
    return
  fi
  keys fktitle -l 'fk-tag'
  sleep 0.3
  keys fktitle Enter
  if wait_for fktitle '\[tag: fk-tag\]' 15; then
    shot
    res PASS "tagged $id; picker row: $(first_match '\[tag: fk-tag\]' | squash)"
  else
    shot
    res FAIL "saved a tag for $id but no [tag: fk-tag] row: $(grep -E 'ago - |just now - ' <<<"$SCREEN" | squash | paste -sd'|' -)"
  fi
  close_view fktitle
}

def fork.session_delete fork "Session deletion" "Confirmation, active-session protection, file removed"
chk_fork_session_delete() {
  fk_title_sessions || { res FAIL "could not create the sessions"; return; }
  session_exists fktitle && ! pane_dead fktitle || rs_start fktitle --model "$MODEL" -r "$FK_TITLE_C" --fork-session || { res FAIL "no session"; return; }
  fk_picker_open fktitle || { shot; res FAIL "no session picker"; return; }
  wait_for fktitle 'ago - |just now - ' 8 || true
  # A tag save refreshes the list, which is when the active session shows up.
  local id protect="" before after i file
  if id=$(fk_picker_manage fktitle t "$FK_TITLE_A" "$FK_TITLE_B"); then
    keys fktitle -l 'fk-del'
    keys fktitle Enter
    wait_for fktitle '\[tag: fk-del\]|\[current\]' 15 || true
  fi
  # Protection: d on the active session is refused.
  for ((i = 0; i < 8; i++)); do
    grep -Eq '^[│ ]*> .*\[current\]' <<<"$(screen fktitle)" && break
    keys fktitle Down
    sleep 0.3
  done
  if grep -Eq '^[│ ]*> .*\[current\]' <<<"$(screen fktitle)"; then
    keys fktitle d
    wait_for fktitle 'Switch sessions before deleting the active session' 5 && protect=$(first_match 'Switch sessions before deleting' | squash)
  fi
  SCREEN=$(screen fktitle)
  shot active
  before=$(fk_picker_row_count)
  # Delete: only an id the suite created in this cwd, only after y.
  if ! id=$(fk_picker_manage fktitle d "$FK_TITLE_A" "$FK_TITLE_B"); then
    shot
    res FAIL "no deletable suite session reached in the picker (protection: ${protect:-none})"
    close_view fktitle
    return
  fi
  file=$(transcript_of fktitle "$id")
  SCREEN=$(screen fktitle)
  grep -q 'Press y to confirm' <<<"$SCREEN" || { res FAIL "no 'Press y to confirm' for $id"; keys fktitle Escape; close_view fktitle; return; }
  keys fktitle y
  for ((i = 0; i < 40; i++)); do
    [[ -e $file ]] || break
    sleep 0.5
  done
  wait_for fktitle 'Select a session to resume' 5 || true
  after=$(fk_picker_row_count)
  shot deleted
  if [[ ! -e $file && -n $protect ]]; then
    res PASS "confirmed delete of $id: transcript removed, rows $before -> $after; active session: '$protect'"
  elif [[ ! -e $file ]]; then
    res FAIL "deleted $id (rows $before -> $after) but the active session was not protected in the picker"
  else
    res FAIL "pressed y for $id but its transcript is still on disk"
  fi
  close_view fktitle
  rs_stop fktitle
}

def fork.compact_first fork "Resume picker: large sessions" "Size shown; compact-first offered from 1 MiB"
chk_fork_compact_first() {
  local out id
  prepare_cwd fkbig || { res FAIL "could not prepare the cwd"; return; }
  # 400 records of ~6 KB each: about 1.3 MiB, offline, no model call.
  out=$(node "$SUITE_DIR/fixtures/long-transcript.mjs" "$CONFIG_DIR" "$WORK" "$CC_VERSION" fkbig 100) || { res FAIL "could not write the synthetic transcript"; return; }
  id=$(cut -f1 <<<"$out")
  rs_cmd fkbig
  session_exists fkbig && tm kill-session -t fkbig
  tm new-session -d -s fkbig -x "$COLS" -y "$ROWS" -c "$(cwd_of fkbig)" "${RS_CMD[@]}" --model "$MODEL" -r
  wait_for fkbig 'Trust this project directory|Select a session to resume' 40 || true
  if grep -q 'Trust this project directory' <<<"$SCREEN"; then
    keys fkbig Enter
  fi
  if ! wait_for fkbig 'Synthetic parity prompt|saved' 30; then
    shot
    res FAIL "the picker did not list the synthetic session $id"
    tm kill-session -t fkbig
    return
  fi
  sleep 1
  SCREEN=$(screen fkbig)
  shot
  if grep -Eq '\[saved [0-9.]+ MiB\]' <<<"$SCREEN" && grep -q 'c compact-first (1 MiB+)' <<<"$SCREEN"; then
    res PASS "row: $(first_match 'saved [0-9.]+ MiB' | squash); hint: $(first_match 'compact-first' | squash) (c not pressed: compaction would bill a 1.3 MiB model turn)"
  else
    res FAIL "no size on the row or no compact-first hint for a $(wc -c <"$(transcript_of fkbig "$id")" | tr -d ' ') byte session: $(first_match 'ago - |just now - ' | squash)"
  fi
  keys fkbig Escape
  sleep 0.5
  tm kill-session -t fkbig
}

# --------------------------------------------------------------- accent

# fk_accent_rgb <session id>: the palette entry for the id, computed here
# (FNV-1a over the id's bytes, mod 8) from the palette documented in
# src/app/session_accent.rs, independently of the app.
fk_accent_rgb() {
  local -a palette=("245,115,120" "246,160,90" "220,195,85" "105,205,145" "90,200,215" "120,175,245" "190,155,240" "235,145,190")
  local id=$1 h=-3750763034362895579 i
  for ((i = 0; i < ${#id}; i++)); do
    h=$(((h ^ $(printf '%d' "'${id:i:1}")) * 1099511628211))
  done
  printf '%s\n' "${palette[h & 7]}"
}

# fk_prompt_rgb <sess>: the colour of the composer's prompt row, as R,G,B.
fk_prompt_rgb() {
  screen_e "$1" | LC_ALL=C grep -a 'Type a message' | head -1 |
    LC_ALL=C sed -nE 's/.*\x1b\[38;2;([0-9]+);([0-9]+);([0-9]+)m.*/\1,\2,\3/p'
}

def fork.accent fork "Session accent and /color" "Stable colour per session; /color changes it"
chk_fork_accent() {
  local id want got blue back
  prepare_cwd fkaccent || { res FAIL "could not prepare the cwd"; return; }
  rs_start fkaccent --model "$MODEL" || { res FAIL "session did not start"; return; }
  id=$(jq -r .sessionId <<<"$REG")
  want=$(fk_accent_rgb "$id")
  got=$(fk_prompt_rgb fkaccent)
  SCREEN=$(screen fkaccent)
  shot
  if [[ $got != "$want" ]]; then
    res FAIL "session $id: prompt row colour '${got:-none}', the id-derived palette entry is $want"
    rs_stop fkaccent
    return
  fi
  submit fkaccent "/color blue"
  wait_for fkaccent 'Session color set to: blue' 10 || true
  sleep 0.5
  blue=$(fk_prompt_rgb fkaccent)
  submit fkaccent "/color default"
  wait_for fkaccent 'Session color reset to default' 10 || true
  sleep 0.5
  back=$(fk_prompt_rgb fkaccent)
  rs_stop fkaccent
  if [[ $blue == 120,175,245 && $back == "$want" ]]; then
    res PASS "id $id -> palette $got; /color blue -> $blue; /color default -> $back"
  else
    res FAIL "after /color blue the prompt was '${blue:-none}' (expected 120,175,245); after /color default '${back:-none}' (expected $want)"
  fi
}

def fork.composer_rules fork "Composer style: rules" "Rules above and below the prompt in the session colour"
chk_fork_composer_rules() {
  local f id want rule rows
  if ! f=$(fk_app_settings); then
    res SKIP "claude-rs's app settings path is only known on macOS"
    return
  fi
  if [[ -e $f ]]; then
    res SKIP "$f already exists; the suite does not touch preferences someone set"
    return
  fi
  # The composer style is an app preference stored outside the Claude profile;
  # write it for this session only and delete it again before the check ends.
  printf '%s\n' '{"presentation":{"composerStyle":"rules"}}' >"$f" || { res FAIL "could not write $f"; return; }
  FK_APP_SETTINGS_CREATED=$(cat "$f")
  prepare_cwd fkrules || { res FAIL "could not prepare the cwd"; return; }
  rs_start fkrules --model "$MODEL" || { rm -f "$f"; FK_APP_SETTINGS_CREATED=''; res FAIL "session did not start"; return; }
  sleep 1
  SCREEN=$(screen fkrules)
  shot
  rows=$(grep -nE '^─{40,}$|^ ?❯ ' <<<"$SCREEN" | cut -d: -f2- | cut -c1-24 | paste -sd'|' -)
  id=$(jq -r .sessionId <<<"$REG")
  want=$(fk_accent_rgb "$id")
  rule=$(screen_e fkrules | LC_ALL=C grep -a '─────' | head -1 | LC_ALL=C sed -nE 's/.*\x1b\[38;2;([0-9]+);([0-9]+);([0-9]+)m.*/\1,\2,\3/p')
  rm -f "$f"
  FK_APP_SETTINGS_CREATED=''
  rs_stop fkrules
  if [[ $(grep -cE '^─{40,}$' <<<"$SCREEN") -ge 2 ]] && grep -qE '^ ?❯ ' <<<"$SCREEN" && [[ $rule == "$want" ]]; then
    res PASS "rows: $rows; rule colour $rule is the session accent"
  else
    res FAIL "rules around the prompt not as documented: rows '$rows', rule colour '${rule:-none}', accent $want"
  fi
}
