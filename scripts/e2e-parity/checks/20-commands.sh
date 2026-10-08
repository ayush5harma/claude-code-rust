# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Built-in slash commands: the inventory of every stock built-in, and the
# ones the matrix names, exercised one by one.

# Stock names with an app equivalent under another name or key.
declare -A COMMAND_EQUIVALENT=(
  [plugin]="/plugins"
  [exit]="Ctrl+Q (Ctrl+C on an empty draft)"
  [plan]="/mode plan, Shift+Tab"
)

# sdk_command_names <sess>: the command list the stock child advertised, as
# claude-rs logged it (bridge diagnostics preset).
sdk_command_names() {
  grep -F 'available commands snapshot accepted' "$WORK/logs/$1.log" 2>/dev/null | tail -1 |
    jq -r '.fields_json | fromjson | .command_names[]' 2>/dev/null
}

app_command_names() {
  grep -Eo 'name: "/[a-z-]+"' "$SRC_DIR/src/app/slash/catalog.rs" | sed -E 's/name: "\/(.*)"/\1/'
}

def cmd.inventory commands "Every stock built-in classified" \
  "100 built-ins in 2.1.293 (local, local-jsx, prompt)"
chk_cmd_inventory() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local sdk app name kinds class note n_app=0 n_fwd=0 n_eq=0 n_gap=0 total=0
  wait_for main "$EMPTY_COMPOSER" 10 || true
  sdk=$(sdk_command_names main)
  app=$(app_command_names)
  if [[ -z $sdk ]]; then
    res FAIL "no advertised command list in $WORK/logs/main.log"
    return
  fi
  if [[ -z $app ]]; then
    res FAIL "no app commands parsed from $SRC_DIR/src/app/slash/catalog.rs"
    return
  fi
  : >"$WORK/inventory.tsv"
  while read -r name; do
    kinds=$(awk -v n="$name" '$1 == n {print $2}' "$SUITE_DIR/data/stock-commands.txt" | paste -sd, - | sed 's/,/, /g')
    note=""
    if grep -qx -- "$name" <<<"$app"; then
      class="app-owned"
      n_app=$((n_app + 1))
      grep -qx -- "$name" <<<"$sdk" && note="also advertised by the SDK; the app command wins"
    elif grep -qx -- "$name" <<<"$sdk"; then
      class="forwarded"
      n_fwd=$((n_fwd + 1))
    elif [[ -n ${COMMAND_EQUIVALENT[$name]:-} ]]; then
      class="equivalent"
      note=${COMMAND_EQUIVALENT[$name]}
      n_eq=$((n_eq + 1))
    else
      class="**stock-only**"
      note="not app-owned and not advertised over the SDK"
      n_gap=$((n_gap + 1))
    fi
    total=$((total + 1))
    printf '%s\t%s\t%s\t%s\n' "$name" "$kinds" "$class" "$note" >>"$WORK/inventory.tsv"
  done < <(awk '!/^#/ {print $1}' "$SUITE_DIR/data/stock-commands.txt" | sort -u)
  res PASS "$total stock built-ins: $n_app app-owned, $n_fwd forwarded, $n_eq equivalent, $n_gap stock-only (table below); SDK advertised $(wc -l <<<"$sdk" | tr -d ' ') commands"
}

# ---------------------------------------------------------- session state

def cmd.model commands "/model <id>" "Switches the session model"
chk_cmd_model() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main "/model sonnet"
  if ! wait_for main '\[Sonnet' 15; then
    res FAIL "footer did not switch to Sonnet: $(footer | head -1)"
    submit main "/model haiku"
    return
  fi
  local mid
  mid=$(footer | head -1)
  submit main "/model haiku"
  if wait_for main '\[Haiku' 15; then
    res PASS "footer followed: $mid, then back to Haiku"
  else
    res FAIL "switched to Sonnet but not back to Haiku: $(footer | head -1)"
  fi
}

def cmd.effort commands "/effort <level>" "Changes the session effort"
chk_cmd_effort() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main "/effort low"
  if ! wait_for main '/Low\]' 15; then
    res FAIL "footer did not show Low: $(footer | head -1)"
    return
  fi
  submit main "/effort medium"
  if wait_for main '/Med\]' 15; then
    res PASS "footer went to /Low] and back to /Med]"
  else
    res FAIL "footer did not return to medium: $(footer | head -1)"
  fi
}

def cmd.fast commands "/fast" "Toggles fast mode, or says the model has none"
chk_cmd_fast() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main "/fast on"
  if wait_for main 'FAST:ON' 10; then
    submit main "/fast off"
    res PASS "footer FAST:ON, then off again"
  elif wait_for main '[Ff]ast mode' 5; then
    res PASS "reported for Haiku: $(first_match '[Ff]ast mode')"
  else
    shot
    res FAIL "no fast-mode change or report: $(footer | head -1)"
  fi
}

def cmd.compact commands "/compact" "Summarises the conversation, keeps the session"
chk_cmd_compact() {
  ensure_main || { res FAIL "main session did not start"; return; }
  grep -q 'PARITY_PONG' <<<"$(screen main)" ||
    turn main "Reply with exactly: PARITY_PONG" '^ *PARITY_PONG *$' 60
  submit main "/compact"
  if wait_for main '[Cc]ompacted' 120; then
    res PASS "$(first_match '[Cc]ompacted')"
  else
    shot
    res FAIL "no compaction notice within 120s: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[\|^Loc' | tail -2 | head -1)"
  fi
  wait_idle main 30 || true
}

def cmd.context commands "/context" "Shows the context usage breakdown"
chk_cmd_context() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main "/context"
  if wait_for main 'Built-in|[Cc]ontext [Uu]sage|tokens' 20; then
    shot
    res PASS "breakdown shown: $(first_match '[Cc]ontext [Uu]sage|tokens' || first_match 'Built-in')"
  else
    shot
    res FAIL "no /context output"
  fi
}

# open_view <sess> <command> <ERE>: run a command that opens a fullscreen view
# and wait for it; the caller closes it with close_view.
open_view() {
  keys "$1" Escape
  wait_for "$1" "$EMPTY_COMPOSER" 5 || true
  enter_cmd "$1" "$2"
  wait_for "$1" "$3" 15
}

close_view() {
  keys "$1" Escape
  wait_for "$1" "$EMPTY_COMPOSER" 5 || { keys "$1" Escape; wait_for "$1" "$EMPTY_COMPOSER" 5; }
}

def cmd.usage commands "/usage (stock 2.1.293 has no /cost)" "Plan limits and session cost"
chk_cmd_usage() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /usage 'Current session totals'; then
    res PASS "$(first_match '\$[0-9.]+ cost') | $(first_match '5-hour')"
  else
    shot
    res FAIL "Usage tab did not show session totals"
  fi
  close_view main
}

def cmd.status commands "/status" "Version, session id, account, model"
chk_cmd_status() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /status 'Session ID: [0-9a-f-]{36}'; then
    res PASS "$(first_match 'Session ID:') | $(first_match '^ *Model: ')"
  else
    shot
    res FAIL "Status tab without a session id"
  fi
  close_view main
}

def cmd.help commands "/help" "Shortcuts and commands"
chk_cmd_help() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /help 'Shortcuts +Commands'; then
    res PASS "Help tab: $(first_match 'Shortcuts +Commands')"
  else
    shot
    res FAIL "Help tab did not open"
  fi
  close_view main
}

def cmd.mcp commands "/mcp" "Server list with status"
chk_cmd_mcp() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /mcp 'total [0-9]+ +connected [0-9]+'; then
    local summary
    summary=$(first_match 'total [0-9]+')
    # The fixture's project server must be listed.
    if wait_for main 'parity +connected' 5; then
      res PASS "$summary; project server: $(first_match 'parity +connected')"
    else
      res FAIL "$summary; the fixture's 'parity' server is not listed as connected"
    fi
  else
    shot
    res FAIL "MCP tab did not open"
  fi
  close_view main
}

# settings_note: claude-rs's settings editor refuses a settings.json that is
# not a regular file (this fleet's is a Nix store symlink), where stock reads
# it. The checks below report that as the reason when it applies.
user_settings_kind() {
  if [[ -L $CONFIG_DIR/settings.json ]]; then echo "a symlink"; else echo "a regular file"; fi
}

def cmd.config commands "/config" "Settings UI"
chk_cmd_config() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /config 'General +Memory +Permissions'; then
    res PASS "Settings opened: $(first_match 'General +Memory') | $(first_match 'Save in:')"
  else
    shot
    res FAIL "Settings did not open"
  fi
  close_view main
}

def cmd.memory commands "/memory" "Picks a CLAUDE.md memory file to edit"
chk_cmd_memory() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /memory 'Auto compact|Automatic memory|[Mm]emory file'; then
    if grep -Eq 'CLAUDE\.md|[Mm]emory file' <<<"$SCREEN"; then
      res PASS "memory files offered: $(first_match 'CLAUDE\.md|[Mm]emory file')"
    else
      res FAIL "opens the Memory settings pane ($(first_match 'Automatic memory')), not a memory-file picker"
    fi
  else
    shot
    res FAIL "Memory pane did not open"
  fi
  close_view main
}

def cmd.permissions commands "/permissions" "Lists allow/deny rules from every scope"
chk_cmd_permissions() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local n row
  n=$(jq '.permissions.allow // [] | length' "$CONFIG_DIR/settings.json" 2>/dev/null || echo 0)
  if ! open_view main /permissions 'Allow rules'; then
    shot
    res FAIL "Permissions pane did not open"
    close_view main
    return
  fi
  row=$(first_match 'Allow rules')
  shot
  if ((n > 0)) && grep -q 'Not set here' <<<"$row"; then
    res FAIL "User scope row '$row' although settings.json ($(user_settings_kind)) has $n allow rules; pane note: $(first_match 'Settings path must be|regular file' || echo none)"
  else
    res PASS "pane open; '$row' (settings.json allow rules: $n)"
  fi
  close_view main
}

def cmd.hooks commands "/hooks" "Lists configured hooks"
chk_cmd_hooks() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local n row
  n=$(jq '.hooks // {} | length' "$CONFIG_DIR/settings.json" 2>/dev/null || echo 0)
  if ! open_view main /hooks 'Definitions'; then
    shot
    res FAIL "Hooks pane did not open"
    close_view main
    return
  fi
  row=$(first_match 'Definitions')
  shot
  if ((n > 0)) && grep -q 'Not set here' <<<"$row"; then
    res FAIL "User scope '$row' although settings.json ($(user_settings_kind)) defines $n hook events; pane note: $(first_match 'Settings path must be|regular file' || echo none)"
  else
    res PASS "pane open; '$row' (settings.json hook events: $n)"
  fi
  close_view main
}

def cmd.resume commands "/resume" "Session picker for this project"
chk_cmd_resume() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if open_view main /resume 'Resume Session|Select a session'; then
    res PASS "picker: $(first_match 'Recent sessions|Select a session')"
  else
    shot
    res FAIL "no session picker"
  fi
  # Never pick: Esc only.
  close_view main
}

def cmd.btw commands "/btw <question>" "Side answer that stays out of the conversation"
chk_cmd_btw() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main "/btw What is 2+2? Reply with only the digit."
  if wait_for main 'BTW' 30 && wait_for main '^│ 4 +│$' 30; then
    res PASS "$(first_match 'Claude · BTW') answered 4"
  else
    shot
    res FAIL "no BTW answer card"
  fi
  wait_idle main 30 || true
}

def cmd.init commands "/init" "Writes CLAUDE.md for the project"
chk_cmd_init() {
  local cwd end
  rs_start init --model haiku --permission-mode bypassPermissions || { res FAIL "init session did not start"; return; }
  cwd=$(cwd_of init)
  submit init "/init" || { res FAIL "could not submit /init"; rs_stop init; return; }
  end=$(($(now) + 240))
  while [[ ! -s $cwd/CLAUDE.md ]] && (($(now) < end)); do sleep 2; done
  wait_idle init 120 || true
  if [[ -s $cwd/CLAUDE.md ]]; then
    res PASS "CLAUDE.md written in the scratch cwd: $(head -1 "$cwd/CLAUDE.md")"
  else
    SCREEN=$(screen init)
    shot
    res FAIL "no CLAUDE.md after 240s: $(grep -v '^ *$' <<<"$SCREEN" | grep -v '^\[\|^Loc' | tail -1)"
  fi
  rs_stop init
}

def cmd.rewind commands "/rewind (stock: Esc Esc)" "Restores the conversation to an earlier message"
chk_cmd_rewind() {
  # Its own session: a rewind restarts the session underneath.
  rs_start rewind --model haiku --permission-mode bypassPermissions || { res FAIL "rewind session did not start"; return; }
  if ! turn rewind "Reply with exactly: PARITY_REWIND_ME" '^ *PARITY_REWIND_ME *$' 60; then
    res FAIL "seed turn failed"
    rs_stop rewind
    return
  fi
  local before_footer after_footer
  before_footer=$(footer | head -1)
  type_slow rewind "/rewind "
  if ! wait_for rewind 'Reply with exactly: PARITY_REWIND_ME +\| [0-9a-f-]{36}' 10; then
    shot
    res FAIL "no rewind target offered for the seed message"
    rs_stop rewind
    return
  fi
  keys rewind Tab
  wait_for rewind 'Restore conversation' 5 || true
  keys rewind Down
  sleep 0.3
  keys rewind Enter
  if ! wait_gone rewind '^ *PARITY_REWIND_ME *$' 30; then
    shot
    res FAIL "seed reply still shown after /rewind <uuid> conversation"
    rs_stop rewind
    return
  fi
  wait_for rewind 'Loc: ' 20 || true
  sleep 2
  SCREEN=$(screen rewind)
  after_footer=$(footer | head -1)
  shot
  if [[ $after_footer != "$before_footer" ]]; then
    res FAIL "conversation rewound, but the session came back as '$after_footer' (was '$before_footer'): launch model and mode lost"
  else
    res PASS "conversation rewound; draft restored: $(composer_line); footer kept: $after_footer"
  fi
  # Leave before any turn could run on a changed model.
  rs_stop rewind
}

def cmd.copy commands "/copy" "Copies the last response to the clipboard"
chk_cmd_copy() {
  if ! command -v pbpaste >/dev/null || ! command -v osascript >/dev/null; then
    res SKIP "needs macOS pbcopy/pbpaste to save and restore the clipboard"
    return
  fi
  # Only touch a clipboard that holds plain text (or nothing), and put it back.
  local info nontext saved got
  # "type, size, type, size, ...": keep only the types.
  info=$(osascript -e 'clipboard info' 2>/dev/null)
  nontext=$(awk -F', ' '{for (i = 1; i <= NF; i += 2) print $i}' <<<"$info" |
    grep -v '^$' | grep -Ev '^(«class utf8»|«class ut16»|string|Unicode text|URL)$')
  if [[ -n $nontext ]]; then
    res SKIP "clipboard holds non-text data ($(paste -sd' ' - <<<"$nontext")); not overwriting it"
    return
  fi
  saved=$(pbpaste)
  ensure_main || { res FAIL "main session did not start"; return; }
  turn main "Reply with exactly: PARITY_COPY_ME" '^ *PARITY_COPY_ME *$' 60 || { res FAIL "seed turn failed"; return; }
  submit main "/copy"
  sleep 1
  got=$(pbpaste)
  printf '%s' "$saved" | pbcopy
  if [[ $got == *PARITY_COPY_ME* ]]; then
    res PASS "clipboard held '$got' (restored afterwards)"
  else
    SCREEN=$(screen main)
    shot
    res FAIL "clipboard did not receive the response (got '${got:0:40}'); clipboard restored"
  fi
}

def cmd.clear commands "/clear" "Starts a fresh conversation (new session id)"
chk_cmd_clear() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local cwd before after
  cwd=$(cwd_of main)
  before=$(jq -r .sessionId <<<"$(registry_entry "$cwd")")
  submit main "/clear"
  if wait_registry "$cwd" ".sessionId != \"$before\"" 20; then
    after=$(jq -r .sessionId <<<"$REG")
    note_session "$after"
    if wait_gone main 'PARITY_PONG|PARITY_COPY_ME' 10; then
      res PASS "new session $after (was $before); transcript cleared"
    else
      res FAIL "new session $after but the old transcript is still shown"
    fi
  else
    shot
    res FAIL "session id unchanged after /clear ($before)"
  fi
}

def cmd.exit commands "/exit" "Exits the TUI"
chk_cmd_exit() {
  rs_start exit --model haiku || { res FAIL "exit session did not start"; return; }
  submit exit "/exit"
  sleep 2
  if pane_dead exit; then
    res PASS "/exit closed claude-rs"
  elif wait_for exit 'not yet supported' 5; then
    res GAP "$(first_match 'not yet supported'); Ctrl+Q quits instead"
  else
    shot
    res FAIL "still running, no message"
  fi
  rs_stop exit
}
