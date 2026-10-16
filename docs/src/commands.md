# Slash Commands

Claude Code Rust has app-owned slash commands and can also show slash commands advertised by the active Agent SDK session. App-owned commands are available from the Rust TUI itself; SDK-advertised commands depend on the current session and what the bridge reports.

Use `/docs commands` in the app to render the live merged command list into chat. That is the source to use when you want to know exactly which app-owned and SDK-advertised commands are available in the current session.

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
| `/plugins` | `/plugins` | Open plugin management. |
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

## Resuming sessions

`/resume` opens the same fullscreen session picker used by `claude-rs resume` at startup. Type to filter by session name, session ID, or branch name; search matches partial text without case sensitivity, and pasted text also searches. Up/Down selects a matching session, Enter resumes it, and Escape closes the picker without switching sessions. Backspace edits the search. The scrollable list shows the SDK-provided title, last activity, branch when available, session ID, and a marker for the current session. All available sessions for the project directory are searchable.

Refreshing session metadata keeps the selected session by ID when it still matches. If it disappears or stops matching, the next result at that position is selected, or the previous result at the end. An empty result list stays open and cannot resume an unrelated session; edit the search or press Escape. Right activates the `fork` action and opens the message picker; Left/Escape returns to the filtered session list. Message previews wrap to the terminal width, preserve paragraph breaks, and show up to four text lines with a `more` indicator for longer messages. Up/Down selects a message, and Enter forks before it. Right opens the selected message for full-text reading with Up/Down scrolling; Left/Escape returns to the message list.

`/resume` asks the SDK for the session list each time it opens, so the active session and sessions other processes wrote are listed; until that answer arrives only Escape works. Each row shows the session's saved size and its SDK tag. Tab resumes a session of 1 MiB or more and compacts it before the first prompt. Ctrl+T edits the selected session's tag (Enter saves, an empty tag clears it, Ctrl+U empties the field) and Ctrl+D deletes the selected session after `y` confirms; the active session cannot be deleted.

Session selection happens inside the picker; `/resume` has no inline session suggestions or chat arguments. Ordinary command-name completion still completes `/resu` to `/resume`. For direct resume from the terminal, use `claude-rs resume <session_id>`, or an exact session title; a title that names several sessions is refused with their IDs.

Resumed history displays saved Claude command records as readable commands, such as `/rename review-444`, and command output as notices. Internal command context, compaction summaries, and background-task notifications are excluded from the fork list. Ordinary conversation XML remains literal, and saved session content is unchanged.

## Reviewing Git changes

`/diff` opens a fullscreen, read-only Git viewer for the repository containing the current directory. The left pane shows changed files in an expanded tree; folders are not selectable. Each file shows its status and line counts, with additions in green and removals in red; zero counts are hidden. Totals appear above the tree. The right pane previews the selected file. Binary files, submodules, unavailable files, clean repositories, and Git read failures are shown explicitly. Repositories without a first commit are supported.

Press `s` to choose a comparison with Up/Down and Enter; Escape cancels. HEAD compares the last commit with current files, Staged compares the last commit with the Git index, and Unstaged compares the index with current files. HEAD and Unstaged also include untracked files that Git does not ignore. This view shows repository changes, not session or turn history.

Refresh keeps the current pane focused and retains the selected change when it remains available. If that change disappears, the next remaining file takes its place, or the previous file when the selection was at the end. If no changes remain, the viewer clears the old preview and stays open with an empty-state message.

| Key | Action |
| --- | --- |
| Up/Down | Select files in the left pane or scroll content in the right pane. |
| Enter or Right | Focus the content pane. |
| Left or Escape in the content pane | Return focus to the file tree. |
| Tab / Shift+Tab | Select the next/previous file without changing the focused pane. |
| `v` | Toggle the diff and full file contents. Staged shows the indexed version; deleted files show their contents before deletion. |
| `[` | Jump to the previous diff hunk (changed block). |
| `]` | Jump to the next diff hunk (changed block). |
| `r` | Refresh, retaining the selected file when it remains in the comparison. |
| Escape in the file tree | Return to the conversation. |
| Ctrl+C | Close the viewer and return to the conversation. |
| Ctrl+Q | Quit the app. |

## Copying responses

`/copy` copies the last completed assistant response as its original Markdown, without thinking or tool output. Responses loaded from session history can be copied too. If the response contains code blocks, choose Full response or a code block with Up/Down, then Enter; Escape cancels. Enable Skip the /copy picker in `/config` to copy the complete Markdown response directly. Responses without code blocks copy directly. If the clipboard is unavailable, the picker stays open so you can retry. The command can copy an earlier completed response while a new response is still running; it does not submit a model request.

`/diff` opens a fullscreen file list. It combines diff content still retained in the current session's tool calls with the current Git working tree; the latter includes staged, unstaged, and untracked files. Choose a file with Up/Down, scroll its diff with Page Up/Page Down, refresh with `r`, and close with Escape. Git reads have bounded time and output; binary and oversized untracked files show an explicit preview message. Session-only entries reflect retained tool diff content, not a complete archive of every edit.

## Reloading skills and output styles

`/reload` asks the active session to re-read skills and output styles from disk, so a skill or style you just edited applies without restarting. The input is blocked until the SDK answers, then a notice reports how many skills and output styles it found, or the error. The slash command list picks up new or changed skills through the SDK's own command-list update. `/reload` does not reload plugins; use the Plugins tab for that.

## Exporting the conversation

`/export` writes the session's complete saved history to a Markdown file. The history comes from the Agent SDK's saved transcript, not from the chat on screen, so it includes messages the app no longer keeps in memory. Without a path the file is `export-<session-id>-<YYYYMMDDTHHMMSSZ>.md` in the session directory, using the full session id and the UTC export time; if that name is taken it becomes `-2`, `-3`, ... before `.md`. `/export <path>` writes there instead: a relative path is relative to the session directory, a leading `~` is your home directory, and spaces in the path are kept.

The file starts with the session title, id, directory, model and export time. Your messages and the assistant's text are written as they were. Each tool call is a heading with the tool name and a short summary of its input, followed by its result in a fenced block and, for edits and new files, a `diff` block. Long results and diffs are cut with a note, and terminal colour codes are removed. A session too large to send at once is cut from its end, and the file says how many later entries were left out.

`/export` never overwrites a file and creates no directories: if an explicit target exists, or its directory does not, it says so and writes nothing. When it finishes it shows the absolute path it wrote.

## Side Questions

`/btw` treats the complete non-empty text after the command as one question, including internal spaces and line breaks. Side questions are shown in a separate status field while pending and produce a bordered `Claude · BTW` transcript card only when answered. During an active turn, the card is inserted at the current point in the agent's output, with subsequent output below it; when idle, it is a standalone transcript entry with the same design. You can continue using the normal composer while up to ten side questions are unresolved; Rust queues and dispatches them one at a time in submission order. Each question sees main-conversation context at SDK dispatch time, not a snapshot from submission. Failures free capacity immediately and remain visible briefly as error rows.

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
