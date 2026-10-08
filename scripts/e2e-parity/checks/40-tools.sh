# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Tools and permissions, subagents, MCP, hooks, skills and plugins. Prompts
# name tokens indirectly where the reply must not match the prompt's echo.

ensure_perm() { ensure perm --model haiku --permission-mode default; }

# perm_prompt <file>: ask for a write the default mode must confirm.
perm_prompt() {
  ensure_perm || return 1
  submit perm "Run the bash command: touch $1" || return 1
  wait_for perm 'Allow once' 60
}

def tools.perm_allow tools "Permission prompt: allow once" "Prompt in default mode; the call runs once allowed"
chk_tools_perm_allow() {
  local f
  f=$(cwd_of perm)/PARITY_PERM_ALLOW
  if ! perm_prompt PARITY_PERM_ALLOW; then
    shot
    res FAIL "no permission prompt for 'touch' in default mode"
    return
  fi
  local prompt
  prompt=$(first_match 'Allow once')
  keys perm Enter
  local end=$(($(now) + 30))
  while [[ ! -e $f ]] && (($(now) < end)); do sleep 0.5; done
  wait_idle perm 60 || true
  if [[ -e $f ]]; then
    res PASS "prompt '$prompt'; Allow once ran it (file created)"
  else
    shot
    res FAIL "allowed, but the file was not created"
  fi
}

def tools.perm_deny tools "Permission prompt: deny" "Denied call does not run; the model is told"
chk_tools_perm_deny() {
  local f
  f=$(cwd_of perm)/PARITY_PERM_DENY
  if ! perm_prompt PARITY_PERM_DENY; then
    shot
    res FAIL "no permission prompt"
    return
  fi
  keys perm Right Right Right
  if ! wait_for perm '▸ ✗ Deny' 3; then
    shot
    res FAIL "could not focus Deny"
    return
  fi
  keys perm Enter
  wait_idle perm 60 || true
  SCREEN=$(screen perm)
  if [[ ! -e $f ]] && grep -q 'Permission denied\|denied' <<<"$SCREEN"; then
    res PASS "file not created; $(first_match 'Permission denied|denied')"
  else
    shot
    res FAIL "file exists: $([[ -e $f ]] && echo yes || echo no)"
  fi
  rs_stop perm
}

def tools.bash tools "Bash tool" "Command row with its output"
chk_tools_bash() {
  if main_turn "Run the bash command: echo PARITY_BASH_OK" 'Bash echo PARITY_BASH_OK' 60; then
    res PASS "$(first_match 'Bash echo PARITY_BASH_OK') / output: $(first_match '^ +PARITY_BASH_OK' || echo 'collapsed')"
  else
    shot
    res FAIL "no Bash row"
  fi
}

def tools.read tools "Read tool" "Read row; content reaches the model"
chk_tools_read() {
  if main_turn "Use the Read tool to read parity_read.txt, then reply with its first line only." '^ *PARITY_READ_TOKEN_7731 *$' 60; then
    res PASS "$(tool_row 'Read' || echo 'no Read row') -> reply PARITY_READ_TOKEN_7731"
  else
    shot
    res FAIL "no reply with the file's first line"
  fi
}

def tools.edit tools "Edit tool with diff" "Shows a -/+ diff; file changes"
chk_tools_edit() {
  local f
  f=$(cwd_of main)/parity_edit.txt
  main_turn "In parity_edit.txt, use the Edit tool to replace the word OLD_TOKEN with NEW_TOKEN. Then reply EDITED." '^ *EDITED\.? *$' 90
  SCREEN=$(screen main)
  if grep -q NEW_TOKEN "$f" 2>/dev/null; then
    local minus='[0-9]+ +- +value = OLD_TOKEN' plus='[0-9]+ +\+ +value = NEW_TOKEN'
    if grep -Eq "$minus" <<<"$SCREEN" && grep -Eq "$plus" <<<"$SCREEN"; then
      res PASS "file changed; $(tool_row 'Edit') with diff rows '$(first_match "$minus")' / '$(first_match "$plus")'"
    else
      shot
      res FAIL "file changed but no -/+ diff on screen: $(tool_row 'Edit' || echo 'no Edit row')"
    fi
  else
    shot
    res FAIL "parity_edit.txt unchanged"
  fi
}

def tools.write tools "Write tool" "Write row with the new content; file created"
chk_tools_write() {
  local f
  f=$(cwd_of main)/parity_write.txt
  main_turn "Use the Write tool to create parity_write.txt whose only line is PARITY_ followed by WRITE_OK (no space). Then reply WRITTEN." '^ *WRITTEN\.? *$' 90
  SCREEN=$(screen main)
  if grep -q PARITY_WRITE_OK "$f" 2>/dev/null; then
    if grep -q 'Write' <<<"$SCREEN" && grep -q 'PARITY_WRITE_OK' <<<"$SCREEN"; then
      res PASS "file created; $(tool_row 'Write' || echo 'no Write row')"
    else
      shot
      res FAIL "file created but the Write row/content is not shown"
    fi
  else
    shot
    res FAIL "parity_write.txt not created"
  fi
}

def tools.grep_glob tools "Glob and Grep tools" "Search rows with results"
chk_tools_grep_glob() {
  if main_turn "Use the Glob tool to list the files matching parity_*.txt, and the Grep tool to find which file in this directory (any name) contains GREP_TOKEN followed by _4410. Reply with only that file's name." '^ *README\.md *$' 90; then
    local g1 g2
    # Stock 2.1.29x may run without Glob/Grep ("find via the Bash tool
    # instead"); that is the stock child's tool set, not a claude-rs gap.
    if grep -q 'No such tool available: \(Glob\|Grep\)' <<<"$SCREEN"; then
      res SKIP "the stock child has no Glob/Grep tool in this configuration: $(first_match 'No such tool available'); README.md found via Bash"
      return
    fi
    g1=$(tool_row 'Glob' || true)
    g2=$(tool_row 'Grep|Search' || true)
    if [[ -n $g1 && -n $g2 ]]; then
      res PASS "rows: '$g1' / '$g2'; answer README.md"
    else
      shot
      res FAIL "answer README.md but rows missing: glob='$g1' grep='$g2'"
    fi
  else
    shot
    res FAIL "no README.md answer"
  fi
}

def tools.todo tools "Todo / task list (TodoWrite or TaskCreate)" "Tasks shown as a checklist"
chk_tools_todo() {
  if main_turn "Use your todo list tool (TodoWrite, or TaskCreate if that is what you have) to record exactly two pending items named PARITY_TODO_ plus ONE and PARITY_TODO_ plus TWO, without spaces. Do nothing else, then reply LISTED." '^ *LISTED\.? *$' 90; then
    if grep -Eq '(□|☐|◻|\[ \]|Create task).*PARITY_TODO_ONE' <<<"$SCREEN"; then
      res PASS "$(first_match 'PARITY_TODO_ONE')"
    else
      shot
      res FAIL "no task rows for PARITY_TODO_ONE"
    fi
  else
    shot
    res FAIL "todo turn did not finish"
  fi
}

# Run after the todo and tool rows exist, so a toggle has something to show.
def input.ctrl_t input "Ctrl+T toggles the task list" "Shows/hides the todo list"
chk_input_ctrl_t() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if same_screen_after main C-t; then
    res GAP "screen unchanged after Ctrl+T (task rows are only inline in the transcript)"
  else
    res PASS "toggled: $DIFF_LINE"
    keys main C-t
  fi
}

def input.ctrl_o input "Ctrl+O toggles the detailed transcript" "Expands tool output / transcript view"
chk_input_ctrl_o() {
  ensure_main || { res FAIL "main session did not start"; return; }
  if same_screen_after main C-o; then
    res GAP "screen unchanged after Ctrl+O with tool rows on screen"
  else
    res PASS "toggled: $DIFF_LINE"
    keys main C-o
  fi
}

def tools.ask tools "AskUserQuestion" "Question with options; the answer returns to the model"
chk_tools_ask() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main 'Use the AskUserQuestion tool to ask me one question, "Which colour?", with exactly two options labelled PARITY_RED and PARITY_BLUE. After I answer, reply with only the label I picked.' ||
    { res FAIL "could not submit"; return; }
  if ! wait_for main '▸ PARITY_RED' 60; then
    shot
    res FAIL "no question widget with PARITY_RED focused"
    return
  fi
  local widget
  widget=$(first_match '▸ PARITY_RED')
  keys main Enter
  if wait_for main '^ *PARITY_RED *$' 60; then
    wait_idle main 30 || true
    res PASS "widget '$widget'; answer returned and echoed: PARITY_RED"
  else
    shot
    res FAIL "answered, but the model did not get it"
  fi
}

def tools.plan tools "Plan mode and plan approval" "Plan shown for approval; work starts once approved"
chk_tools_plan() {
  local f
  ensure plan --model haiku --permission-mode plan || { res FAIL "plan session did not start"; return; }
  f=$(cwd_of plan)/parity_plan.txt
  submit plan "Plan how to create a file named parity_plan.txt containing PLAN_OK. Keep the plan to one line and present it with ExitPlanMode right away." ||
    { res FAIL "could not submit"; return; }
  # Plan mode asks first to write the plan file, then to approve the plan
  # (ExitPlanMode). Accept each prompt's first option until the file exists.
  local end exit_ui="" prompts=() model row
  end=$(($(now) + 300))
  while [[ ! -e $f ]] && (($(now) < end)); do
    if wait_for plan '▸ [✓✗]' 5; then
      row=$(first_match '▸ [✓✗]')
      prompts+=("${row#*▸ }")
      # The plan approval is the prompt whose first option is Approve.
      if [[ $row == *Approve* ]]; then
        exit_ui=$(paste -sd' ' - <<<"$(grep -E '▸ ✓ Approve|✗ Reject' <<<"$SCREEN" | sed -E 's/^[│ ]+//')")
        shot approval
      fi
      keys plan Enter
      wait_gone plan "$(ere_escape "$row")" 15 || true
    fi
  done
  wait_idle plan 60 || true
  model=$(footer "$(screen plan)" | head -1)
  if [[ -e $f && -n $exit_ui ]]; then
    res PASS "ExitPlanMode approval '$exit_ui'; approved, then parity_plan.txt was written (prompts: ${#prompts[@]}; footer $model)"
  elif [[ -e $f ]]; then
    res FAIL "file written but no plan approval was shown (prompts: ${prompts[*]:-none})"
  else
    SCREEN=$(screen plan)
    shot
    res FAIL "parity_plan.txt not written in 300s (prompts answered: ${#prompts[@]}; plan approval: ${exit_ui:-none})"
  fi
  rs_stop plan
}

def tools.subagent tools "Subagent (Agent tool)" "Agent row; the result comes back"
chk_tools_subagent() {
  if main_turn 'Use the Agent tool once with subagent_type general-purpose and model haiku, with this prompt: "Reply with exactly PARITY_SUB followed by _OK, no space. Use no tools." Then reply with exactly what it returned.' '^ *PARITY_SUB_OK *$' 120; then
    res PASS "$(tool_row 'Agent|Task' || echo 'no agent row') -> PARITY_SUB_OK"
  else
    shot
    res FAIL "no subagent result"
  fi
}

def tools.background_agent tools "Background subagent" "Runs in the background; result reported later"
chk_tools_background_agent() {
  ensure_main || { res FAIL "main session did not start"; return; }
  submit main 'Use the Agent tool once with subagent_type general-purpose, model haiku, run_in_background true, and this prompt: "Reply with exactly PARITY_BG followed by _OK, no space. Use no tools." Do not wait for it. Reply STARTED.' ||
    { res FAIL "could not submit"; return; }
  if ! wait_for main '^ +[✓✗↗⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] .*backgrounded' 60; then
    shot
    res FAIL "no background agent row"
    return
  fi
  local row
  row=$(tool_row 'backgrounded|[Bb]ackground')
  if wait_for main '─ PARITY_BG_OK|^ *PARITY_BG_OK' 120; then
    wait_idle main 60 || true
    res PASS "'$row'; result delivered: PARITY_BG_OK"
  else
    shot
    res FAIL "'$row' but no result within 120s"
  fi
}

def tools.mcp tools "MCP tool call" "Calls a project MCP server's tool"
chk_tools_mcp() {
  if main_turn "Call the parity_echo tool from the parity MCP server with text PARITY_MCP_IN, then reply with its output only." 'PARITY_MCP_ECHO:PARITY_MCP_IN' 90; then
    res PASS "$(tool_row '[Pp]arity' || echo 'no tool row') -> $(first_match 'PARITY_MCP_ECHO:PARITY_MCP_IN')"
  else
    shot
    res FAIL "no PARITY_MCP_ECHO output"
  fi
}

def tools.hook_start tools "SessionStart hook" "Project hook runs at start"
chk_tools_hook_start() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local f
  f=$(cwd_of main)/.parity/session-start
  if [[ -s $f ]]; then
    res PASS "project SessionStart hook wrote .parity/session-start at $(cat "$f")"
  else
    res FAIL "no .parity/session-start in the main cwd"
  fi
}

def tools.hook_end tools "SessionEnd hook" "Project hook runs at exit (the owner's archive hook rides on it)"
chk_tools_hook_end() {
  rs_start hooks --model haiku || { res FAIL "hooks session did not start"; return; }
  local f end
  f=$(cwd_of hooks)/.parity/session-end
  keys hooks C-q
  end=$(($(now) + 15))
  while [[ ! -s $f ]] && (($(now) < end)); do sleep 0.5; done
  if [[ -s $f ]]; then
    res PASS "Ctrl+Q ran the project SessionEnd hook (.parity/session-end at $(cat "$f"))"
  else
    res FAIL "no .parity/session-end 15s after Ctrl+Q"
  fi
  tm kill-session -t hooks 2>/dev/null
}

def tools.skill tools "Skill as a slash command" "Project skill runs as /<name>"
chk_tools_skill() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local listed=no
  sdk_command_names main | grep -qx parity-skill && listed=yes
  if main_turn "/parity-skill" '^ *PARITY_SKILL_OK *$' 60; then
    res PASS "advertised: $listed; /parity-skill answered PARITY_SKILL_OK"
  else
    shot
    res FAIL "advertised: $listed; no PARITY_SKILL_OK reply"
  fi
}

def tools.plugin tools "Plugin commands" "Installed plugins' commands in the slash menu"
chk_tools_plugin() {
  ensure_main || { res FAIL "main session did not start"; return; }
  local names n example
  names=$(sdk_command_names main | grep ':' || true)
  n=$(grep -c . <<<"$names")
  if ((n == 0)); then
    res SKIP "no plugin commands installed for this identity"
    return
  fi
  example=$(head -1 <<<"$names")
  clear_draft main
  type_slow main "/${example%%:*}:"
  if wait_for main "/${example%%:*}:" 5 && grep -Eq "> /${example%%:*}:|/${example%%:*}:[a-z]" <<<"$(screen main | grep -v '❯')"; then
    res PASS "$n plugin commands advertised; menu lists e.g. $(first_match "/${example%%:*}:[a-z-]+" | grep -Eo "/${example%%:*}:[a-z-]+" | head -1) (not run: each is a full prompt)"
  else
    res FAIL "$n advertised but the menu does not offer /${example%%:*}:"
  fi
  keys main Escape
  clear_draft main
}
