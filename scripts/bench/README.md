# claude-rs vs Claude Code benchmark

Measures CPU and memory of claude-rs against the stock Claude Code CLI on the
same machine, account, model and terminal, and writes Markdown tables for the
README. Like `e2e-parity/`, it is local and opt-in and **never runs in CI**: it
needs a logged-in Claude Code and spends a little real model usage (one short
Haiku turn per streaming run).

## What it needs

- macOS (process accounting reads `libproc` through `ctypes`), tmux, `python3`
  (standard library only; every script runs under `python3 -I`).
- A **stock Claude Code binary**, passed directly with `--claude`. Not a shell
  alias or a launch wrapper: those would add their own processes, or route to
  claude-rs.
- A **claude-rs release layout**: an installed release (the binary with its
  bridge and Bun runtime next to it), or a source release build laid out the
  same way. `scripts/e2e-parity/README.md` describes that scratch layout; a
  release binary ignores `CLAUDE_RS_AGENT_BRIDGE*`.

## Running

```bash
scripts/bench/run.sh \
  --claude-rs ~/.local/bin/claude-rs \
  --claude ~/.local/share/claude/versions/<version> \
  --runs 5 --mcp none --out /tmp/ccbench-out
```

| Option | Meaning |
| --- | --- |
| `--runs N` | Runs per tool per scenario (default 5). Tool order alternates every run. |
| `--mcp none\|user` | `none` starts both tools with `--strict-mcp-config` and no servers, so only the tools are measured. `user` loads the MCP servers configured under `CLAUDE_CONFIG_DIR`, identically for both. |
| `--engine official\|bundled\|<path>` | The agent engine under claude-rs. `official` (default) is the `--claude` binary, so both sides run the same engine version; `bundled` is the Agent SDK's own. |
| `--scenarios main,typing,resume` | `main` is start-up, idle and one streaming turn. |
| `--session-file F` | Transcript for the resume scenario (default: a large idle one from the config dir). |
| `--idle-secs S` | Idle window (default 30). |

`CLAUDE_CONFIG_DIR` is passed through when set; otherwise both tools use the
default config. Output: `summary.md` (tables, versions, machine, method notes),
`machine.json`, and one `raw/<scenario>_<tool>_<n>.json` per run.

## Scenarios

| Scenario | Measured |
| --- | --- |
| Start-up | Spawn to an input-ready prompt; tree CPU until it settles (below 3% of a core over 2 s). |
| Idle | 30 s at the prompt, sampled every second: CPU, RSS, physical footprint, threads, process count. |
| Streaming turn | `Write the integers from 1 to 400, one per line, with no other text.`: tree CPU from Enter to the last screen change, peak RSS and footprint, wall time. |
| Typing | 300 characters into the composer (not submitted): time until all are on screen, CPU spent. |
| Resume and scroll | Resume a large transcript: CPU and memory once it settles, then CPU for 20 PageUp keys. |

Every metric is reported for the whole process tree and per role: `ui`
(claude-rs), `bridge` (the Bun bridge), `engine` (the Claude Code child that the
Agent SDK starts), `ui+engine` (stock Claude Code, one process) and `other`
(MCP servers and their wrappers).

## Side effects and safety

- Each session runs in a fresh `git init` repository at `/private/tmp/ccbench-cwd`
  inside a private tmux server (`tmux -L ccbench`), with a whitelisted
  environment. The scratch directory and its `projects/` entry are removed after
  every run.
- Teardown is SIGKILL of the recorded descendants, never a kill by name, so
  `SessionEnd` hooks do not fire on benchmark sessions. Exit-time behaviour is
  therefore not measured.
- Resume never opens the original transcript. A copy is written under a new
  session id with its recorded `cwd` rewritten to the scratch directory, and
  the original's hash is checked before and after every run; the harness aborts
  if it changed. (A same-id copy is unsafe: claude-rs resolves a bare session id
  across all projects and can append metadata to the original.)
- The one-time trust prompt is accepted in an unmeasured warm-up launch, which
  leaves one trust entry for `/private/tmp/ccbench-cwd` in the config's
  `.claude.json`. Both tools also write their usual history and logs to the
  config directory.

## Reading the numbers

- Compare physical footprint (Activity Monitor's "Memory"), not summed RSS:
  RSS counts shared pages once per process and drops under memory pressure.
- Peaks come from 4 Hz sampling; short spikes can be missed. Hook and tool
  children that exit between samples are still counted through their parent's
  reaped-children CPU.
- The ready markers differ: claude-rs paints its composer at once and is marked
  ready when the engine connects; Claude Code draws its prompt after its own
  init. The "settles" rows are the like-for-like start-up comparison.
- Start-up is warm-cache (one unmeasured warm-up launch per tool; the file cache
  is not purged). The load average and free memory before each run are in
  `raw/*.json`; run it on an otherwise quiet machine.
