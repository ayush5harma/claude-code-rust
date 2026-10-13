# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# Presentation capture, not a pass/fail comparison: the same prompts go to
# claude-rs and to the stock Claude Code TUI, and both screens are kept, plain
# and with colours (capture-pane -e), under WORK/presentation. The first
# prompt makes the model reproduce a fixed Markdown sample; the second runs one
# command that prints ANSI colours. docs/src/presentation-gaps.md is written
# from these files by a person, not generated.

PRESENT_MD_PROMPT='Read presentation-sample.md with the Read tool, then output its full contents exactly as your reply, as raw Markdown with nothing before or after it.'
PRESENT_ANSI_PROMPT='Run the bash command: bash ansi.sh   and show me its output.'

# present_save <sess> <name>: the screen as text and with colour escapes.
present_save() {
  local dir=$WORK/presentation
  screen "$1" >"$dir/$2.txt"
  screen_e "$1" >"$dir/$2.ansi"
}

def present.capture fork "Presentation capture, stock and claude-rs" "Same Markdown reply and ANSI command; both screens kept"
chk_present_capture() {
  local dir=$WORK/presentation cwd rows_rs rows_stock
  mkdir -p "$dir"
  prepare_cwd present || { res FAIL "could not prepare the cwd"; return; }
  cwd=$(cwd_of present)
  cp "$SUITE_DIR/fixtures/presentation-sample.md" "$cwd/" || { res FAIL "could not copy the sample"; return; }
  cat >"$cwd/ansi.sh" <<'EOF'
#!/usr/bin/env bash
# Fixed ANSI sample for the presentation capture.
printf '\033[31mANSI_RED\033[0m \033[1;32mANSI_BOLD_GREEN\033[0m \033[4mANSI_UNDERLINE\033[0m\n'
printf '\033[38;5;208mANSI_256_ORANGE\033[0m \033[38;2;10;200;250mANSI_TRUECOLOR\033[0m \033[7mANSI_REVERSE\033[0m\n'
printf 'plain ANSI_PLAIN_LINE\n'
EOF
  # A tall pane so a whole reply fits on one screen.
  local COLS=130 ROWS=110
  rs_start present --model "$MODEL" --permission-mode bypassPermissions || { res FAIL "claude-rs did not start"; return; }
  submit present "$PRESENT_MD_PROMPT" || { res FAIL "claude-rs: could not submit"; return; }
  wait_for present 'Elapsed [0-9.]+s' 120 || true
  wait_idle present 30 || true
  present_save present claude-rs.markdown
  submit present "$PRESENT_ANSI_PROMPT" || true
  wait_for present 'ANSI_PLAIN_LINE' 120 || true
  wait_idle present 60 || true
  present_save present claude-rs.ansi
  rs_stop present

  session_exists presstock && tm kill-session -t presstock
  tm new-session -d -s presstock -x "$COLS" -y "$ROWS" -c "$cwd" env ${CONFIG_ENV[@]+"${CONFIG_ENV[@]}"} "$CLAUDE_BIN" --model "$MODEL"
  # Stock asks about workspace trust once per directory; the directory is the
  # suite's own scratch copy.
  wait_for presstock '^❯|trust' 40 || true
  if grep -Eqi 'trust (this|the) (folder|files)|Yes, I trust' <<<"$SCREEN"; then
    keys presstock Enter
    wait_for presstock '^❯' 30 || true
  fi
  type_text_raw presstock "$PRESENT_MD_PROMPT"
  keys presstock Enter
  wait_for presstock '✻ .* for [0-9]+s' 120 || true
  sleep 1
  present_save presstock stock.markdown
  type_text_raw presstock "$PRESENT_ANSI_PROMPT"
  keys presstock Enter
  wait_for presstock 'ANSI_PLAIN_LINE' 120 && wait_for presstock '✻ .* for [0-9]+s' 60 || true
  sleep 1
  present_save presstock stock.ansi
  # Stock keeps the full tool output behind Ctrl+O.
  keys presstock C-o
  sleep 1.5
  present_save presstock stock.ansi-expanded
  keys presstock C-o
  keys presstock C-c
  sleep 0.5
  keys presstock C-c
  sleep 1
  tm kill-session -t presstock 2>/dev/null
  CUR_SHEBANG=$(grep -B2 -A2 '^\*\*!/usr/bin/env' "$dir/claude-rs.markdown.txt" | head -6)
  if [[ -n $CUR_SHEBANG ]] && grep -q '#!/usr/bin/env bash' "$dir/stock.markdown.txt"; then
    defect "A shebang first line in a fenced bash block is rendered as bold text without its #" \
      "1. Have the model answer with a fenced bash block whose first line is #!/usr/bin/env bash. 2. Look at the rendered block in claude-rs: the line reads **!/usr/bin/env bash** (the # is gone, literal asterisks appear, a blank row is inserted); stock shows #!/usr/bin/env bash." \
      "$CUR_SHEBANG"
  fi
  rows_rs=$(grep -vc '^ *$' "$dir/claude-rs.markdown.txt")
  rows_stock=$(grep -vc '^ *$' "$dir/stock.markdown.txt")
  local f missing=""
  for f in claude-rs.markdown stock.markdown; do
    grep -q 'PRES long line' "$dir/$f.txt" || missing+=" $f(PRES long line)"
  done
  # The command's output must be on both screens, or the capture claims too much
  # (stock can sit on a permission prompt instead).
  for f in claude-rs.ansi stock.ansi; do
    grep -q 'ANSI_PLAIN_LINE' "$dir/$f.txt" || missing+=" $f(ANSI_PLAIN_LINE)"
  done
  if [[ -z $missing ]]; then
    res PASS "both replies captured: $rows_rs non-blank rows (claude-rs), $rows_stock (stock); the ANSI command's output is on both screens; files: $dir"
  else
    SCREEN=$(<"$dir/stock.ansi.txt")
    shot
    res FAIL "capture incomplete, missing:$missing"
  fi
}
