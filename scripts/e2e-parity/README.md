# Claude Code parity suite

End-to-end checks that drive a real claude-rs build in a private tmux server
against the real, logged-in stock Claude Code and record, per functionality,
whether claude-rs does what stock does. The results table is
`docs/src/parity.md`. New features add their live checks here: a `def` line and
a `chk_<id>` function in the matching `checks/NN-<group>.sh`.

It is local and opt-in. **It is never run in CI**: it needs a logged-in Claude
Code and spends real (small) model usage.

## What it needs

- tmux, jq, node (for the fixture MCP server) and bash 4 or later (on macOS:
  not `/bin/bash`; the scripts' `env bash` finds a newer one on PATH).
- A claude-rs build: an installed release (default `~/.local/bin/claude-rs`;
  its bridge and Bun runtime sit next to it), or a source build with
  `--bin target/debug/claude-rs --bridge agent-sdk/dist/bridge.js` after
  `cargo build` and `npm run build --prefix agent-sdk` (the Bun runtime
  defaults to the newest installed release's). A debug build skips the
  performance group, which would otherwise fail by construction; use
  `cargo build --release` to measure it. A release build ignores `--bridge`
  and the `CLAUDE_RS_AGENT_BRIDGE*` variables (`src/agent/bridge.rs` honours
  them in debug builds only): it finds the bridge script and the Bun runtime
  next to its own executable. To measure a source release build, copy it into
  a scratch directory laid out like an installed release (the binary,
  `claude-rs-bridge-bun` from an installed release, `agent-sdk/dist` and
  `agent-sdk/package.json`, and a `node_modules` link to
  `agent-sdk/node_modules`) and pass that binary with `--bin`, without
  `--bridge`. Make the scratch directory with
  `mktemp -d "${TMPDIR:-/tmp}/claude-rs-release.XXXXXX"` (not inside the
  checkout or the work directory) and delete it with `rm -rf` when the run is
  done; it holds a copy of the binary and a symlink, nothing else.
- A **logged-in stock Claude Code binary**, started directly. Resolution:
  `--claude`, else `$PARITY_CLAUDE_BIN`, else the installed
  `~/.local/share/claude/ClaudeCode.app` binary, else `~/.local/bin/claude`,
  else `claude` on PATH. A script in that place is refused: a launcher such as
  system-config's `claude-launch` can route a bare `claude` to claude-rs, which
  would make the stock side of every comparison claude-rs. (That launcher's
  own switch for stock is `SC_CLAUDE_TUI=stock`; the suite never needs it
  because it never runs the launcher.)
- A logged-in config directory (see below).

## Which profile

On macOS the login is stored in the keychain under a name that depends on the
config directory's path, so a fresh, isolated directory is not logged in.
Two ways in:

- `--config-dir DIR` or `$PARITY_CONFIG_DIR`: a directory that is already
  logged in and is not the owner's. The suite refuses `~/.claude` and
  `~/.claude-personal`, including paths that resolve to them.
- `--owner-profile`: use `$CLAUDE_CONFIG_DIR` (else `~/.claude`) itself; the
  variable is only passed to the sessions when it was set, because setting it
  to the default path selects a different keychain item. The sessions still run
  in scratch directories, never a real project, but the owner's settings,
  hooks, plugins and MCP servers apply. It cannot be combined with `--keep`.
  What the suite's own cleanup removes, and what stays, is under Safety.

## Cost and time

Every model turn uses Haiku by default (`--model haiku|sonnet|opus` or
`$PARITY_MODEL`). The run ends with the cost, summed from the `/usage`
readout of each claude-rs session when the suite stops it, and warns above
`$PARITY_COST_BUDGET` (default 0.60). The performance group runs no model
turns but idles 120 s of settling plus a 60 s sample per target
(`PARITY_PERF_SETTLE`, `PARITY_PERF_IDLE`), so a full run with it takes about
21 minutes (one 12-minute block is the performance sampling); without it (a
debug build) about 9. Measured 2026-10-13 on Haiku, 113 checks, owner profile:
$0.50 to $0.56, 527 s without the performance group and 1277 to 1308 s with it; the
plan-mode check is $0.30 of it because the stock child plans on Sonnet.

## Running

```sh
scripts/e2e-parity/run.sh --list                        # the checks
export PARITY_CONFIG_DIR=/path/to/logged-in-claude-config   # or --owner-profile
scripts/e2e-parity/run.sh --only cmd.model              # one check
scripts/e2e-parity/run.sh --only identity 'agents.*'    # groups or globs
scripts/e2e-parity/run.sh --only '10-*'                 # one check file
scripts/e2e-parity/run.sh --report docs/src/parity.md   # full run + table
scripts/e2e-parity/run.sh --bin target/debug/claude-rs \
  --bridge agent-sdk/dist/bridge.js --model haiku --only '10-*'
```

`--only` takes check ids, group names and globs over ids or over the check
file names (`10-*` is `checks/10-launch.sh`).

Each check prints one line, `PASS`, `FAIL` (claude-rs attempts it and the
result differs from stock), `GAP` (stock has it, claude-rs does not) or
`SKIP`, with the evidence it judged by: the screen row, registry entry or file
it read. Checks synchronise on what the screen, `claude agents --json` or the
files show, polling with a timeout, never on fixed sleeps alone.

The work directory (`--work`, default a new unique directory under `$TMPDIR`)
holds one scratch cwd per session while the run lasts (a copy of
`fixtures/project`: a project skill, agent, hooks, statusLine and an MCP
server), and keeps `logs/` (claude-rs bridge diagnostics), `screens/` (the
capture a failing check judged by), `results.tsv` and `inventory.tsv` as the
evidence. An explicit `--work` must be empty for a fresh run. `--keep`
requires an explicit directory marked by an earlier suite run; only selected
result rows and suite-owned fixture directories are replaced, and nothing is
cleaned up at exit.

## Safety

- tmux runs on a unique per-run socket. At exit (also on failure or Ctrl+C)
  the suite stops each session, kills only that server, and unless `--keep`
  deletes the scratch cwds and the transcripts, `file-history` and
  `session-env` entries Claude Code wrote for them under the config
  directory, after waiting for hooks still reading them. It clears inherited `NO_COLOR` and
  uses a color-capable terminal before starting tmux.
- Sessions run in scratch directories under the work directory, never a real
  repository, so `/init` and the file tools write nothing that matters. The
  first launch in each directory answers claude-rs's trust prompt with Yes.
- The suite never edits the config directory's `settings.json`; setting-
  dependent checks use project settings in the scratch directory, or flags
  (`--settings`) of the session under test.
- Two things outside the work directory are touched, both guarded and undone
  by `fork_cleanup` (`checks/90-fork.sh`, called from the exit trap):
  `fork.composer_rules` writes claude-rs's own app settings file
  (`~/Library/Application Support/claude-code-rust/settings.json`, where the
  Composer style preference lives) for the few seconds of one session, only
  if no such file exists, and deletes it again; `fork.agents` starts one
  fixture process (`fixtures/hold.mjs`, a 90 s timer) inside a subagent and
  stops it from the `/agents` panel, and the cleanup kills any that remain.
- `fork.compact_first` writes a synthetic 1.3 MiB transcript under the scratch
  cwd's project folder without a model call and never presses `c`, which
  would compact it with a billed turn.
- `present.capture` also starts the stock TUI, in its own scratch cwd, for two
  prompts; its screens land under `WORK/presentation/`. Checks that find a
  real claude-rs defect also write `WORK/defects/NN.md`, which the report
  prints under "Defects found by this run".
- The suite starts no background sessions of its own beyond the one the
  background-subagent check asks a model turn to start.
- Resume rendering uses an offline 400-record synthetic transcript under the
  isolated config and scratch cwd, without a model call to generate history.
  The performance row fails when the build exceeds stock or the installed
  baseline by more than 10% plus its documented absolute slack.
- `/copy` saves and restores a plain-text clipboard and skips when the
  clipboard holds anything else; the Ctrl+V image check is skipped.
- Every session id the run creates is appended to `--sessions-file` (default
  `WORK/test-sessions.txt`) and printed at the end. The owner's hooks still run
  for these sessions; `CLAUDE_ARCHIVE_SKIP=1` is exported so a SessionEnd hook
  that archives transcripts and honours it skips them.
- Cleanup under the config directory is by this run's own ids and cwds only:
  `projects/<scratch cwd>`, and `file-history/<id>`, `session-env/<id>` and
  `tasks/<id>` for each session id (validated as a UUID first). Plan files are
  written to `.parity/plans` in the scratch cwd, by the fixture's
  `plansDirectory` setting, and go with it.
- What `--owner-profile` still leaves behind, because it is Claude Code's own
  bookkeeping that the suite will not edit: entries for the scratch cwds in
  `.claude.json` (trust and per-project state) and the rotated
  `backups/.claude.json.backup.*` copies of it, files under `shell-snapshots/`
  and `telemetry/`, and whatever a hook does with a session beyond the
  archive opt-out. An isolated `--config-dir` can simply be deleted.
- Variables of an enclosing Claude Code session (`CLAUDECODE`,
  `CLAUDE_CODE_SESSION_ID`, ...) are scrubbed before tmux starts.

## Layout

- `run.sh`: options, preflight, the run loop, cleanup, the summary.
- `lib/harness.sh`: tmux, screen polling, session lifecycle, result
  recording. `lib/report.sh`: `docs/src/parity.md`.
- `checks/NN-<group>.sh`: one `def` plus one `chk_<id>` function per check.
  `90-fork.sh`, `91-fork-sessions.sh` and `92-presentation.sh` hold the checks
  for the fork's own features (ids `fork.*`, `present.capture`).
- `data/stock-commands.txt`: stock's built-in slash commands (refresh it when
  stock adds commands).
- `fixtures/`: the scratch project, the MCP echo server and the `$EDITOR`
  stand-in.
