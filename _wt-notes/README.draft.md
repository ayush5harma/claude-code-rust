<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/mascot-dark.svg">
    <img src="assets/mascot-light.svg" alt="The claude-rs crab: an orange pixel-art crab with raised claws" width="216">
  </picture>
</p>

<h1 align="center">Claude Code Rust</h1>

<p align="center">
  A native Rust terminal interface for Claude Code.<br>
  Ayush's fork of <a href="https://github.com/srothgan/claude-code-rust">srothgan/claude-code-rust</a>, synced with upstream 0.16.1.
</p>

<p align="center">
  <a href="https://github.com/ayush5harma/claude-code-rust/releases/latest"><img src="https://img.shields.io/github/v/release/ayush5harma/claude-code-rust?label=version" alt="Version"></a>
  <a href="https://github.com/ayush5harma/claude-code-rust/actions/workflows/pr.yml"><img src="https://github.com/ayush5harma/claude-code-rust/actions/workflows/pr.yml/badge.svg" alt="CI"></a>
  <a href="docs/src/about.md"><img src="https://img.shields.io/badge/docs-manual-blue" alt="Manual"></a>
  <a href="https://www.apache.org/licenses/LICENSE-2.0"><img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="License"></a>
</p>

> [!NOTE]
> **This is a fork.** The original project is [Claude Code Rust by Simon Rothgang](https://github.com/srothgan/claude-code-rust). This fork follows upstream (currently synced to upstream `main` at `246757b`, release 0.16.1) and adds the changes under [What this fork adds](#what-this-fork-adds). Fork releases come from [this repository's releases](https://github.com/ayush5harma/claude-code-rust/releases); `npm install -g claude-code-rust` and the upstream install scripts install the upstream build, not this fork. Report fork-specific problems [here](https://github.com/ayush5harma/claude-code-rust/issues) and upstream problems [upstream](https://github.com/srothgan/claude-code-rust/issues).

## Contents

- [Screenshots](#screenshots)
- [What it is](#what-it-is)
- [How it compares to Claude Code's own interface](#how-it-compares-to-claude-codes-own-interface)
- [Benchmarks](#benchmarks)
- [Features](#features)
- [Slash commands](#slash-commands)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Command line](#command-line)
- [Install](#install)
- [What this fork adds](#what-this-fork-adds)
- [Appearance](#appearance)
- [Documentation](#documentation) · [Development](#development) · [Status and limitations](#status-and-limitations) · [License](#license)

## Screenshots

{{SCREENSHOTS}}

<p align="center"><sub>GitHub shows the dark or the light pictures to match your own theme. They are drawn from demo data by <code>recordings/showcase.sh</code>, so they need no login.</sub></p>

## What it is

Claude Code Rust (`claude-rs`) replaces the Claude Code terminal interface with a native Rust binary built on [Ratatui](https://ratatui.rs/) and Crossterm. Sessions run through Anthropic's public [Agent SDK](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/agent-sdk) via a local bridge, with your existing Claude account, models, tools, file edits, permissions, MCP servers, skills, hooks and session history. Only the interface is different.

```text
┌──────────────────────┐  NDJSON   ┌───────────────────────┐  Agent SDK  ┌───────────────────────┐
│ claude-rs            │◀── stdio ─▶│ bridge (TypeScript)   │◀───────────▶│ Claude Code engine    │
│ Rust UI: Ratatui,    │           │ on a private Bun      │             │ the `claude` binary,  │
│ Crossterm, syntect   │           │ runtime               │             │ tools, MCP, hooks     │
└──────────────────────┘           └───────────────────────┘             └───────────────────────┘
   everything you see                 translates SDK messages               does the actual work
```

Finished output is written into the terminal's own scrollback, so the conversation stays scrollable, searchable and selectable with your terminal's tools; only the live area (the streaming reply, the composer and the footer) is redrawn, by diffing cells. Larger tools such as the Git diff viewer, the agents view, the session picker and the settings editor open full screen and return to the chat.

## How it compares to Claude Code's own interface

claude-rs does not replace Claude Code's agent: it starts the same `claude` engine underneath, so model behaviour, tools, permissions and billing are the same. What changes is the process that draws the terminal.

| | Claude Code | claude-rs |
| --- | --- | --- |
| Interface process | Part of the `claude` process (JavaScript, React Ink) | A separate native binary (Rust, Ratatui) |
| Agent engine | Built in | The same `claude` binary, started by the Agent SDK |
| Extra processes | None | The Bun bridge |
| Account, billing, tools, MCP, hooks, skills | Claude Code's | The same, through the Agent SDK |

{{COMPARE_SUMMARY}}

## Benchmarks

{{BENCHMARKS}}

## Features

- **Native scrollback.** Finished messages, tool calls and diffs go into the terminal's own scrollback; resizing reflows the live area without losing history.
- **Markdown and code.** Headings, nested lists, tables, quotes and shortened links; fenced code is highlighted by syntect with the Oniguruma engine, in shaded blocks, with light themes in a light terminal.
- **Tool calls you can read.** Numbered Read previews, coloured Edit/Write diffs, shell output with its colours (other control sequences are dropped), and `Ctrl+O` for the full detail.
- **Git diff viewer.** `/diff` opens a full-screen viewer with a file tree, HEAD / staged / unstaged comparisons, full-file mode and hunk jumping.
- **Subagents.** `/agents` (or `Left` on an empty prompt) lists every subagent with live status, model, usage and recent tools, reads saved transcripts and stops a running one. While Claude delegates, a shelf above the footer shows each agent's activity and elapsed time.
- **Sessions.** `/resume` is a searchable full-screen picker with saved sizes, tags, confirmed deletion and compact-first resume for large sessions; `claude-rs resume <title>` resumes by exact title.
- **A footer you can arrange.** One row with up to twelve fields (permission mode, model, effort, fast mode, running agents, session name, directory, branch, context left, 5-hour and 7-day usage, finished agents), each with its own side, order and colour, plus the latest plan and your `statusLine` command.
- **Settings and integrations.** `/config` edits user, project and local settings in a tabbed form or as JSON, including permissions, sandbox and hooks; `/mcp` shows server status and runs authorization; `/plugins` manages plugins and marketplaces.
- **Search, export and copy.** `/search` or `Ctrl+F` searches the retained transcript; `/export` writes Markdown without overwriting a file; `/copy` copies a response or one of its code blocks.
- **Composer.** Multi-line editing with readline keys, undo and redo, paste-safe input, `Ctrl+G` to edit the draft in `$EDITOR`, `Esc Esc` to rewind, `Shift`+arrows for permission mode and effort.
- **Hook visibility.** Slow and failed hooks leave a notice, and `/status` shows startup milestones and the hooks run this session.
- **Accessible palette.** Apple's system colours with a tested 4.5:1 contrast floor, in a dark and a light variant chosen from your terminal's background.
- **Diagnostics.** `claude-rs doctor`, redacted log bundles (`claude-rs logs --bundle`), `claude-rs config show`, shell completions and man pages.

## Slash commands

claude-rs handles these itself; anything else (`/clear`, `/context`, `/rename`, `/color` and the other commands the session advertises) goes to Claude Code, and `/docs commands` lists the live set.

| Command | What it does |
| --- | --- |
| `/agents` | Full-screen subagents view: live status, saved transcripts, `s` stops one. |
| `/diff` | Full-screen Git viewer: file tree, HEAD / staged / unstaged, full file, hunk jumps. |
| `/resume` | Searchable session picker with sizes, tags, delete and compact-first resume. |
| `/config` | Settings editor (General, Memory, Permissions, Sandbox, Hooks, Workflows & Worktrees). |
| `/permissions`, `/sandbox`, `/hooks`, `/memory` | Open that settings pane directly. |
| `/mcp`, `/plugins` | MCP status and authorization; plugins, marketplaces and local plugin folders. |
| `/status`, `/usage`, `/help` | Session and account status, quota, built-in help. |
| `/search [query]` | Search the retained transcript (also `Ctrl+F`). |
| `/export [path]` | Save the conversation as Markdown; never overwrites. |
| `/copy` | Copy the last response, or pick one of its code blocks. |
| `/btw <question>` | Ask a side question without adding it to the conversation. |
| `/model <id>`, `/mode <id>`, `/agent <name>` | Switch model, permission mode or main-thread agent. |
| `/effort <level>`, `/thinking <on\|off>`, `/fast [on\|off]`, `/ultracode <on\|off>` | Session-scoped reasoning and speed settings. |
| `/compact`, `/rewind`, `/new-session`, `/cancel` | Compact context, restore an earlier point, start fresh, cancel the turn. |
| `/reload` | Re-read skills and output styles from disk. |
| `/login`, `/logout`, `/exit` | Account and quit. |
| `/docs <topic>` | Help in the chat: modes, models, shortcuts, commands, agents. |

The full list is in [Slash commands](docs/src/commands.md).

## Keyboard shortcuts

| Keys | Action |
| --- | --- |
| `Enter` / `Shift+Enter` (or `\` then `Enter`) | Send / new line |
| `Esc` / `Esc Esc` | Cancel the turn / rewind to an earlier message |
| `Shift+Tab`, `Shift+←/→` / `Shift+↑/↓` | Permission mode / effort |
| `Ctrl+G` | Edit the draft in `$VISUAL` or `$EDITOR` |
| `↑` / `←` on an empty prompt | Recall the last message / open `/agents` |
| `Page Up` / `Page Down`, `Ctrl+End` | Read earlier output, jump back to the latest |
| `Ctrl+F` / `Ctrl+O` / `Ctrl+T` | Search / detailed transcript / plan, agents and tasks |
| `Ctrl+C` / `Ctrl+D` / `Ctrl+Q` | Clear the draft or quit / delete forward or quit / quit |

Every view's keys, and how to remap them, are in [Keyboard shortcuts](docs/src/shortcuts.md).

## Command line

```bash
claude-rs                                        # new session in the current folder
claude-rs "Explain this project"                 # start with a prompt
claude-rs --model opus --effort high --permission-mode plan "Review this project"
claude-rs --continue                             # most recent session here
claude-rs resume "release notes"                 # resume by exact title, or open the picker
claude-rs --continue --fork-session --name "Experiment"
claude-rs --add-dir ../shared --strict-mcp-config --mcp-config ./mcp.json
claude-rs doctor                                 # environment and install checks
```

Flags cover sessions (`-r/--resume`, `-c/--continue`, `-n/--name`, `--fork-session`), the model (`--model`, `--effort`, `--permission-mode`, `--agent`), configuration (`--add-dir`, `--settings`, `--mcp-config`, `--strict-mcp-config`, `--system-prompt[-file]`, `--append-system-prompt[-file]`, `-C/--dir`) and logging (`--enable-logs`, `--diagnostics-preset`, `--log-file`, `--log-filter`). Subcommands: `resume`, `doctor`, `logs`, `config`, `completions`. See [CLI commands and options](docs/src/cli.md).

## Install

Install the [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) first and keep `claude` on `PATH`: claude-rs runs it as its engine, and a few features still go through it ([details](docs/src/about.md)).

### This fork

Download the archive for your platform from [this fork's latest release](https://github.com/ayush5harma/claude-code-rust/releases/latest) and check it against the release's `SHA256SUMS`. Keep the extracted layout whole: it includes the bridge and its Bun runtime.

To run unreleased changes, clone `https://github.com/ayush5harma/claude-code-rust.git` and follow [Build and run locally](docs/src/development.md).

### Upstream

These install Simon Rothgang's distribution, not this fork:

```bash
# macOS and Linux
curl -fsSL https://raw.githubusercontent.com/srothgan/claude-code-rust/main/scripts/install/install.sh | sh
```

```powershell
# Windows PowerShell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm 'https://raw.githubusercontent.com/srothgan/claude-code-rust/main/scripts/install/install.ps1' | iex"
```

```bash
# Node.js 24+
npm install -g claude-code-rust
```

> **Agent SDK billing is unchanged.** Anthropic has paused the previously announced Agent SDK credit change. Agent SDK usage, including `claude -p` and third-party apps like this one, still draws from your normal Claude subscription limits. See [Use the Claude Agent SDK with your Claude plan](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan).

## What this fork adds

Several fork features were contributed upstream and shipped in 0.16.0 (`/agents`, `/export`, `/reload`, launch flags, live session names); the fork now runs upstream's versions of them. What remains fork-only:

| Change | What it does |
| --- | --- |
| Pixel crab | The welcome crab is pixel art drawn in block elements. It scuttles while connecting, snaps its claws once when it connects, and stays still at idle or under `prefersReducedMotion`. |
| Accessible palette | One semantic palette after Apple's [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/color) instead of named ANSI colours, which terminal themes remap; every text colour reaches 4.5:1 contrast, enforced by tests. Light and dark variants follow the terminal ([Appearance](#appearance)). |
| Footer facts | Footer fields for the session name, 5-hour and 7-day usage, and finished agents; an agent shelf with each running agent's model, activity, usage and elapsed time; the latest plan; your `statusLine` command. |
| Transcript search | `/search` and `Ctrl+F` over retained user, assistant and visible tool text, with Unicode-aware matching. |
| Session management | Tags, confirmed deletion, saved sizes and compact-first resume in the picker; resume by exact title with ambiguity reported; the saved title is kept as the process title on resume. |
| Session accents and composer | A stable accent colour per session (`/color` applies once the SDK confirms it); a `filled` or `rules` composer. |
| Input | `\` then `Enter` for a new line, `Ctrl+G` external editor, `Esc Esc` rewind; rewind keeps the model, mode and effort. |
| Hook visibility | Slow-hook notices, per-hook timing, startup milestones and the hooks run this session in `/status`; hook output is sanitized and redacted before it is kept. |
| Command line | Help with examples and exit statuses, a guard against subcommand typos, exit 25 without a terminal, `--log-filter` checked at parse time. |
| Tooling | A live end-to-end parity suite against stock Claude Code ([`scripts/e2e-parity`](scripts/e2e-parity/README.md)), a render benchmark between revisions, the [CPU and memory benchmark](scripts/bench/README.md), and fork releases with platform archives and SHA-256 checksums. |

The [changelog](CHANGELOG.md) has every fork release and the details of each upstream sync.

### Not yet available

A stock-style memory-file picker, a native Remote Control start/stop/share flow, `!` shell input mode, Vim keymaps and general MCP resource attachments wait on operations the public Agent SDK does not expose yet. `/memory` opens the Memory settings instead.

## Appearance

The palette has a dark and a light variant. At startup, claude-rs picks one in this order:

1. `CLAUDE_RS_APPEARANCE=light` or `dark`, if set.
2. The background color the terminal reports (an OSC 11 query, answered by Ghostty, iTerm2, kitty, WezTerm, Terminal.app and most others).
3. Claude Code's own `theme` setting, if the terminal does not answer and the theme is a light or dark one (`light`, `dark-daltonized` and so on).
4. Dark.

The colors are Apple's system colors from the [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/color). Where the default variant fell short of the contrast floor, the increased-contrast variant is used. Body text uses the terminal's own foreground color. States carry a label or an icon as well as a color, so nothing depends on telling hues apart. Set `prefersReducedMotion` in Claude Code to keep the crab still.

## Documentation

The manual is in [`docs/src`](docs/src/SUMMARY.md): [overview and limitations](docs/src/about.md), [install](docs/src/installation.md), [your first session](docs/src/usage.md), [conversations](docs/src/conversations.md), [configuration](docs/src/configure.md), [MCP and plugins](docs/src/integrations.md), [troubleshooting](docs/src/troubleshooting.md), and the [CLI](docs/src/cli.md), [slash command](docs/src/commands.md), [shortcut](docs/src/shortcuts.md) and [settings](docs/src/settings.md) references. Upstream publishes the same manual for its releases at [srothgan.github.io/claude-code-rust](https://srothgan.github.io/claude-code-rust/).

For the design history, including the move from fullscreen to inline rendering in 0.12.0, see Simon Rothgang's [I rebuilt Claude Code's terminal UI in Rust. Then I deleted 12,000 lines of it.](https://medium.com/@simonrothgang/i-rebuilt-claude-codes-terminal-ui-in-rust-then-i-deleted-12-000-lines-of-it-e8593a200452)

## Development

[Build and run locally](docs/src/development.md) and [Test and validate changes](docs/src/testing.md) have the build, test and verification commands. `recordings/showcase.sh` redraws the screenshots above from demo data (it needs [VHS](https://github.com/charmbracelet/vhs) and Node.js), and `scripts/bench/run.sh` reruns the benchmarks ([how](scripts/bench/README.md)).

## Status and limitations

The project is pre-1.0 and under active development. It follows new Agent SDK releases and problems found in daily use, and has no fixed roadmap or 1.0 date.

Startup is bounded by the Agent SDK runtime that the interface wraps: claude-rs draws its prompt at once, but a session is ready only when the engine has started.

## License

[Apache License 2.0](LICENSE), as upstream.

## Disclaimer and legal notice

This project is not affiliated with, endorsed by, or supported by Anthropic.

The original author's clarification applies to this fork: claude-code-rust is a terminal UI written from scratch in Rust. It is not a fork, copy or port of the latest Claude Code source leak. It talks to Anthropic's official [Agent SDK](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/agent-sdk) as a runtime dependency instead, the same way any other third-party tool would. No Anthropic source code was read or used as reference at any point during development.

The project authenticates through your existing Claude Code account via the Agent SDK, and the Agent SDK's terms allow building on top of it. Billing, credits, limits, and overage behavior are controlled by Anthropic, including any future changes Anthropic may make to how Agent SDK usage is metered. Other community projects do the same. As far as I can tell, using this project is fine, but I am a single maintainer, not a lawyer. If anything changes on Anthropic's end, I will update this section and adjust the project accordingly.

This project's source code is licensed under [Apache-2.0](LICENSE). The Agent SDK itself is proprietary and governed by [Anthropic's Commercial Terms of Service](https://www.anthropic.com/legal/commercial-terms).

For official Claude documentation, see [https://claude.ai/docs](https://claude.ai/docs).
