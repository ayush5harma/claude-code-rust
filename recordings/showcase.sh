#!/usr/bin/env bash
# Renders the README screenshots, assets/screenshot-{dark,light}-{welcome,mcp}.png,
# with VHS. The app runs against tests/fixtures/fake-bridge.js in its `showcase`
# scenario, so the pictures carry neutral demo data and need no Claude login.
#
# Needs vhs (with ttyd and ffmpeg), node and cargo on PATH. Run from anywhere:
#   recordings/showcase.sh            # both appearances
#   recordings/showcase.sh light      # one
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build --quiet --manifest-path "$repo/Cargo.toml"

# Apple's dark and light window backgrounds, with ANSI colors close to Terminal.app's.
dark_theme='{"name":"Apple Dark","background":"#1c1c1e","foreground":"#f2f2f7","cursor":"#f2f2f7","selection":"#3a3a3c","black":"#1c1c1e","red":"#ff6165","green":"#30d158","yellow":"#ffd600","blue":"#5cb8ff","magenta":"#ea8dff","cyan":"#3cd3fe","white":"#d1d1d6","brightBlack":"#636366","brightRed":"#ff6165","brightGreen":"#30d158","brightYellow":"#ffd600","brightBlue":"#5cb8ff","brightMagenta":"#ea8dff","brightCyan":"#3cd3fe","brightWhite":"#ffffff"}'
light_theme='{"name":"Apple Light","background":"#ffffff","foreground":"#1c1c1e","cursor":"#1c1c1e","selection":"#d1d1d6","black":"#1c1c1e","red":"#d70028","green":"#00802d","yellow":"#966200","blue":"#0066d6","magenta":"#b02fc2","cyan":"#0076a3","white":"#8e8e93","brightBlack":"#636366","brightRed":"#d70028","brightGreen":"#00802d","brightYellow":"#966200","brightBlue":"#0066d6","brightMagenta":"#b02fc2","brightCyan":"#0076a3","brightWhite":"#1c1c1e"}'

record() {
  local appearance=$1 theme=$2
  # A HOME of its own, so the app shows the project as ~/demo-project.
  local home="$work/$appearance"
  local profile="$home/profile" project="$home/demo-project"
  mkdir -p "$profile" "$project"
  # Pre-accept the trust dialog for the demo project.
  printf '{"projects":{"%s":{"hasTrustDialogAccepted":true}}}' "$project" >"$profile/.claude.json"

  # No CLAUDE_RS_APPEARANCE here on purpose: the app asks the terminal (xterm.js
  # answers OSC 11), so the pictures also show the detection working.
  cat >"$work/$appearance.tape" <<TAPE
Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 16
Set Width 1240
Set Height 560
Set Padding 18
Set WindowBar Colorful
Set BorderRadius 10
Set Margin 0
Set CursorBlink false
Set WaitTimeout 30s
Set Theme $theme
Env TERM "xterm-256color"
Env COLORTERM "truecolor"
Env HOME "$home"
Env CLAUDE_CONFIG_DIR "$profile"
Env CLAUDE_RS_AGENT_BRIDGE "$repo/tests/fixtures/fake-bridge.js"
Env CLAUDE_RS_AGENT_BRIDGE_RUNTIME "$(command -v node)"
Env FAKE_BRIDGE_SCENARIO "showcase"
Hide
Type "unset NO_COLOR; clear && '$repo/target/debug/claude-rs' --no-update-check --dir '$project'"
Enter
Wait+Screen /Type a message/
Sleep 2.5s
Show
Sleep 600ms
Screenshot "$repo/assets/screenshot-$appearance-welcome.png"
Hide
Type "/mcp"
Enter
Wait+Screen /connected/
Sleep 1s
Show
Sleep 600ms
Screenshot "$repo/assets/screenshot-$appearance-mcp.png"
Sleep 300ms
TAPE
  (cd "$work" && vhs "$work/$appearance.tape" -o "$work/$appearance.gif")
}

appearances=("$@")
[ ${#appearances[@]} -gt 0 ] || appearances=(dark light)
for appearance in "${appearances[@]}"; do
  case $appearance in
    dark) record dark "$dark_theme" ;;
    light) record light "$light_theme" ;;
    *) echo "unknown appearance: $appearance" >&2; exit 2 ;;
  esac
done
