# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Session name (/rename) and the /color command, live and after a resume, run
# twice: with the bare binary, and through the owner's launch path
# (system-config's claude-launch), where the owner reported "/rename is not
# working". Checked: /rename prints "Session renamed to: <name>", the rule
# above the prompt carries the name in the dim rule colour, and the name
# survives a resume; /color runs inside Claude Code, its replies show in the
# chat and the transcript gets its agent-color record. Not checked: any colour
# drawn on the rule or the name. claude-rs no longer draws it (it read Claude
# Code internals the upstream maintainer rejected); stock paints the rule,
# which gap.session_color records.

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

# 2026-10-09: saved TUI captures place a blank padding row between the
# session rule and ❯. Associate the nearest nonblank row with the last composer.
rule_index() {
  screen "$1" | awk '
    /^ ?❯ / { rule = previous; composer = NR }
    /[^[:space:]]/ { previous = NR }
    END { if (rule > 0 && composer - rule <= 3) print rule; else exit 1 }
  '
}

rule_line() {
  local i
  i=$(rule_index "$1") || return 1
  screen "$1" | sed -n "${i}p" | sed -E 's/[[:space:]]+$//'
}

# rule_names <sess> <name>: the rule row reads "──── <name> ─".
rule_names() {
  grep -Eq -- "─+ +$(ere_escape "$2") +─" <<<"$(rule_line "$1")"
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

# Under the Agent SDK, Claude Code does not put a resumed session's name back
# into its registry the way its own TUI does, and since 0.15.2-fork.1 the bridge
# no longer passes it back as --name, so a missing name reads GAP, not FAIL.
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
    res GAP "registry name after resume: '$(jq -r .name <<<"$REG")', stock keeps $t (session $(jq -r .sessionId <<<"$REG")); Claude Code does not restore it under the SDK"
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
  # The last check that uses the resumed session; it used to be stopped by the
  # colour-after-resume check that followed.
  rs_stop "$s"
}

for v in "${ID_VARIANTS[@]}"; do
  if [[ $v == bin ]]; then how="bare binary"; else how="claude-launch"; fi
  def "rename.$v.live_output" identity "/rename output, live ($how)" "Prints \`Session renamed to: <name>\`"
  def "rename.$v.live_composer" identity "/rename name in the composer, live ($how)" "Rule above the prompt reads \`──── <name> ─\`"
  def "rename.$v.persisted" identity "/rename persisted ($how)" "sessions/<pid>.json, transcript custom-title and \`claude agents --json\` carry the name"
  def "rename.$v.status_tab" identity "Session name in /status ($how)" "Status shows the session name"
  def "color.$v.replies" identity "/color replies ($how)" "Set, reset-to-default and invalid-colour lines"
  def "color.$v.persisted" identity "/color persisted ($how)" "Transcript \`agent-color\` entry"
  def "rename.$v.resume_name" identity "Name kept after Ctrl+Q and resume ($how)" "Registry keeps the name"
  def "rename.$v.resume_history" identity "/rename in the restored history ($how)" "Shows \`/rename <name>\` and its output"
  def "rename.$v.resume_composer" identity "Name in the composer after resume ($how)" "Rule carries the stored title"
  eval "chk_rename_${v}_live_output() { id_live_output $v; }
    chk_rename_${v}_live_composer() { id_live_composer $v; }
    chk_rename_${v}_persisted() { id_persisted $v; }
    chk_rename_${v}_status_tab() { id_status_tab $v; }
    chk_color_${v}_replies() { id_color_replies $v; }
    chk_color_${v}_persisted() { id_color_persisted $v; }
    chk_rename_${v}_resume_name() { id_resume_name $v; }
    chk_rename_${v}_resume_history() { id_resume_history $v; }
    chk_rename_${v}_resume_composer() { id_resume_composer $v; }"
done
unset v how
