# Terminal regression fixture

`fake-bridge.js` is a local NDJSON peer for the real `claude-rs` binary. The PTY tests exercise terminal rendering, resizing, input delivery, and shutdown without calling a model or using credentials. They do not validate the real SDK or terminal font rendering.

Run the suite with `cargo test --all-features --test terminal_resize -- --test-threads=1`. Bun or Node and `rustc` must be available on `PATH`; the auth tests compile a small native fake CLI into their temporary directory.

The PR workflow runs this suite on Ubuntu, Windows, and macOS, and requires all three platform jobs to pass. Windows uses ConPTY; Linux and macOS use native Unix PTYs. Windows receives complete paste actions; Unix additionally exercises fragmented control sequences and UTF-8 scalars. Exact Unicode payloads, including combining accents, are checked in the journal on every platform. Submission waits out the application's existing paste Enter suppression window.

| Scenario | First turn |
| --- | --- |
| `stream` | Streams text and completes normally. |
| `hold-success` | Streams text, waits for the release file, then completes. |
| `hold-error` | Streams text, waits for the release file, then emits a service error. |
| `hold-eof` | Streams text, waits for the release file, then closes stdout by exiting. |
| `hold-malformed` | Streams text, waits for the release file, then writes invalid NDJSON and remains alive for shutdown. |
| `permission` | Starts a tool, requests an allow/deny decision, then completes after the correlated response. |
| `question` | Starts a tool, requests a destination selection, then completes after the correlated answer or cancellation. |
| `disconnect-during-auth` | Connects normally, then exits when the release file appears while the auth child owns stdin. |
| `resize-ready` | Streams a completed transcript for scrollback checks across zoom and reading-mode resizes. |
| `resize-replay` | Emits a large completed reply in one chunk for interruption and replay-budget checks. |
| `resize-code` | Completes the first reply, then holds a second reply inside an unfinished code block. |
| `native-diff` | Emits a retained session tool diff and answers `/diff` Git refreshes with a separate working-tree file. |
| `read-result` | Emits a completed Read of a four-line markdown file as `file_text` content, so a test can check one source line per row behind a line-number gutter. |
| `bash-colour` | Emits a completed Bash call whose output carries SGR colours, an OSC 8 hyperlink, a title change and a cursor-up, so a test can check which cells are coloured and that the rest never reach the terminal. |
| `hook-activity` | Sends a slow and a fast SessionStart hook right after connecting, then answers the first prompt after a failed PreToolUse hook. |
| `export` | Answers `export_session_history` with a short conversation (messages, a Bash call with output, an Edit with a diff) in two chunks under the command's request id. |
| `reload` | Answers the first `reload_skills_and_styles` command with two skills and three output styles, and every later one with a `slash_error`. |
| `render-corpus` | Resumes the shared render corpus as history, then streams its long reply in 200 chunks on the first prompt. |
| `session-accent` | Sends confirmed color updates for `/color purple` and `/color default`; replacement checks the session-ID fallback. |
| `resume-large` / `resume-large-fail` | Lists a 2 MiB saved session and returns a successful or failed compact-first turn after the picker choice. |
| `resume-failure` | Answers a resume of `Shared title` (ambiguous, two candidate ids) or `Missing title` (unknown) with `session_resume_failed` under the request id and sends no connect event; any other resume connects. |
| `session-refresh` | Lists only a saved session around connect, then answers `list_sessions` (sent when the picker opens) with the saved and the active session once the release file appears. |
| `session-management` | Lists a saved and active session; acknowledges tag and confirmed delete by exact ID with refreshed SDK-style metadata. |
| `verbose-bash` | Answers the first prompt with a completed Bash call whose output arrives both as update content and as raw output, then a short reply. |
| `agents` | Connects with a running and a completed subagent plus a background shell, as normalized task lifecycle rows. Lists a saved transcript only for the completed subagent, returns its messages, and answers a stop request with an acknowledgement followed by a separate `killed` lifecycle row. Prompts stream normally. |

`FAKE_BRIDGE_JOURNAL` records commands, events, and the `reply-held` coordination point as JSONL. `FAKE_BRIDGE_RELEASE_FILE` lets a test release a held turn after exercising the UI. Reply length and interval remain configurable with `FAKE_BRIDGE_LINES`, `FAKE_BRIDGE_FOLLOW_UP_LINES`, and `FAKE_BRIDGE_INTERVAL_MS`.

The fixture serializes overlapping prompts and correlates queued/start events by message UUID. Completion includes the remaining queue count. Cancellation emits an interrupt receipt and an `aborted_streaming` completion, stops the active timer, and keeps queued work. Permission and question requests follow an existing tool-call event; responses are correlated by session and tool-call ID. A new-session command emits `session_replaced` with a different session ID, and a resume whose launch settings ask for `fork_session` connects as `<session id>-fork`. These sequences follow the checked-in Rust wire types and TypeScript bridge handlers, but the fixture is not an independent SDK conformance oracle.

The tests cover queued/start ordering, cancellation followed by another turn, permission acceptance and denial, question selection with notes and cancellation, simultaneous resizing and input, and fatal EOF/malformed-output shutdown. Journals verify exact prompts and exactly one interaction response. EOF and malformed NDJSON are deliberately injected protocol faults, not claims that the SDK normally emits them. Protocol failures currently terminate the real app; these tests do not claim draft persistence after that exit.

Resize tests inspect native terminal scrollback as well as the visible screen, including one-row terminals, reading mode, unfinished code blocks, and a new resize interrupting an active transcript replay. They wait for draws at the requested dimensions and require every completed reply row to remain accessible.

`fake-claude.rs` is a native stand-in for `claude auth login`, placed first on the child app's `PATH`. It reads one line from inherited stdin, records it inside the isolated profile, and waits for a release file. It can succeed using a clearly invalid fixture token or fail with exit code 7. The spawn-failure case uses an invalid executable on Windows and an executable script with a missing interpreter on Unix; plain executable text can run through a shell on macOS and does not reliably trigger a spawn error. The tests verify that the real TUI stays silent during child ownership, sends all input to the child, restores usable typing and paste after resizing, and stops an owned child during fatal shutdown. They do not exercise browser OAuth, real credential renewal, or the installed Claude CLI.

Each test owns its temporary project, configuration directory, fake CLI, journal, and diagnostics log. No model, subscription, or user credential file is used. Add protocol behavior alongside a real workflow test; the search-retained scenario checks navigation over supplied history updates, while SDK history reconstruction and cancellation races remain outside this fixture's coverage.

## Render corpus

`render-corpus.json` is a deterministic transcript that covers what a chat renders: Markdown (headings, nested lists, a table, a quote, inline code, links, a long paragraph), fenced code in six languages, a 2000-line Bash output with ANSI colour, small and 500-line Edit and Write diffs, a failed tool call, notices, and user messages. `history` is a resumed conversation and `stream` is one long reply in chunks, both as bridge `session_update` payloads. The `render-corpus` scenario above replays it through the real binary; `src/app/connect/render_corpus.rs` replays the same file through the app's event path for the render benchmark (`cargo test --release --lib render_bench -- --ignored --nocapture`, compared across refs by `scripts/dev/bench-render.mjs`). The app has no thinking text to render, so the corpus carries the one thing it shows: the `thinking` activity phase that opens the streamed reply.

`generate-render-corpus.js` is the authority and the JSON is its committed output: edit the generator and rerun `node tests/fixtures/generate-render-corpus.js`. The test `render_corpus_fixture_matches_its_generator` runs it with `--check`, so a stale JSON fails `cargo test`. Changing the corpus changes every benchmark baseline, so compare refs that share one corpus.

The `session-agents` scenario exposes two current-session subagents, their saved messages, and a running task. It records list, inspect, and stop commands; Stop produces a task update so the panel displays the canonical stopped state.

- `agents-inspector`: two live agents with factual model choice, usage, prompt and a child Read. The SDK listing omits the second agent, proving Ctrl+T selection opens and retains its live inspector.

- `footer-agents`: three delegated agents (two running, one done), resolved models, usage and progress summaries, and a child Read. The release-file barrier finishes all agents; the PTY test checks 200/100/50-column layouts and the idle count after the shelf closes. Existing scenarios use lifecycle-ordered fixture statusLine output `[READY]` as a completion barrier; this is test data, not product footer chrome.
