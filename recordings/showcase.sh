#!/usr/bin/env bash
# Renders the README screenshots, assets/screenshot-{dark,light}-{welcome,mcp,chat,
# agents,sessions,diff,settings}.png, with VHS. The app runs against
# tests/fixtures/fake-bridge.js in its `showcase` scenario (the scene comes from
# FAKE_BRIDGE_SHOWCASE_SCENE), so the pictures carry neutral demo data and need no
# Claude login.
#
# Needs vhs (with ttyd and ffmpeg), node, git and cargo on PATH. Run from anywhere:
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

# A small fictional Rust CLI as a real Git repository: one commit, then two modified
# files and an untracked one, which the /diff scene shows.
make_repo() {
  local project=$1
  mkdir -p "$project/src"
  cat >"$project/Cargo.toml" <<'EOF'
[package]
name = "tally"
version = "0.3.1"
edition = "2024"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
EOF
  printf 'target/\n' >"$project/.gitignore"
  cat >"$project/src/main.rs" <<'EOF'
use clap::Parser;

#[derive(Parser)]
#[command(name = "tally", about = "Count lines of code")]
struct Cli {
    /// Directory to scan
    path: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let report = tally::scan(&cli.path)?;
    println!("{report}");
    Ok(())
}
EOF
  cat >"$project/src/lib.rs" <<'EOF'
use std::fs;
use std::path::Path;

pub struct Report {
    pub files: usize,
    pub lines: usize,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} files, {} lines", self.files, self.lines)
    }
}

pub fn scan(dir: &Path) -> anyhow::Result<Report> {
    let mut report = Report { files: 0, lines: 0 };
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() {
            report.files += 1;
            report.lines += fs::read_to_string(&path)?.lines().count();
        }
    }
    Ok(report)
}
EOF
  # No global or system Git config, so nothing from the host leaks in.
  local git=(env GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
    GIT_AUTHOR_DATE="2026-10-01T09:00:00Z" GIT_COMMITTER_DATE="2026-10-01T09:00:00Z"
    git -C "$project" -c user.name="Demo Dev" -c user.email="demo@example.com")
  "${git[@]}" init --quiet -b main
  "${git[@]}" add .
  "${git[@]}" commit --quiet -m "Initial scanner"

  # The working-tree changes.
  cat >"$project/src/main.rs" <<'EOF'
use clap::Parser;

#[derive(Parser)]
#[command(name = "tally", about = "Count lines of code")]
struct Cli {
    /// Directory to scan
    path: std::path::PathBuf,

    /// Print every file as it is scanned
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let report = tally::scan(&cli.path, cli.verbose)?;
    println!("{report}");
    Ok(())
}
EOF
  cat >"$project/src/lib.rs" <<'EOF'
use std::fs;
use std::path::Path;

pub struct Report {
    pub files: usize,
    pub lines: usize,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} files, {} lines", self.files, self.lines)
    }
}

pub fn scan(dir: &Path, verbose: bool) -> anyhow::Result<Report> {
    let mut report = Report { files: 0, lines: 0 };
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() {
            if verbose {
                eprintln!("scanning {}", path.display());
            }
            report.files += 1;
            report.lines += fs::read_to_string(&path)?.lines().count();
        }
    }
    Ok(report)
}
EOF
  cat >"$project/src/report.rs" <<'EOF'
//! JSON output for a scan.
use crate::Report;

pub fn as_json(report: &Report) -> String {
    format!("{{\"files\":{},\"lines\":{}}}", report.files, report.lines)
}
EOF
}

# render <appearance> <theme> <scene> <height> <tape steps>
# Records one scene in a fresh app session with a HOME of its own, so the app shows
# the project as ~/demo-project. Every scene but `basic` (welcome and mcp) sets
# FAKE_BRIDGE_SHOWCASE_SCENE and gets the project as a Git repository.
render() {
  local appearance=$1 theme=$2 scene=$3 height=$4 steps=$5
  local home="$work/$appearance-$scene"
  local profile="$home/profile" project="$home/demo-project"
  local bridge_scene=$scene
  [ "$scene" != basic ] || bridge_scene=
  mkdir -p "$profile" "$project"
  [ "$scene" = basic ] || make_repo "$project"
  # Pre-accept the trust dialog for the demo project.
  printf '{"projects":{"%s":{"hasTrustDialogAccepted":true}}}' "$project" >"$profile/.claude.json"

  # No CLAUDE_RS_APPEARANCE here on purpose: the app asks the terminal (xterm.js
  # answers OSC 11), so the pictures also show the detection working.
  cat >"$work/$appearance-$scene.tape" <<TAPE
Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 16
Set Width 1240
Set Height $height
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
Env FAKE_BRIDGE_SHOWCASE_SCENE "$bridge_scene"
Hide
Type "unset NO_COLOR; clear && '$repo/target/debug/claude-rs' --no-update-check --dir '$project'"
Enter
Wait+Screen /Type a message/
Sleep 2.5s
$steps
TAPE
  (cd "$work" && vhs "$work/$appearance-$scene.tape" -o "$work/$appearance-$scene.gif")
}

# shot <appearance> <name>: tape lines that show the screen, take the picture, hide again.
shot() {
  printf 'Show\nSleep 600ms\nScreenshot "%s/assets/screenshot-%s-%s.png"\nSleep 600ms\nHide\n' "$repo" "$1" "$2"
}

record() {
  local appearance=$1 theme=$2

  render "$appearance" "$theme" basic 560 "$(shot "$appearance" welcome)
Type \"/mcp\"
Enter
Wait+Screen /connected/
Sleep 1s
$(shot "$appearance" mcp)"

  render "$appearance" "$theme" chat 1260 "Type \"Add a --verbose flag to the CLI and run the tests\"
Enter
Wait+Screen /Run it with/
Sleep 1.5s
$(shot "$appearance" chat)"

  render "$appearance" "$theme" agents 520 "Type \"/agents\"
Enter
Wait+Screen /Add tests for the scanner/
Sleep 1s
$(shot "$appearance" agents)"

  render "$appearance" "$theme" sessions 700 "Type \"/resume\"
Enter
Wait+Screen /Prepare the 0.4.0 release notes/
Sleep 1s
$(shot "$appearance" sessions)"

  render "$appearance" "$theme" diff 700 "Type \"/diff\"
Enter
Wait+Screen /report.rs/
Sleep 1s
$(shot "$appearance" diff)"

  render "$appearance" "$theme" settings 560 "Type \"/config\"
Enter
Wait+Screen /Language/
Up
Up
Right
Right
Wait+Screen /Allow rules/
Sleep 1s
$(shot "$appearance" settings)"
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
