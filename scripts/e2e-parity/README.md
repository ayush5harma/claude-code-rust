# Claude Code parity suite

End-to-end checks that drive claude-rs in a private tmux server against the
real stock Claude Code and record, per functionality, whether claude-rs does
what stock does. The results table is `docs/src/parity.md`.

It is local and opt-in. **It is never run in CI**: it needs a logged-in Claude
Code and spends real (small) model usage.

## What it needs

- A logged-in stock Claude Code (`claude`; by default the installed
  `~/.local/share/claude/ClaudeCode.app` binary) and its config directory
  (`--config-dir`, default `$PARITY_CONFIG_DIR` or `~/.claude-personal`).
- tmux, jq, node (for the fixture MCP server) and bash 4 or later.
- A claude-rs build: an installed release (default `~/.local/bin/claude-rs`;
  its bridge and Bun runtime sit next to it), or a source build with
  `--bin target/debug/claude-rs --bridge agent-sdk/dist/bridge.js` (the Bun
  runtime defaults to the newest installed release's).
- For the launcher-path checks (`rename.launch.*`, `color.launch.*`):
  system-config's `claude-launch` (`--launcher`, default
  `~/.local/bin/claude-launch`). The launcher only runs claude-rs from a
  release layout, so a source build is staged into one under the work
  directory (binary and `agent-sdk/dist` copied, runtime and `node_modules`
  linked from the installed release); the staged bridge must carry the EAGAIN
  fix (`scWriteSync`). Without a launcher those checks report SKIP.

## Cost and time

Every model turn uses Haiku (`--model haiku`). A full run takes about eight
minutes and costs about $0.40 (measured 2026-10-08), most of it the plan-mode check: the
stock child plans on Sonnet even when the session runs on Haiku. The run
prints the total, read from each session's Usage tab.

## Running

```sh
scripts/e2e-parity/run.sh --list                        # the checks
scripts/e2e-parity/run.sh --only cmd.model              # one check
scripts/e2e-parity/run.sh --only identity 'agents.*'    # groups or globs
scripts/e2e-parity/run.sh --report docs/src/parity.md   # full run + table
scripts/e2e-parity/run.sh --bin target/debug/claude-rs \
  --bridge agent-sdk/dist/bridge.js --report docs/src/parity.md
```

Each check prints one line, `PASS`, `FAIL` (claude-rs attempts it and the
result differs from stock), `GAP` (stock has it, claude-rs does not) or
`SKIP`, with the evidence it judged by: the screen row, registry entry or file
it read. Checks synchronise on what the screen, `claude agents --json` or the
files show, polling with a timeout, never on fixed sleeps alone.

The work directory (`--work`, default `$TMPDIR/claude-rs-parity`) holds one
scratch cwd per session (a copy of `fixtures/project`: a project skill, agent,
hooks, statusLine and an MCP server), `logs/` (claude-rs bridge diagnostics),
`screens/` (the capture a failing check judged by), `results.tsv` and
`inventory.tsv`. It is wiped at the start of each run.

## Safety

- tmux runs on its own socket (`tmux -L parity`) and the server is killed at
  the end of every run, including on failure or Ctrl+C.
- Sessions run in scratch directories under the work directory, never a real
  repository, so `/init` and the file tools write nothing that matters. The
  first launch in each directory answers claude-rs's trust prompt with Yes.
- The suite never edits the config directory's `settings.json`; setting-
  dependent checks use project settings in the scratch directory.
- The agent view lists the owner's real background sessions: the checks only
  read it and leave with Esc, never Enter, Space or Ctrl+X.
- `/copy` saves and restores a plain-text clipboard and skips when the
  clipboard holds anything else; the Ctrl+V image check is skipped.
- Every session id the run creates is appended to `--sessions-file` (default
  `WORK/test-sessions.txt`) and printed at the end, for cleanup. The owner's
  SessionEnd hooks still run for these sessions.
- Variables of an enclosing Claude Code session (`CLAUDECODE`,
  `CLAUDE_CODE_SESSION_ID`, ...) are scrubbed before tmux starts.

## Layout

- `run.sh`: options, preflight, the run loop, cleanup, the summary.
- `lib/harness.sh`: tmux, screen polling, session lifecycle, the launcher
  staging, result recording. `lib/report.sh`: `docs/src/parity.md`.
- `checks/NN-<group>.sh`: one `def` plus one `chk_<id>` function per check.
- `data/stock-commands.txt`: stock's built-in slash commands (refresh it when
  stock adds commands).
- `fixtures/`: the scratch project, the MCP echo server and the `$EDITOR`
  stand-in.
