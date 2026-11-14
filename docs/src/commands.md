# Slash Commands

Claude Code Rust has app-owned slash commands and can also show slash commands advertised by the active Agent SDK session. App-owned commands are available from the Rust TUI itself; SDK-advertised commands depend on the current session and what the bridge reports.

Use `/docs commands` in the app to render the live merged command list into chat. That is the source to use when you want to know exactly which app-owned and SDK-advertised commands are available in the current session.

For step-by-step use of resume, fork, export, copy and Git review, see [Work with conversations](conversations.md). This page owns command syntax, completion and current-session controls.

## App-Owned Commands

| Command | Usage | Purpose |
| --- | --- | --- |
| `/btw` | `/btw <question>` | Ask a contextual side question without adding it to the main conversation. |
| `/cancel` | `/cancel` | Cancel the active assistant turn. |
| `/exit` | `/exit` | Exit the application and restore the terminal. |
| `/compact` | `/compact` | Ask the active session to compact conversation context. |
| `/config` | `/config` | Open fullscreen settings. |
| `/memory` | `/memory` | Open the Memory settings pane. |
| `/permissions` | `/permissions` | Open saved permission defaults, rules and directories. |
| `/sandbox` | `/sandbox` | Open sandbox settings without toggling sandboxing. |
| `/hooks` | `/hooks` | Inspect and edit saved hook configuration without executing hooks. |
| `/copy` | `/copy` | Copy the last completed response, or choose a code block from it. |
| `/search` | `/search [query]` | Search retained user, assistant, and visible tool text. |
| `/diff` | `/diff` | Review Git changes and full file contents without changing files or the index. |
| `/docs` | `/docs <mode\|models\|shortcuts\|commands\|agents>` | Render command, shortcut, model, mode, or subagent help into chat. |
| `/agent` | `/agent <name\|reset>` | Change the main-thread agent for the active session. Applies on the next turn. |
| `/effort` | `/effort <low\|medium\|high\|xhigh\|max\|reset>` | Change thinking effort for the active session. |
| `/thinking` | `/thinking <on\|off\|reset>` | Set the thinking preference for the active session or restore its inherited preference. |
| `/ultracode` | `/ultracode <on\|off\|status>` | Enable, disable, or inspect Ultracode for the active session. |
| `/fast` | `/fast [on\|off]` | Toggle fast mode or explicitly enable/disable it for the active session. |
| `/help` | `/help` | Open the fullscreen Help tab. |
| `/mcp` | `/mcp` | Open MCP status and authorization. |
| `/plugins` | `/plugins` | Manage installed plugins, marketplaces and session-only local folders. |
| `/status` | `/status` | Open session and account status. |
| `/usage` | `/usage` | Open quota and usage information. |
| `/agents` | `/agents` | Inspect this session's subagents: live status, the selected agent's model, usage, activity and prompt, and saved transcripts. |
| `/login` | `/login` | Run Claude CLI authentication and reconnect the session. |
| `/logout` | `/logout` | Run Claude CLI logout and clear the active authenticated session. |
| `/mode` | `/mode <id>` | Switch to a mode advertised by the active session. |
| `/model` | `/model <id>` | Switch to a model advertised by the active session. |
| `/new-session` | `/new-session` | Start a fresh bridge session in the current folder. |
| `/resume` | `/resume` | Search and select a session in the fullscreen picker, refreshed from the SDK each time it opens. Tab resumes a large session compact-first; Ctrl+T tags and Ctrl+D deletes the selected session. |
| `/rewind` | `/rewind <user_message_uuid> <both\|conversation\|code>` | Restore conversation, code, or both to a previous user message. |
| `/reload` | `/reload` | Re-read skills and output styles from disk for the active session. |
| `/export` | `/export [path]` | Save the complete conversation as a Markdown file. |

In the `/resume` picker, `t` edits the selected session's SDK tag; pasting inserts at the end of the draft as one line (line breaks become spaces), `Ctrl+U` clears the draft and Enter saves it (an empty tag clears the saved tag). `d` opens deletion confirmation for the selected ID, and only `y` confirms it. Esc cancels either action before submission. The active session cannot be deleted; switch to another session first. Tags and deletions persist through the public Agent SDK session APIs, and the picker refreshes after the SDK acknowledges them.

Enter completes the selected slash command or argument and submits it when ready. Tab completes without submitting. Matches are ranked: the exact command name first, then names that start with what you typed, then names with a `:`, `-`, `_` or `/` separated part that starts with it, then any other substring match, each group in alphabetical order. Arrow keys can select a different match. App commands that still need required arguments stay in the composer; commands with argument choices show those choices, while `/btw` continues as ordinary question text. An empty suggestion menu allows Enter to submit the typed input for normal validation.

Editing the draft cancels a pending submission. Editing, completion, and submission keys cannot change or send the draft while pasted text is awaiting insertion, including when suggestions are open or keys are remapped. Suggestions refresh after the pasted text is inserted.

Completing the name of a command with no arguments or optional arguments closes the popup and returns focus to the draft. For commands accepting arguments, pressing Tab again requests argument completion explicitly. When the SDK provides only an argument hint, Tab displays that hint without changing or sending the draft; Enter sends the typed command for SDK validation. SDK hints are descriptive text, so their brackets never determine whether the app allows submission. Empty hints and `none` or `[none]` do not produce argument help.

Space inserts a literal separator without accepting the highlighted suggestion. It ends command-name completion and continues argument assistance only when the app requires arguments, or when you explicitly requested argument help. There is no special Space dismissal state. Escape dismisses the current menu; argument edits, cursor movement within arguments, and metadata refreshes keep it closed. Tab requests completion again, and editing the command name starts a new completion interaction. A command is recognized only at the start of the draft, allowing leading whitespace; slashes on later lines remain literal prompt or argument text.

The command menu includes the full merged inventory and scrolls as you move through it. SDK commands remain available after `/clear` changes the session identifier. In SDK sessions that advertise `/clear [name]`, `/new` and `/reset` are aliases; the optional name labels the conversation you are leaving. `/new-session` is the separate app command that starts a fresh bridge session. App command definitions take precedence when the SDK advertises the same name.

## Reloading skills and output styles

`/reload` asks the active session to re-read skills and output styles from disk, so a skill or style you just edited applies without restarting. The input is blocked until the SDK has answered both reloads, output styles first, then a single notice reports how many skills and output styles it found. The two halves are reported separately: one that fails shows its error next to the count for the other, and one that does not answer within 10 seconds is reported as unanswered (it may still apply) instead of as a failure. Before it runs, `/reload` waits for earlier requests for the same session to finish. If they take longer than 10 seconds from when `/reload` reached the bridge, whether stalled or just slow, `/reload` is skipped, nothing is reloaded, and a notice says so; it does not run later when the earlier request finishes. Once started, each half is given 10 seconds, so the input can stay blocked for up to 30 seconds in all; either way it is released. The slash command list picks up new or changed skills through the SDK's own command-list update. `/reload` does not reload plugins; use the Plugins tab for that.

## Session picker additions

These fork additions extend the picker described in [Work with conversations](conversations.md#resuming-sessions).

`/resume` asks the SDK for the session list each time it opens, so the active session and sessions other processes wrote are listed; until that answer arrives only Escape works. Each row shows the session's saved size and its SDK tag. Tab resumes a session of 1 MiB or more and compacts it before the first prompt. Ctrl+T edits the selected session's tag (Enter saves, an empty tag clears it, Ctrl+U empties the field) and Ctrl+D deletes the selected session after `y` confirms; the active session cannot be deleted.

Session selection happens inside the picker; `/resume` has no inline session suggestions or chat arguments. Ordinary command-name completion still completes `/resu` to `/resume`. For direct resume from the terminal, use `claude-rs resume <session_id>`, or an exact session title; a title that names several sessions is refused with their IDs.

Resumed history displays saved Claude command records as readable commands, such as `/rename review-444`, and command output as notices. Internal command context, compaction summaries, and background-task notifications are excluded from the fork list. Ordinary conversation XML remains literal, and saved session content is unchanged.

## SDK-Advertised Commands

The active SDK session can advertise additional slash commands. These are not documented as a fixed table here because they can change with SDK behavior, session capabilities, account state, and future upstream changes.

Use:

```text
/docs commands
```

to inspect the current session's full command list. The output includes app-owned commands and SDK-advertised commands, with descriptions when the SDK provides them.

Commands that Claude Code runs locally, such as `/rename`, `/color` or `/context`, reply in the chat under the command as they do in Claude Code. The composer edge and prompt glyph use a stable color selected from the session ID. An explicit `/color <name>` changes that accent only when the SDK confirms a successful local `/color` result with the submitted prompt ID. If the SDK omits either confirmation field, claude-rs keeps the current accent; it never infers color from chat text. `/color default` restores the session-ID color. The accent resets when the session changes and does not recolor tool status indicators.

`/search` and the remappable `Ctrl+F` shortcut open a local search viewer. Type or paste a query, use Enter or Down for the next match and Shift+Enter or Up for the previous match, and press Esc to return to chat. The highlighted excerpt is from retained session content only; older content evicted by the app's history limit and archived transcripts are not searched.

Rewinding a conversation keeps the model, effort and permission mode currently selected in the session.

## Status Tab

`/status` opens the Status tab. Besides the session, account, model and settings rows it has two measured sections.

**Startup** lists the launch milestones the app observed, as offsets from the moment the app started: the bridge launch, the `SessionStart` hooks that finished before the session connected (with their count), the SDK connection, and the first frame in which the composer accepts a prompt. A milestone that has not happened is not shown, and nothing is estimated. The offsets include any wait on the workspace trust prompt, because the bridge starts only after trust is accepted.

**Hooks this session** gives the number of hook runs, how many failed, how many are still running, their total time, and the three slowest. It covers the last 200 runs of the active session and says how many older runs are not kept. It starts empty again when the session changes or the conversation is cleared. Hooks of the same event and matcher share one SDK name, so two runs of different commands can look alike here.

## Session Name

The Status tab shows the session name. Its rename draft (`r`) starts from that name, and title generation (`g`) uses it as the description when there is one.

The name is whatever the Agent SDK's session API reports as the session's title; claude-rs keeps no title of its own. It is read when a session connects or resumes, when the session is replaced, when the conversation resets, after each turn, and after a Status-tab rename or title generation. A `/rename` typed in the composer therefore shows once its turn completes. For a session nobody named, the API reports a generated title instead, so such a session shows that title after its first turn.

When claude-rs resumes a session it passes the session's saved title to the SDK as the process title, so the name other clients list for the running session (for example in `claude agents`) is the saved title and not a folder-derived default. The saved title in the transcript is not changed by this.

## Session Commands

`/ultracode on` enables session-scoped dynamic-workflow orchestration while retaining the current thinking effort; `/ultracode off` disables it. Both require an idle turn. `/ultracode status` can be used during a turn and reports the latest verified SDK snapshot: on, available and off, requested but unavailable, unavailable and off, or unknown. Availability depends on SDK session capabilities and model support. The footer shows `Ultracode` only when it is verified as effective. Changing effort preserves active Ultracode, and changing models refreshes its state. This command does not persist a preference or control the one-turn `ultracode` keyword trigger.

If the SDK accepts `/ultracode on` but reports Ultracode unavailable, the command shows an error explaining that the request was saved but Ultracode remains inactive. The verified requested-but-unavailable state remains visible through `/ultracode status`, which reports it as information.

`/model`, `/mode`, `/effort`, `/thinking`, `/agent`, `/fast`, and `/ultracode` change the current session; `/config` edits saved defaults for new sessions. Choices are checked against SDK capabilities and acknowledged only after the SDK accepts them. Model, effort, fast mode, and Ultracode show reported runtime results, including caps or fallbacks, rather than echoing requests. The starting permission mode comes from SDK initialization, before the first prompt. Saved permission defaults remain inherited by the native session. Both `/mode` and the mode-cycle shortcut keep the confirmed mode visible while awaiting SDK acceptance; a rejected change leaves it unchanged.

`/effort reset` uses the current model's native session default; it does not reload the saved per-model effort. `max` is session-only. `/thinking reset` removes the temporary override and restores the inherited thinking preference. Thinking is a preference, and the model may still require or restrict thinking. `/agent reset` clears the active main-thread agent rather than restoring its saved default.

`/fast` changes fast mode only for the active session. It does not rewrite the persisted Fast mode setting. Use the settings surface to choose the fast-mode preference applied when future sessions start. If an accepted change cannot be verified, the footer shows `FAST:?`; retry explicitly with `/fast on` or `/fast off`.

`/agents` is the inspector for the active session's subagents and is not part of the settings surface. `Left` in an empty input opens it too, and `Enter` on an agent in the `Ctrl+T` task list opens it with that agent selected. Its list shows the status the SDK task lifecycle reports. The selected agent's details show the model its responses came from, else the model alias the session requested, else its agent definition's default; worktree isolation; tools, tokens and elapsed time; its current activity; up to eight recent child tools; and the first six lines of its prompt. A transcript is available once the SDK lists a saved transcript for the agent's task id; a running subagent can be missing from that list for its first seconds, and `r` checks again. `s` sends a stop request; the agent shows stopping until the session reports it stopped. While agents run, the footer shelf lists them. Keys are listed in [Keyboard Shortcuts](shortcuts.md#agents-view).

For subagent status, saved transcripts and stop requests, see [Read and stop subagents](conversations.md#read-and-stop-subagents).
