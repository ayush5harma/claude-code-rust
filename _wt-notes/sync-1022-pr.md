Merges upstream main `246757b` (0.16.0, 0.16.1 and #508-#510: #484-#510, 26 commits) into the fork as one merge commit `aded13d` (fork main `9e58256`, merge base `1c25754`, 102 conflicted files, 429 conflict hunks), plus one follow-up commit `f5b9cb4` (rustls-webpki 0.103.16 for RUSTSEC-2026-0382). Policy: one authority per invariant; where upstream now implements what the fork also had, upstream's version stays and the fork copy is deleted; fork-only behaviour is rebuilt on upstream's structure.

## Fork features removed or reshaped in favour of upstream

| Fork feature | Now | Why |
| --- | --- | --- |
| `/agents` inspector (fork #34, #18, #35): own list/detail UI, running-first sort, `forget_unlisted_selection`, `recent_child_tool_ids` deque, `tool_call_scopes` | Removed; upstream's full-screen `/agents` (#493) | Same feature; upstream adds run-id stop guard, Interrupted, saved-response preview, Markdown transcript |
| Ctrl+T task list, agent shelf, per-row clock/tools/tokens, finished-agents field | Kept, re-pointed at upstream's `subagent_rows` / `recent_child_tools` | Fork-only facts rebuilt on upstream's data |
| `/export` (fork) | Removed; upstream's (#491) | Fork version was a subset |
| `/reload` running beside other commands (`Promise.all`, `boundedReloadControl`, read lane) | Upstream's queued `/reload` with a 10 s deadline (#487) | One fix for the stalled-queue hang |
| Bridge-side launch config (`parseLaunchConfig`, rename before connect) | Upstream's Rust `SessionConfig` (#492) | Rust parses `--settings` / `--mcp-config`; bridge renames after init |
| Resume by exact title, id beyond the list limit, saved title as process title on resume | Kept on upstream's `resume()` / `buildLaunchQueryOptions` | Fork-only; upstream passes no title on a plain resume |
| CLI: flag merge after `resume`, `parse_existing_dir`, `parse_session_name`, blank-prompt parser, `--bridge-script` is-file check, legacy `--log-append` target | Upstream's global flags, `validate_launch_paths`, `sanitize_session_title`, `inspect_bridge_script` (#489, #492) | Same checks, one owner |
| CLI help epilogue, typo guard, Session/Diagnostics headings, exit 25 without a terminal, parse-time `--log-filter` | Kept on upstream's `Cli` | Fork-only |
| Rust hook failure notices (`HookNotice`) | Removed; the bridge reports failures and decisions (#494) | Would double-warn; `presentation.hookNotices` now covers slow-hook notices only |
| Read `file_text` content variant (`FileText`) | Removed; upstream's Read previews (#484) | Same facts via `readTextResult` + `output_metadata.read` |
| `ui/ansi.rs` SGR interpreter | Removed; upstream's `ui/terminal_output.rs` (#484) | One interpreter for shell output |
| Markdown: `render_code_block`, `hard_wrap_line`, fork edits to `document_table.rs` / `markdown/lists.rs` | Upstream's `document.rs` + tui-markdown (#497, #498); fork headings, shaded code and link shortening ported onto it | Upstream deleted both files |
| Status "Session name" fallback to summary / first prompt | Upstream's rule: title only (#488) | One title rule |
| Trust prompt intro text and directory rows | Upstream's layout and wording (#490), fork palette | Same prompt |
| `presentation.footer` in the bridge settings catalog | Upstream's Rust footer preferences in claude-rs's own settings file (#485), 12 fields | Upstream owns footer preferences |

## Decisions in detail

### #484 Markdown, Read previews and shell output
- `src/ui/terminal_output.rs` is the one escape-sequence interpreter. Deleted `src/ui/ansi.rs` and the highlight wrappers around it; the `statusLine` row strips through `terminal_output::strip_ansi` and still filters control characters at its boundary.
- DROPPED: the fork tests `bash_output_never_passes_other_control_sequences_through` and `bash_output_drops_unsupported_sgr_instead_of_printing_it`. They pinned `ansi.rs` behaviour; upstream renders reverse video and honours carriage returns. The SGR-colour and diff-colour tests stay.
- Read previews are upstream's `read.rs`. DROPPED the fork's `FileText` variant end to end: `model/tools.rs`, `wire.rs`, `types.ts`, `tooling.ts` (`fileTextReadContent`), the converter, retention and export arms, `src/ui/tool_call/read_file.rs`, its tests, the `read-result` fixture scenario and its PTY test. Kept: the Read preview's syntax theme follows the appearance (Monokai Extended Bright / Light).

### #485 footer preferences in Rust, #493 `[AGENTS:n]`
- Upstream's `app/footer/preferences.rs` owns the layout. The fork's `sessionTitle`, `rateLimits` and `agents` fields are fields in it (12 in total). Default order: left permissionMode, model, effort, ultracode, fastMode, runningSubagents, sessionTitle; right directory, gitBranch, contextRemaining, rateLimits, agents. The three fork fields are not user-colourable (they keep their fixed and gauge colours).
- RESHAPED (owner should know): footer preferences now live in claude-rs's own settings file (`CLAUDE_RS_CONFIG_DIR`, else the system config dir), not in Claude Code's `settings.json`. A layout saved by fork.7 under `presentation.footer` in Claude Code's settings is not read and is not migrated; set it again in `/config`. `presentation.footer` is removed from the bridge catalog; `presentation.composerStyle` stays.
- RESHAPED: `SessionRuntimeState.last_rate_limit_update`, which #505 removed, is back (set by `handle_rate_limit_update`, cleared on session reset). The `rateLimits` field falls back to it because usage snapshots are only fetched by the Usage tab.
- Owner note: upstream's `[AGENTS:n]` (running subagents) and the fork's agent shelf both show while agents run.

### #486 Shift+arrow shortcuts: taken
- No clash with fork keys. Ctrl+F (transcript search) and Ctrl+D (delete or quit) keep their fork meaning, as before the merge.

### #487 `/reload`: upstream wins
- Upstream's `ReloadOutcome` per half, styles before skills, the 10 s queue deadline and `skills_and_styles_reload_expired`.
- DROPPED: the fork's concurrent reload (`Promise.all`, `boundedReloadControl`), `reload` in `CONCURRENT_COMMANDS`, the test "skills reload runs while startup session data is blocked", the fork's reload client tests and the fake bridge's reload arrays. A `/reload` sent while startup data is still loading now waits in the queue and asks to be run again after 10 s, where the fork ran it at once. `list_sessions` stays alone in the read lane (picker refresh).

### #488 rename once saved: upstream wins
- `derive_session_name` (title only). DROPPED the fork's Status fallback to the summary or first prompt; the footer `sessionTitle` field follows the same rule.

### #489 `--dir` / `--bridge-script` validation, #490 trust prompt: upstream wins
- `validate_launch_paths` and `inspect_bridge_script` replace the fork's `parse_existing_dir` and `--bridge-script` is-file check. `--bridge-script` stays hidden in help.
- Trust prompt: upstream's layout and wording on the fork palette; the fork's intro text and directory rows are dropped.

### #491 `/export`: upstream wholesale
- `history_export.ts` and its test, `src/app/export/*`, `emitHistoryExportFailure` for an unknown session. The fork's export code and client tests are removed.

### #492 launch flags: upstream structure, fork extras ported
- Upstream's wire `SessionConfig` (settings and MCP configs parsed in Rust as JSON objects), `launch_config.ts` and `session_config.rs`.
- DROPPED: the bridge's `parseLaunchConfig` / `LaunchConfig`, its own system-prompt and flag-settings copy, the malformed `--settings` bridge tests, rename before connect with `RENAME_FAILURE_FIXTURE`, duplicate `buildQueryOptions` tests; in Rust, `Command::Resume` flattened options and flag merging after `resume` (upstream's global flags cover it), `Command::name`, the per-flag "applies only to interactive launches" wording, the fork's `parse_session_name` and blank-prompt parser; 7 `cli_arguments` tests for them.
- KEPT: resume by exact title or by an id beyond the list limit, with ambiguity reported through `session_resume_failed` (upstream's session_title test now waits for that event); the help epilogue (examples, exit statuses including 25, environment), `override_usage`, the Session Options and Diagnostics Options headings, the typo guard (`Cli::parse_from_args`), `--log-filter` validated at parse time, `--log-append` requiring `--log-file`, exit 25 without a terminal and help on a closed pipe.
- RESHAPED (upstream behaviour changed): a plain resume passes `name ?? saved title` as the SDK process title (`buildLaunchQueryOptions` takes `resumedTitle`). Upstream passes nothing, which registers the folder name in the SDK process registry. Upstream's contract assertion is changed to expect the saved title; its fixture stores the title only for new or forked sessions.
- Rewind keeps the current model, mode and effort (fork arm kept in `session_start.rs`).

### #493 `/agents`: upstream wins, fork list and shelf rebuilt on it
- Upstream's `app/agents.rs`, `ui/agents.rs` and both test files wholesale.
- Ported onto it: `open_at`, `open_task_list` and `handle_list_key` for the Ctrl+T list (Enter opens the view on that agent; test ported); `is_subagent` made `pub(crate)`; the shelf, finished-agents field and `agent_rows.rs` re-pointed at `subagent_rows`, `launching_call` and `recent_child_tools`; status badges on `theme::Tone`; the "Usage" and "Worktree: isolated" detail rows.
- A new SDK run of an existing task restarts its row clock (`run_id` change); rows first seen already finished show no observed elapsed time (resumed rows used to show "Usage: 0s").
- DROPPED: the fork inspector UI and tests, `forget_unlisted_selection`, the running-first sort (lifecycle order everywhere), the `recent_child_tool_ids` deque and `tool_call_scopes`.
- Bridge `tasks.ts`: upstream's run admission plus the fork's `usage` / `last_tool_name` lifecycle metadata and child-model observation (`observeTaskChildModel`, `applyLinkedChildModel`). The fork's late `applyTaskLifecycleState` call is removed.
- Owner note: the shelf and Ctrl+T rows show the live child model (from child messages); upstream's "Model:" row in `/agents` shows `agent_model_choice`. Two sources for the agent's model remain.

### #494 logging and failed hooks: upstream's bridge handler is the one owner
- `handleHookSystemMessage` in `message_handlers.ts` owns hook messages (logging, the exit-2 decision table, failure/decision notices held in `pendingHookNotices` until connect). The fork's `hook_activity.ts` is reduced to `forwardHookActivity`, called first, so Status "Hooks this session", the startup timeline and slow-hook notices keep their data. Both pending queues stay (they hold different data) and flush together at connect.
- RESHAPED: Rust `HookRun::notice` returns timing notices only; `HookNotice` is removed and `presentation.hookNotices` now governs slow-hook notices only (catalog text updated). A PreToolUse exit 2 is a decision and gets no notice (upstream semantics).
- Owner note: Status "Runs: N (k failed)" counts an exit-2 decision as failed while no notice is shown.
- Logging is upstream's `logging/mod.rs` minus the legacy shared-log `--log-append` target (the fork requires `--log-file`); upstream's legacy-append test is dropped.

### #495 incremental replay, #500 / #501 tool output: taken
- `chat_session.rs` is upstream's update / `prepare_rows` / replay flow. The fork's `plan_chat_frame` is re-extracted around upstream's `layout_pending` logic so `render_bench` still paints the real plan. `render_composer_field` draws the fork's filled composer field with its session-coloured edge, or the rules style, for both editor bodies.
- `Transcript.revision` (fork; content, used by search) is kept beside upstream's `topology_revision` (positions). Different invariants, bumped at the same sites.
- Tool-call rendering is upstream's structure on the fork palette.

### #496 SDK 0.3.302, #505 consolidated SDK types
- Both `package.json` files pin `@anthropic-ai/claude-agent-sdk` 0.3.302; `EXPECTED_AGENT_SDK_VERSION = "0.3.302"`. Lockfiles regenerated with `npm install --package-lock-only --ignore-scripts`.
- `types.rs` re-exports upstream's model types; identity converters removed. The fork's `sessions_list_failed` event stays in the wire contract (`wire.rs` and `types.ts` together).

### #497 Oniguruma, #498 tui-markdown lists and tables: upstream structure, fork styling ported
- `syntect` `default-onig`, `two-face` `syntect-onig`, `tui-markdown` without default features.
- Ported onto `document.rs`: the heading hierarchy, shaded code rows (indent, rail, gutter, fill to the row end, label row inside the shade, tabs as four spaces; unbounded layouts are not padded), `prefix_block_lines` keeping the code fill inside quotes, and link destination shortening (inline links and a tui-markdown post-pass). Upstream's code-layout tests are updated to the shaded rows.
- RESHAPED (owner should know): code blocks inside list items are drawn by tui-markdown and are no longer shaded; a code block without a language shows upstream's `code` label (the fork showed none).
- DROPPED: `render_code_block`, `hard_wrap_line` (upstream's `split_line_at_width`), and the fork's edits to `document_table.rs` and `markdown/lists.rs`, which upstream deleted.

### #499 / #506 releases, #502-#504 / #510 dependencies and CI, #508 / #509 docs
- Crate and npm versions stay `0.15.2-fork.7`. Each Cargo dependency takes the higher requirement; `Cargo.lock` is the fork lock re-resolved by cargo. `tempfile` stays a runtime dependency (upstream made it dev-only, but the fork's Ctrl+G external-editor draft uses it in `src/app/mod.rs`).
- `CHANGELOG.md`: git's auto-merge put upstream's 0.16.0 entries under fork.2 and the fork's Unreleased section under 0.16.0. Rebuilt as: Unreleased (this sync and the previous one), upstream 0.16.1 and 0.16.0 verbatim, then the fork releases as they were.
- CI pin bumps from upstream are taken; the fork's release workflow (GitHub releases without npm) is unchanged. `assets/demo.gif` and `recordings/demo.tape` are upstream's (#508).
- Docs: upstream's manual structure (#509). Fork-only content is ported into the new pages (cli, conversations, settings, diagnostics, shortcuts, subsystems, testing; CONTRIBUTING's `check-all.mjs` / `nix develop` paragraph and the Render Benchmark section moved to `docs/src/testing.md`). `README.md` is the fork's, unchanged.
- Open item: `docs/src/parity.md` and `docs/src/presentation-gaps.md` (fork-generated) still describe pre-merge behaviour; only their build warnings were fixed.

### Rust 1.98 lints in upstream code
- The fork pins 1.98 (upstream 1.89), so newer clippy lints fired on upstream code: `Box::default` -> `SettingsBrowse::default()`, a missing `;` in `ui/config/mcp.rs`, `loop` -> `while let` in `ui/inline_chat_rows.rs`.

### Follow-up commit `f5b9cb4`
- `cargo deny check advisories` failed on RUSTSEC-2026-0382 (rustls-webpki 0.103.15), present in both merge parents' locks. `cargo update -p rustls-webpki --precise 0.103.16` fixes it; cargo also points tempfile at the getrandom 0.3.4 already in the graph instead of 0.4.3 (both satisfy its range).

## Checks (Rust 1.98.1 and Node 24.21.0 via `nix develop`)
All on the final head `f5b9cb4` unless noted. Cargo tests ran with `env -u CLAUDE_RUST_NO_UPDATE_CHECK -u CLAUDE_RS_BRIDGE_DIAGNOSTICS`: the owner's shell sets both, and the `update_check` tests fail while `CLAUDE_RUST_NO_UPDATE_CHECK=1` is set.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --all-targets --all-features -- -D warnings`: pass.
- `cargo test --all-features --no-fail-fast`: pass. lib 2316 passed / 1 ignored; main 3; claude_config_dir 3; cli_arguments 7; cli_startup 20; config_flow 5; docs_cli_contract 3; integration 35; slash_input_flow 27; terminal_resize 77 passed / 1 ignored (38 s); doc-tests 0. In total 2496 passed, 0 failed, 2 ignored.
  - Load flake: `app::update_install::tests::worker_records_a_failed_install_and_a_later_success_clears_it` failed in 3 of 6 parallel lib runs before the commit (`left: AlreadyRunning, right: Installed` at `src/app/update_install.rs:603`); it passed 20/20 alone and in the final run. The test and `update_install.rs` are upstream's and unchanged by the merge (only the copyright line differs from fork main). A likely cause is the worker's lock file descriptor being briefly held by another test's child process between fork and exec, so the second `try_lock` sees `WouldBlock`. Not fixed here. A fork-main baseline run was started but stopped when the disk filled (see the note below), so the "pre-existing" claim rests on the unchanged code, not on a measurement.
  - `bridge_shutdown_deadline_covers_a_blocked_stdin_writer` (the known flake) did not fail in any run.
- `cargo fetch --locked`: pass (before and after the bump).
- `cargo deny check bans licenses sources advisories`: on the merge commit, advisories FAILED (RUSTSEC-2026-0382, rustls-webpki 0.103.15, also in both parents' locks); after `f5b9cb4`: advisories ok, bans ok, licenses ok, sources ok (11 `duplicate` warnings).
- `cargo build --release --locked --bin claude-rs`: pass (the smoke binary).
- agent-sdk (merge tree; the bridge sources are unchanged since): `npm ci` (0 vulnerabilities), `npm run build`, `npm run test` 695/695 pass, `npm run lint` (93 files, no fixes), `npm run knip` (no findings), `npm run audit` ("No direct dependency advisories at moderate or higher severity").
- Root: `npm ci` pass; `npm run quality:duplicates` pass: 31 clones, 553 duplicated lines (0.38%; Rust 30 clones 0.44%, TypeScript 1 clone 0.06%).
- `mdbook build docs`: clean (no warnings) with the rebuilt changelog; link check after the docs merge found 0 broken links.
- Not run: `cargo +1.88.0 check --all-features` (MSRV; 1.88 toolchain not installed); `recordings/showcase.sh` (needs VHS; its scenario was exercised by hand, see Smoke).
- Note: the machine's data volume was full during this work (about 510 MB free after the fork-main baseline build failed with "No space left on device"; nothing of mine accounted for the rest). The worktree's build output was removed after the checks to give space back.

## Smoke (tmux `sync1022-smoke`, 120x40, release build, `--model haiku`, `CLAUDE_CONFIG_DIR=~/.claude-personal`, scratch Git repo /tmp/claude-rs-sync-1022)
- Trust prompt: upstream's layout showed `Path: /private/tmp/claude-rs-sync-1022`; Yes continued.
- Welcome: the fork's pixel crab animated while connecting (frames changed between captures), then showed "Subscription: Claude Team" and the session ID.
- Prompt: NOT answered. The personal account had hit its 5-hour limit ("Turn blocked by account or plan limits: You've hit your session limit · resets 5:40am (Asia/Calcutta)"; Usage tab: 5-hour 100% used). The blocked turn rendered as a turn failure, the composer stayed editable. So no live model reply and no live Read tool call were exercised; see the fake-bridge runs below.
- `/agents`: upstream's full-screen view, "Subagents -/0 / No subagents yet", Esc back.
- `/diff`: HEAD -> working tree listed `M notes.txt (+1)` with the hunk; Esc back.
- `/resume`: picker listed the current session (`[current] ... [saved 338 KiB]`) with Tab / Ctrl+T / Ctrl+D hints; closed with Esc, nothing deleted.
- `/export /tmp/claude-rs-sync-1022/export.md`: "Exported the conversation to ..."; the file holds the session header, the prompt and the limit reply.
- Footer: `[Auto] · [Haiku 5.5] · [Med] · [FAST:OFF]` left, `Loc`, `Branch: main`, `Context: 94%` right, the owner's `statusLine` row below. After opening the Usage tab the fork's `rateLimits` field showed `Usage: 5h 100% 7d 33%`; after a Status-tab rename to "sync smoke" the fork's `sessionTitle` field showed it and Status showed "Session name: sync smoke". Status also showed the fork's startup timeline and "Hooks this session".
- Exit: Ctrl+Q, exit status 0, resume hint printed; claude-rs, its bridge and the bridge's children (claude, MCP servers) all gone.
- Fake bridge, release binary: `FAKE_BRIDGE_SCENARIO=showcase` (as `recordings/showcase.sh` sets it, own HOME and pre-trusted profile) showed the crab welcome with `Cwd: ~/demo-project`, and `/mcp` listed `github connected` / `filesystem connected` plus the needs-auth, pending, failed and disabled rows. `FAKE_BRIDGE_SCENARIO=resize-read` rendered upstream's numbered Read previews (`Read read-107.rs`, `... 100 lines hidden ...`). `recordings/showcase.sh` itself was not run (it needs VHS and a debug build).
- Cleanup: my tmux sessions killed, scratch dir, `~/.claude-personal/projects/-private-tmp-claude-rs-sync-1022` and the empty `session-env/<session id>` removed. The trust prompt added a trust entry for /private/tmp/claude-rs-sync-1022 to `~/.claude-personal/.claude.json`; left, as in the previous sync.

## Open items
- Live prompt and Read tool call against the model: blocked by the account limit; rerun after 05:40 IST.
- `app::update_install::tests::worker_records_a_failed_install_and_a_later_success_clears_it` fails under parallel load (see Checks). Untouched upstream test; fork-main baseline not measured (disk full).
- `docs/src/parity.md`, `docs/src/presentation-gaps.md` are stale.
