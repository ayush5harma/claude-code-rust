# Claude Code parity suite

End-to-end checks that drive claude-rs in a private tmux server against the
real stock Claude Code and record, per functionality, whether claude-rs does
what stock does. The results table is `docs/src/parity.md`.

It is local and opt-in. **It is never run in CI**: it needs a logged-in Claude
Code and spends real (small) model usage.

## What it needs

- A logged-in stock Claude Code (`claude`; by default the installed
  `~/.local/share/claude/ClaudeCode.app` binary) and an explicit, isolated
  config directory (`--config-dir` or `$PARITY_CONFIG_DIR`). The suite refuses
  `~/.claude` and `~/.claude-personal`, including paths that resolve to them.
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

Every model turn uses Haiku (`--model haiku`). A full run took about eight
minutes and cost about $0.40 on 2026-10-08. Most of that measured cost came
from the plan-mode check: the stock child plans on Sonnet even when the
session runs on Haiku. The run prints a TUI Usage subtotal. Idle CPU and memory now use a 120-second settling period
per target before the 60-second sample (`PARITY_PERF_SETTLE` and
`PARITY_PERF_IDLE`). Allow about 15 minutes for a full run; model usage does
not increase during these idle waits.

## Running

```sh
scripts/e2e-parity/run.sh --list                        # the checks
export PARITY_CONFIG_DIR=/path/to/isolated-claude-config
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

The work directory (`--work`, default a new unique directory under `$TMPDIR`)
holds one
scratch cwd per session (a copy of `fixtures/project`: a project skill, agent,
hooks, statusLine and an MCP server), `logs/` (claude-rs bridge diagnostics),
`screens/` (the capture a failing check judged by), `results.tsv` and
`inventory.tsv`. An explicit `--work` must be empty for a fresh run. `--keep`
requires an explicit directory marked by an earlier suite run; only selected
result rows and suite-owned fixture directories are replaced.

## Safety

- tmux runs on a unique per-run socket. The suite kills only that server at
  exit, including on failure or Ctrl+C. It clears inherited `NO_COLOR` and
  uses a color-capable terminal before starting tmux.
- Sessions run in scratch directories under the work directory, never a real
  repository, so `/init` and the file tools write nothing that matters. The
  first launch in each directory answers claude-rs's trust prompt with Yes.
- The suite never edits the config directory's `settings.json`; setting-
  dependent checks use project settings in the scratch directory.
- The suite starts no background sessions. The agent-view gap row presses
  Left once on an empty prompt and leaves with Esc if anything opens.
- Resume rendering uses an offline 400-record synthetic transcript under the
  isolated config and scratch cwd, without a model call to generate history.
  The performance row fails when the build exceeds stock or the installed
  baseline by more than 10% plus its documented absolute slack.
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
