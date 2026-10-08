# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Session name and colour (/rename, /color), live and after a resume, run
# twice: with the bare binary, and through the owner's launch path
# (system-config's claude-launch), where the owner reported "/rename is not
# working". Stock reference (2.1.293): /rename prints "Session renamed to:
# <name>" and the rule above the prompt carries the name; /color prints
# "Session color set to: <c>" and paints that rule; both survive a resume.

ID_VARIANTS=(bin launch)
ID_COLOR=blue
ID_RUN_TAG=$(date +%H%M%S)
declare -A ID_STARTED=() ID_RENAMED=() ID_COLORED=() ID_RESUMED=() ID_SID=() ID_SKIP=()

id_sess() { printf 'id-%s\n' "$1"; }
id_title() { printf 'parity-rn-%s-%s\n' "$1" "$ID_RUN_TAG"; }

# id_start <variant> [args...]: 0 started, 1 failed, 2 not available here.
id_start() {
  local v=$1
  shift
  if [[ $v == bin ]]; then
    rs_start "$(id_sess "$v")" --model haiku "$@"
  else
    launch_start "$(id_sess "$v")" --model haiku "$@"
  fi
}

# id_live <variant>: the live session, started once. Sets ID_SKIP when the
# launcher path cannot run here.
id_live() {
  local v=$1 rc
  [[ -n ${ID_SKIP[$v]:-} ]] && return 2
  if [[ -z ${ID_STARTED[$v]:-} ]]; then
    id_start "$v"
    rc=$?
    if ((rc == 2)); then
      ID_SKIP[$v]="claude-launch not found ($LAUNCHER) or no release layout for --bin (a source build needs --bridge)"
      return 2
    fi
    ((rc == 0)) || return 1
    ID_STARTED[$v]=1
    ID_SID[$v]=$(jq -r .sessionId <<<"$REG")
  fi
  return 0
}

id_renamed() {
  local v=$1
  id_live "$v" || return $?
  if [[ -z ${ID_RENAMED[$v]:-} ]]; then
    submit "$(id_sess "$v")" "/rename $(id_title "$v")" || return 1
    ID_RENAMED[$v]=1
  fi
}

id_colored() {
  local v=$1
  id_live "$v" || return $?
  if [[ -z ${ID_COLORED[$v]:-} ]]; then
    submit "$(id_sess "$v")" "/color $ID_COLOR" || return 1
    ID_COLORED[$v]=1
  fi
}

# id_resumed <variant>: quit the renamed, coloured session with Ctrl+Q and
# resume it (bin: -r <id>; launcher: claude-launch -r <name>, as the owner does).
id_resumed() {
  local v=$1 s
  [[ -n ${ID_RESUMED[$v]:-} ]] && return 0
  id_renamed "$v" || return $?
  id_colored "$v" || return $?
  s=$(id_sess "$v")
  wait_registry "$(cwd_of "$s")" ".name == \"$(id_title "$v")\"" 10 || true
  rs_stop "$s"
  if [[ $v == bin ]]; then
    id_start "$v" -r "${ID_SID[$v]}" || return 1
  else
    id_start "$v" -r "$(id_title "$v")" || return 1
  fi
  ID_RESUMED[$v]=1
  wait_for "$s" "$EMPTY_COMPOSER" 10 || true
  sleep 1
  SCREEN=$(screen "$s")
}

# id_guard <rc>: turn a setup failure into the check's reading.
id_guard() {
  case $1 in
    0) return 0 ;;
    2) res SKIP "${ID_SKIP[$2]}" ;;
    *) shot; res FAIL "session setup failed: $(grep -v '^ *$' <<<"$SCREEN" | tail -1)" ;;
  esac
  return 1
}

id_shot() {
  SCREEN=$(screen "$1")
  shot
}

id_shot_color() {
  id_shot "$1"
  screen_e "$1" >"$WORK/screens/$CUR.ansi.txt"
}

# 2026-10-09: saved TUI captures place a blank padding row between the
# session rule and ❯. Associate the nearest nonblank row with the last composer.
rule_index() {
  screen "$1" | awk '
    /^ ?❯ / { rule = previous; composer = NR }
    /[^[:space:]]/ { previous = NR }
    END { if (rule > 0 && composer - rule <= 3) print rule; else exit 1 }
  '
}

composer_index() { screen "$1" | grep -n -E '^ ?❯ ' | tail -1 | cut -d: -f1; }

rule_line() {
  local i
  i=$(rule_index "$1") || return 1
  screen "$1" | sed -n "${i}p" | sed -E 's/[[:space:]]+$//'
}

# sgr_of_line <sess> <line no>: the distinct SGR sequences on a row (capture -e).
sgr_of_line() {
  screen_e "$1" | sed -n "$2p" | grep -Eo $'\e\\[[0-9;]*m' | sed $'s/\e//' | sort -u
}

# rule_sgr <sess>: SGR on the rule row that the composer row does not also
# use, so the input area's own background never reads as a colour.
rule_sgr() {
  local i c
  i=$(rule_index "$1") || return 1
  c=$(composer_index "$1") || return 1
  comm -23 <(sgr_of_line "$1" "$i") <(sgr_of_line "$1" "$c") | paste -sd' ' -
}

# rule_names <sess> <name>: the rule row reads "──── <name> ─".
rule_names() {
  grep -Eq -- "─+ +$(ere_escape "$2") +─" <<<"$(rule_line "$1")"
}

# rule_coloured: a foreground colour on the rule and a background badge.
rule_coloured() {
  local sgr
  sgr=$(rule_sgr "$1")
  grep -Eq '(38;5;[0-9]+|38;2;[0-9;]+|\[3[1-7](;|m))' <<<"$sgr" &&
    grep -Eq '(48;5;[0-9]+|48;2;[0-9;]+|\[4[1-7](;|m))' <<<"$sgr"
}

# Per-variant check bodies; the def loop at the end binds them to ids.

id_live_output() {
  local v=$1 s t
  id_renamed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  t=$(id_title "$v")
  if wait_for "$s" "Session renamed to: $t" 10; then
    id_shot "$s"
    res PASS "$(first_match 'Session renamed to:')"
  else
    shot
    res FAIL "no 'Session renamed to: $t' after /rename; last chat row: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[\|^Loc\|❯' | tail -1)"
  fi
}

id_live_composer() {
  local v=$1 s
  id_renamed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  wait_for "$s" "$EMPTY_COMPOSER" 5 || true
  sleep 1
  if rule_names "$s" "$(id_title "$v")"; then
    id_shot "$s"
    res PASS "rule above the prompt: '$(rule_line "$s")'"
  else
    id_shot "$s"
    res FAIL "row above the prompt: '$(rule_line "$s")' (no name)"
  fi
}

id_persisted() {
  local v=$1 s t pid reg_file reg_name sess_name title transcript
  id_renamed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  t=$(id_title "$v")
  wait_registry "$(cwd_of "$s")" ".name == \"$t\"" 10
  reg_name=$(jq -r .name <<<"$REG")
  pid=$(jq -r .pid <<<"$REG")
  reg_file=$CONFIG_DIR/sessions/$pid.json
  sess_name=$(jq -r .name "$reg_file" 2>/dev/null)
  transcript=$(transcript_of "$s" "$(jq -r .sessionId <<<"$REG")")
  title=$(grep '"type":"custom-title"' "$transcript" 2>/dev/null | tail -1 | jq -r .customTitle 2>/dev/null)
  if [[ $reg_name == "$t" && $sess_name == "$t" && $title == "$t" ]]; then
    id_shot "$s"
    res PASS "sessions/$pid.json name, transcript custom-title and \`claude agents --json\` all read $t"
  else
    res FAIL "expected $t: sessions/$pid.json '$sess_name', custom-title '$title', agents --json '$reg_name'"
  fi
}

id_status_tab() {
  local v=$1 s t
  id_renamed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  t=$(id_title "$v")
  keys "$s" Escape
  enter_cmd "$s" /status
  if wait_for "$s" 'Session name:' 10; then
    if grep -q "Session name: $t" <<<"$SCREEN"; then
      id_shot "$s"
      res PASS "$(first_match 'Session name:')"
    else
      res FAIL "Status tab '$(first_match 'Session name:')' while the registry name is $t"
    fi
  else
    res FAIL "Status tab did not open"
  fi
  keys "$s" Escape
  wait_for "$s" "$EMPTY_COMPOSER" 5 || true
}

id_color_replies() {
  local v=$1 s got=() missing=()
  id_live "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  submit "$s" "/color mauve"
  if wait_for "$s" 'Invalid color "mauve"' 8; then got+=(invalid); else missing+=(invalid); fi
  submit "$s" "/color default"
  if wait_for "$s" 'Session color reset to default' 8; then got+=(reset); else missing+=(reset); fi
  id_colored "$v"
  if wait_for "$s" "Session color set to: $ID_COLOR" 8; then got+=(set); else missing+=(set); fi
  if ((${#missing[@]} == 0)); then
    id_shot "$s"
    res PASS "invalid, reset and 'Session color set to: $ID_COLOR' replies shown"
  else
    shot
    res FAIL "replies shown: ${got[*]:-none}; missing: ${missing[*]}"
  fi
}

id_color_rule() {
  local v=$1 s
  id_colored "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  wait_for "$s" "$EMPTY_COMPOSER" 5 || true
  sleep 1
  if rule_coloured "$s"; then
    id_shot_color "$s"
    res PASS "rule SGR: $(rule_sgr "$s")"
  else
    id_shot_color "$s"
    res FAIL "row above the prompt '$(rule_line "$s")' has no colour (SGR: $(rule_sgr "$s" || true))"
  fi
}

id_color_persisted() {
  local v=$1 s transcript c
  id_colored "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  transcript=$(transcript_of "$s" "$(jq -r .sessionId <<<"$(registry_entry "$(cwd_of "$s")")")")
  c=$(grep '"type":"agent-color"' "$transcript" 2>/dev/null | tail -1 | jq -r .agentColor 2>/dev/null)
  if [[ $c == "$ID_COLOR" ]]; then
    id_shot "$s"
    res PASS "transcript agent-color: $c"
  else
    res FAIL "transcript agent-color '${c:-none}', expected $ID_COLOR"
  fi
}

id_resume_name() {
  local v=$1 s t
  id_resumed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  t=$(id_title "$v")
  if wait_registry "$(cwd_of "$s")" ".name == \"$t\"" 15; then
    id_shot "$s"
    res PASS "after Ctrl+Q and resume the registry name is $t (session $(jq -r .sessionId <<<"$REG"))"
  else
    res FAIL "registry name after resume: '$(jq -r .name <<<"$REG")', expected $t (session $(jq -r .sessionId <<<"$REG"))"
  fi
}

id_resume_history() {
  local v=$1 s t
  id_resumed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  t=$(id_title "$v")
  SCREEN=$(screen "$s")
  shot
  if grep -q '<command-name>' <<<"$SCREEN"; then
    res FAIL "restored history shows the raw record: $(first_match '<command-name>')"
  elif grep -q "/rename $t" <<<"$SCREEN"; then
    res PASS "restored as '$(first_match "/rename $t")'$(grep -q "Session renamed to: $t" <<<"$SCREEN" && echo ' with its output')"
  else
    res FAIL "the /rename turn is missing from the restored history"
  fi
}

id_resume_composer() {
  local v=$1 s
  id_resumed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  if rule_names "$s" "$(id_title "$v")"; then
    id_shot "$s"
    res PASS "rule after resume: '$(rule_line "$s")'"
  else
    id_shot "$s"
    res FAIL "row above the prompt after resume: '$(rule_line "$s")' (no name)"
  fi
}

id_resume_color() {
  local v=$1 s
  id_resumed "$v"
  id_guard $? "$v" || return
  s=$(id_sess "$v")
  if rule_coloured "$s"; then
    id_shot_color "$s"
    res PASS "rule SGR after resume: $(rule_sgr "$s")"
  else
    id_shot_color "$s"
    res FAIL "no colour on the row above the prompt after resume (SGR: $(rule_sgr "$s" || true))"
  fi
  rs_stop "$s"
}

for v in "${ID_VARIANTS[@]}"; do
  if [[ $v == bin ]]; then how="bare binary"; else how="claude-launch"; fi
  def "rename.$v.live_output" identity "/rename output, live ($how)" "Prints \`Session renamed to: <name>\`"
  def "rename.$v.live_composer" identity "/rename name in the composer, live ($how)" "Rule above the prompt reads \`──── <name> ─\`"
  def "rename.$v.persisted" identity "/rename persisted ($how)" "sessions/<pid>.json, transcript custom-title and \`claude agents --json\` carry the name"
  def "rename.$v.status_tab" identity "Session name in /status ($how)" "Status shows the session name"
  def "color.$v.replies" identity "/color replies ($how)" "Set, reset-to-default and invalid-colour lines"
  def "color.$v.live_rule" identity "/color paints the composer rule ($how)" "Rule in the colour, name as a black-on-colour badge"
  def "color.$v.persisted" identity "/color persisted ($how)" "Transcript \`agent-color\` entry"
  def "rename.$v.resume_name" identity "Name kept after Ctrl+Q and resume ($how)" "Registry keeps the name"
  def "rename.$v.resume_history" identity "/rename in the restored history ($how)" "Shows \`/rename <name>\` and its output"
  def "rename.$v.resume_composer" identity "Name in the composer after resume ($how)" "Rule carries the stored title"
  def "color.$v.resume_rule" identity "Colour kept after resume ($how)" "Rule repainted from the transcript"
  eval "chk_rename_${v}_live_output() { id_live_output $v; }
    chk_rename_${v}_live_composer() { id_live_composer $v; }
    chk_rename_${v}_persisted() { id_persisted $v; }
    chk_rename_${v}_status_tab() { id_status_tab $v; }
    chk_color_${v}_replies() { id_color_replies $v; }
    chk_color_${v}_live_rule() { id_color_rule $v; }
    chk_color_${v}_persisted() { id_color_persisted $v; }
    chk_rename_${v}_resume_name() { id_resume_name $v; }
    chk_rename_${v}_resume_history() { id_resume_history $v; }
    chk_rename_${v}_resume_composer() { id_resume_composer $v; }
    chk_color_${v}_resume_rule() { id_resume_color $v; }"
done
unset v how
