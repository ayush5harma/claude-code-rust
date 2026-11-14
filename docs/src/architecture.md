# Architecture

Claude Code Rust is a terminal application with two local components: a Rust process owns the interface and application state; a TypeScript bridge translates between the app and Anthropic's Agent SDK. Read [Development](development.md) to run a checkout and [Testing](testing.md) to validate a change.

## Runtime Shape

```text
Keyboard / terminal events
           |
           v
Rust app state and event loop <----> Rust terminal rendering
           |
     NDJSON commands / events
           |
           v
TypeScript bridge <----> Claude Agent SDK <----> Claude services and tools
```

Rust starts a Tokio runtime and runs the app inside a `LocalSet`, which supports the non-`Send` parts of terminal and child-process handling. The bridge is a child process running under Bun. Newline-delimited JSON (NDJSON) carries one command or event envelope per line over its standard input and output.

Support commands such as `doctor`, CLI help, completions and manual generation take a separate path before an interactive session starts. Detailed launch resolution belongs in [Startup and launch settings](subsystems.md#startup-and-launch-settings).

## Follow one prompt

1. Startup resolves the working directory and launch arguments. The app handles trust, authentication and session selection before admitting an initial prompt.
2. The composer submits a prompt through the application's existing input and command path. The bridge client sends a command envelope over stdin.
3. The TypeScript bridge maps the command to the active SDK session. The SDK supplies model output, tool activity and interaction requests.
4. The bridge converts those results into typed events. Rust applies them to the relevant session state; permission and question requests become interactive UI.
5. Rendering projects the canonical transcript into terminal rows. Finished output enters terminal scrollback; the live reply, composer and footer remain in the active area.
6. A permission answer or other interaction travels back through the command path. Completion, cancellation and failure are handled by their lifecycle owners, rather than inferred from displayed text.

The invariant reference explains the identity checks, saved-history behavior and failure paths behind this flow. In particular, saved history must not cause another desktop notification, and a transport acknowledgement does not by itself prove that an operation completed.

## Rust Terminal App

| Area | Responsibility |
| --- | --- |
| `src/main.rs` | Process entrypoint, runtime, logging and exit behavior. |
| `src/lib.rs` | CLI arguments, subcommands and diagnostics presets. |
| `src/agent/` | Bridge resolution, process client, wire types and bridge errors. |
| `src/app/` | Application state, lifecycle, sessions, input, permissions, settings and trust. |
| `src/ui/` | Ratatui and Crossterm rendering of chat, tools, dialogs and fullscreen views. |

The normal conversation renders inline. Fullscreen views temporarily use the alternate screen and return to chat when closed. Transcript layout, insertion into terminal scrollback, and resize replay have separate responsibilities; see [Transcript rendering](subsystems.md#transcript-rendering) and [Resize replay](subsystems.md#resize-replay) before changing that path.

## Agent SDK Bridge

`agent-sdk/src/bridge.ts` and the modules under `agent-sdk/src/bridge/` adapt SDK commands, callbacks and history into the application's protocol. The runtime executes the compiled `agent-sdk/dist/bridge.js`; editing TypeScript requires rebuilding it.

The paired wire definitions are `src/agent/wire.rs` and `agent-sdk/src/types.ts`. Update both in one change when the transport contract changes. These definitions describe the internal Rust/bridge boundary, not a supported third-party SDK.

The SDK owns subprocess option serialization. The bridge forwards its argument array without reconstructing it. Assistant text and Markdown links are preserved across streaming, completed replies and saved history; structured citation objects are not transported to Rust as source references.

## Where a change belongs

| Change | Start here | Relevant coverage |
| --- | --- | --- |
| Launch flag or startup behavior | `src/lib.rs`, launch resolution, bridge session lifecycle | `tests/cli_startup.rs`, `tests/docs_cli_contract.rs` |
| Saved setting or structured editor | Bridge settings catalog/service; Rust config state and UI | Bridge settings tests, `tests/config_flow.rs` |
| Slash command or composer interaction | App command and input handling, bridge dispatch where needed | `tests/slash_input_flow.rs`, bridge command tests |
| Transcript layout, resizing or shutdown | UI layout and application terminal lifecycle | `tests/terminal_resize.rs` and rendering unit tests |
| SDK event or wire change | Bridge event projection and Rust conversion/consumer | Bridge contract tests plus the affected Rust workflow |
| Installer or package contents | `scripts/`, npm launcher and platform mapping | The `npm-package-layout` CI job; [Packaging](packaging.md) |

The [testing guide](testing.md) maps these areas to executable checks. For a cross-boundary change, follow the actual input through its consumer and observable outcome before choosing tests.

## Subsystems

Each link below owns the detailed rules for that concern.

| Concern | Detailed reference |
| --- | --- |
| MCP input requests | [Queue ownership, validation, settlement and teardown](subsystems.md#mcp-elicitation-ownership) |
| Settings and trust | [Catalog, scope resolution, writes, trust and updates](subsystems.md#settings-and-trust) |
| Presentation and timing | [Metadata provenance and formatting](subsystems.md#presentation-and-timing) |
| Transcript rendering | [Canonical content and resumable row production](subsystems.md#transcript-rendering) |
| Resize replay | [Preparation, insertion, cancellation and shutdown](subsystems.md#resize-replay) |
| Transcript reading | [Stable anchors and following policy](subsystems.md#transcript-reading) |
| Response copying | [Canonical text and clipboard boundary](subsystems.md#copy) |
| Turn activity | [SDK activity and presentation decisions](subsystems.md#turn-activity-and-thinking) |
| Background tools | [Run identity and lifecycle outcomes](subsystems.md#background-tool-execution) |
| Subagents | [Discovery, saved transcripts and stop requests](subsystems.md#subagents-view) |
| Session titles | [SDK title ownership and rename persistence](subsystems.md#session-title) |
| Notifications | [Delivery eligibility, deduplication and replay](subsystems.md#notifications) |
| Plugin attachments | [Reload, confirmation and session lifetime](subsystems.md#session-plugin-attachments) |

## Fork subsystem notes

The fork keeps these invariants beyond the subsystem reference. Each extends the section of [Subsystem ownership and invariants](subsystems.md) named in its heading.

### Session picker and listing (extends Startup and launch settings)

The session picker lists metadata returned by the public Agent SDK, including a saved tag. Tag edits and confirmed deletion use `tagSession()` and `deleteSession()` against the exact selected session ID and SDK-reported project directory. The bridge rejects deletion of a live session; mutation results carry a request ID and a fresh SDK list, so the picker updates after acknowledgment rather than changing local metadata optimistically. Opening the picker with `/resume` sends a `list_sessions` request under its own request ID and shows a loading state until that listing arrives, because the list the bridge sends when a session connects cannot include the active session once it has a transcript, nor sessions another process wrote since; the answer also puts the selection on the active session, which the list marks `[current]`.

### Flag settings (extends Settings and trust)

A `--settings` value is the session's flag-settings layer. The SDK accepts one such layer, as a path or an object, so the bridge parses the user's JSON and lays its own host restrictions over it (`feedbackDrafts: "off"`, and `crossSessionInbound: "refuse"` unless the flag value or the saved cascade says `accept` or `refuse`); Rust only reads the file text. The Config snapshot does not include this layer.

### Read output and shell colour (extends Presentation and timing)

A successful text Read reaches Rust as the file's own lines plus `output_metadata.read` with the number of the first one, taken by the bridge from the SDK's structured result rather than from the line-numbered text the model sees. Rust shows one source line per row behind a dim line-number gutter, wraps long lines under the text column, applies the head/tail cap to source lines before wrapping, and never parses line numbers back out of text. Markdown files are shown as source. Search, the verbose view and export read the same text.

Shell output keeps the colour and style the program asked for, and nothing else. `src/ui/terminal_output.rs` is the one control-sequence filter: a single linear pass that maps SGR sequences (reset, bold, dim, italic, underline, their off codes, the 16 standard and bright colours, 256-colour and truecolour foreground and background) to styles and drops every other escape sequence and control character, including OSC titles and OSC 8 hyperlinks, cursor movement, erase, mode changes, DCS/APC strings and C1 controls. Unknown or malformed SGR parameters are dropped, and an SGR sequence over 128 bytes is ignored. Existing `NO_COLOR` handling remains in effect.

### Task rows (extends Background tool execution)

A task row's lifecycle state is separate from its tool call. A notification that follows a finished tool call still settles the row, and only a new start or another outcome changes a settled row. Every lifecycle edge (start, progress, update, notification, and a `TaskStop` result) marks its row `sdk_lifecycle_task` and clears the `sdk_background_task` marker of a row the inventory created. A background snapshot removes only rows that still carry the background marker, so a subagent keeps its row when it leaves `background_tasks_changed`, whether the outcome arrives before or after that snapshot. A `TaskList` snapshot replaces plan tasks while retaining runtime rows, and Rust never counts a lifecycle-marked row as a plan step.

### One agent inspector (extends Subagents view)

The view is the one agent inspector: the footer shelf is the glance, `Ctrl+T` gives the list, and `Enter` there opens the view on the same selection. It lists the active session's `local_agent` lifecycle rows from the task state Rust already holds, one per task id; there is no second task store and no polling. The footer shelf counts the rows the lifecycle reports as running. Answers for another session are ignored. A row has a transcript only when `listSubagents()` returned its task id, which is read when the view opens and on `r`. A transcript read never changes a row's status. A stop request is offered only for a running row. Once it is sent the row shows stopping, its acknowledgement does not change that, and only the lifecycle reports it stopped. A rejected request is shown as an error and leaves the row as it was. The bridge refuses to stop a task it does not hold as a running subagent, and runs the request outside the session command lane because `stopTask()` can stay pending until its query ends.

### Agent progress and inspection (extends Subagents view)

The bridge normalizes task progress into the existing task metadata: `usage` (`total_tokens`, `tool_uses`, `duration_ms`), `last_tool_name`, `summary`, and the child's resolved `model` and `model_display`. Child assistant messages may precede task linkage; the bridge remembers their model by parent tool-use ID and emits a task update when the linked model changes. Background inventory snapshots preserve lifecycle-owned metadata and cannot erase a completed agent's history merely because membership changes arrive before its completion event.

Rust's task inventory remains the status authority. The footer shelf, the `Ctrl+T` list and the Agents view share the same agent-row projection and renderer, and the list and view share one selection by task id. Requested model aliases and agent-definition defaults remain distinguishable from a model observed in a child response. A bounded index of the last eight child tool-call IDs per parent is maintained on tool events and pruned with transcript retention; rendering reads this index without scanning the transcript or performing I/O. Local observation times extend reported running durations, and the visible shelf requests at most one redraw per second when no other animation is active.

The shelf includes running agents and agents finished during the current continuous delegation wave; the task list and the Agents view retain prior agent history.

### Launch name (extends Session title)

A launch name (`-n, --name`) travels in the launch settings and the bridge applies it, because only the bridge knows which SDK path fits: the SDK's `title` option for a new or forked session, whose transcript file does not exist until its first turn and so cannot be renamed yet, and `renameSession()` for a resumed session, before it connects, because the SDK keeps a resumed session's persisted title over `title`. A resumed session also passes its title (the launch name, or the saved title the SDK reports) as `title`, which on a resume only names the process that other clients list, for example in `claude agents`, and leaves the transcript alone. Either way the title reaches Rust through the same `getSessionInfo()` reads.

### Hook activity

| Concern | Owner | Location |
| --- | --- | --- |
| Asking the SDK for hook events (`includeHookEvents`) and normalising `hook_started`, `hook_progress` and `hook_response` into one `hook_activity` session update | Bridge | `agent-sdk/src/bridge/hook_activity.ts` |
| Hook runs, their durations, the notice rule and its threshold | Rust | `src/app/state/hook_runs.rs` |
| Which finished hooks leave a notice (`presentation.hookNotices`) | Bridge catalog, Rust reads the snapshot | `settings_catalog.ts`, `src/app/events/hooks.rs` |
| Launch milestones and their placement on one timeline | Rust | `src/app/state/startup_timeline.rs` |

The bridge forwards what the SDK declares: hook id, name and event, phase, outcome, exit code and a bounded output excerpt (stderr for a failure, combined output otherwise). It reads no hook files or settings. The SDK declares no hook timing, so each update carries the epoch milliseconds at which the bridge saw it, and Rust derives a duration from the started and finished observations. SessionStart hooks finish before the SDK answers its initialize request, and Rust drops session updates for a session it does not know yet, so the bridge holds hook updates until it sends the connect event and sends them right after it.

Rust keeps the last 200 runs of the active session and clears them when the session id changes or the conversation resets. A run that finishes is judged once for a notice. Until the first settings snapshot is acknowledged it waits, so startup never decides from a guessed preference. The Startup section of the Status tab places the bridge's wall-clock stamps on the monotonic timeline through the wall-clock reading taken at app start, and counts only SessionStart hooks that finished before the connect.

## Boundaries

Claude Code Rust owns the terminal UI, local settings surface, bridge process management and event rendering. Anthropic owns the SDK, authentication and service behavior, billing, models and upstream Claude Code semantics.

The application uses its local TypeScript bridge as the SDK integration point. SDK package subpath exports such as `/browser`, `/bridge` and `/assistant` are not the application's runtime entry point.

## Related maintainer guides

- [Packaging and release invariants](packaging.md): artifact layout, generation and verification.
- [Governance](governance.md): repository protection and publication authority.
- [Test and validate changes](testing.md): checks for each affected area.
