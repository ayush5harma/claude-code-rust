# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Gaps the fork already knew about, read from the binary rather than assumed.

# stock_only_reply <command>: what claude-rs answers when the command is typed.
stock_only_reply() {
  ensure_main || return 1
  submit main "$1" || true
  if wait_for main "$(ere_escape "$1") is not yet supported" 5; then
    REPLY_LINE=$(grep -E -- "$(ere_escape "$1") is not yet supported" <<<"$SCREEN" | tail -1)
    return 0
  fi
  REPLY_LINE=$(grep -v '^ *$' <<<"$(screen main)" | grep -v '^\[\|^Loc\|❯' | tail -1)
  return 1
}

def gap.remote_control gaps "Remote Control (/remote-control)" "Shares the session to claude.ai / the app"
chk_gap_remote_control() {
  if stock_only_reply /remote-control; then
    res GAP "$REPLY_LINE"
  else
    res FAIL "no 'not yet supported' reply; last row: $REPLY_LINE"
  fi
}

def gap.diff gaps "/diff panel" "Shows the working-tree diff"
chk_gap_diff() {
  if stock_only_reply /diff; then
    res GAP "$REPLY_LINE"
  else
    res FAIL "no 'not yet supported' reply; last row: $REPLY_LINE"
  fi
}

def gap.vim gaps "Vim editing mode" "Editor mode: vim in /config"
chk_gap_vim() {
  ensure_main || { res FAIL "main session did not start"; return; }
  keys main Escape
  enter_cmd main /config
  if ! wait_for main 'Type to filter|Search' 10; then
    res FAIL "Settings did not open"
    return
  fi
  # "/" focuses the settings search.
  keys main /
  sleep 0.3
  type_slow main "vim"
  sleep 0.5
  SCREEN=$(screen main)
  if grep -Eqi '›? *(editor mode|vim mode)|vim +(On|Off|Default)' <<<"$SCREEN"; then
    res PASS "setting: $(grep -Ei 'editor mode|vim' <<<"$SCREEN" | head -1 | sed -E 's/ {2,}/ … /g')"
  else
    res GAP "no vim / editor-mode setting in /config (search 'vim' matches nothing); no vim bindings in the composer"
  fi
  keys main Escape
  keys main Escape
  wait_for main "$EMPTY_COMPOSER" 5 || keys main Escape
}

# 2026-10-10: claude-rs no longer draws the session colour or opens the agent
# view, since both depended on Claude Code internals the upstream maintainer
# rejected. They are recorded without a probe: an unnamed session has no rule
# to read a colour from, and the hint needs a background session the suite no
# longer starts (it cost a Haiku turn per run).

def gap.session_color gaps "Session colour on the composer rule" "/color paints the prompt bar and the name badge"
chk_gap_session_color() {
  res GAP "/color runs inside Claude Code and its reply shows, but claude-rs draws no session colour"
}

def gap.agents_hint gaps "\`← N agents\` in the footer" "Dim hint while the input is empty, from the background sessions"
chk_gap_agents_hint() {
  res GAP "claude-rs draws no agent hint in the footer; list sessions with \`claude agents\` from a shell"
}

def gap.agent_view gaps "Agent view from ← on an empty prompt" "Opens the agent view (list, peek, attach, dispatch); Esc returns"
chk_gap_agent_view() {
  res GAP "claude-rs has no agent view; run \`claude agents\` from a shell"
}
