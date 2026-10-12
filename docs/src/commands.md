# Slash Commands

Claude Code Rust has app-owned slash commands and can also show slash commands advertised by the active Agent SDK session. App-owned commands are available from the Rust TUI itself; SDK-advertised commands depend on the current session and what the bridge reports.

Use `/docs commands` in the app to render the live merged command list into chat. That is the source to use when you want to know exactly which app-owned and SDK-advertised commands are available in the current session.

## App-Owned Commands

| Command | Usage | Purpose |
| --- | --- | --- |
| `/btw` | `/btw <question>` | Ask a contextual side question without adding it to the main conversation. |
| `/cancel` | `/cancel` | Cancel the active assistant turn. |
| `/compact` | `/compact` | Ask the active session to compact conversation context. |
| `/config` | `/config` | Open fullscreen settings. |
| `/memory` | `/memory` | Open the Memory settings pane. |
| `/permissions` | `/permissions` | Open saved permission defaults, rules and directories. |
| `/sandbox` | `/sandbox` | Open sandbox settings without toggling sandboxing. |
| `/hooks` | `/hooks` | Inspect and edit saved hook configuration without executing hooks. |
| `/copy` | `/copy` | Copy the last completed response, or choose a code block from it. |
| `/search` | `/search [query]` | Search retained user, assistant, and visible tool text. |
| `/diff` | `/diff` | Browse retained session tool diffs and current Git working-tree changes. |
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
| `/agents` | `/agents` | Inspect current-session subagents, their saved messages and running tasks. |
| `/login` | `/login` | Run Claude CLI authentication and reconnect the session. |
| `/logout` | `/logout` | Run Claude CLI logout and clear the active authenticated session. |
| `/mode` | `/mode <id>` | Switch to a mode advertised by the active session. |
| `/model` | `/model <id>` | Switch to a model advertised by the active session. |
| `/new-session` | `/new-session` | Start a fresh bridge session in the current folder. |
| `/resume` | `/resume [session_id_or_title]` | Open the session picker, or resume an exact ID or displayed title. |
| `/rewind` | `/rewind <user_message_uuid> <both\|conversation\|code>` | Restore conversation, code, or both to a previous user message. |
| `/reload` | `/reload` | Re-read skills and output styles from disk for the active session. |
| `/export` | `/export [path]` | Save the complete conversation as a Markdown file. |

In the `/resume` picker, `t` edits the selected session's SDK tag; `Ctrl+U` clears the draft and Enter saves it (an empty tag clears the saved tag). `d` opens deletion confirmation for the selected ID, and only `y` confirms it. Esc cancels either action before submission. The active session cannot be deleted; switch to another session first. Tags and deletions persist through the public Agent SDK session APIs, and the picker refreshes after the SDK acknowledges them.

Enter completes the selected slash command or argument and submits it when ready. Tab completes without submitting. Exact command names appear ahead of substring matches; arrow keys can select a different match. App commands that still need required arguments stay in the composer; commands with argument choices show those choices, while `/btw` continues as ordinary question text. An empty suggestion menu allows Enter to submit the typed input for normal validation.

Editing the draft cancels a pending submission. Editing, completion, and submission keys cannot change or send the draft while pasted text is awaiting insertion, including when suggestions are open or keys are remapped. Suggestions refresh after the pasted text is inserted.

Completing the name of a command with no arguments or optional arguments closes the popup and returns focus to the draft. Optional arguments remain supported: `/resume` opens the session picker, and `/resume <session_id_or_title>` resumes directly. A title must match the displayed title exactly; if it names more than one session, the app asks for an ID. Pressing Tab again requests argument completion explicitly. When the SDK provides only an argument hint, Tab displays that hint without changing or sending the draft; Enter sends the typed command for SDK validation. SDK hints are descriptive text, so their brackets never determine whether the app allows submission. Empty hints and `none` or `[none]` do not produce argument help.

Space inserts a literal separator without accepting the highlighted suggestion. It ends command-name completion and continues argument assistance only when the app requires arguments, or when you explicitly requested argument help. There is no special Space dismissal state. Escape dismisses the current menu; argument edits, cursor movement within arguments, and metadata refreshes keep it closed. Tab requests completion again, and editing the command name starts a new completion interaction. A command is recognized only at the start of the draft, allowing leading whitespace; slashes on later lines remain literal prompt or argument text.

The command menu includes the full merged inventory and scrolls as you move through it. SDK commands remain available after `/clear` changes the session identifier. In SDK sessions that advertise `/clear [name]`, `/new` and `/reset` are aliases; the optional name labels the conversation you are leaving. `/new-session` is the separate app command that starts a fresh bridge session. App command definitions take precedence when the SDK advertises the same name.

## Copying responses

`/copy` copies the last completed assistant response as its original Markdown, without thinking or tool output. Responses loaded from session history can be copied too. If the response contains code blocks, choose Full response or a code block with Up/Down, then Enter; Escape cancels. Enable Skip the /copy picker in `/config` to copy the complete Markdown response directly. Responses without code blocks copy directly. If the clipboard is unavailable, the picker stays open so you can retry. The command can copy an earlier completed response while a new response is still running; it does not submit a model request.

`/diff` opens a fullscreen file list. It combines diff content still retained in the current session's tool calls with the current Git working tree; the latter includes staged, unstaged, and untracked files. Choose a file with Up/Down, scroll its diff with Page Up/Page Down, refresh with `r`, and close with Escape. Git reads have bounded time and output; binary and oversized untracked files show an explicit preview message. Session-only entries reflect retained tool diff content, not a complete archive of every edit.

## Reloading skills and output styles

`/reload` asks the active session to re-read skills and output styles from disk, so a skill or style you just edited applies without restarting. The input is blocked until the SDK answers, then a notice reports how many skills and output styles it found, or the error. The slash command list picks up new or changed skills through the SDK's own command-list update. `/reload` does not reload plugins; use the Plugins tab for that.

## Exporting the conversation

`/export` writes the session's complete saved history to a Markdown file. The history comes from the Agent SDK's saved transcript, not from the chat on screen, so it includes messages the app no longer keeps in memory. Without a path the file is `claude-rs-<session title or short id>-<YYYYMMDD-HHMMSS>.md` in the session directory. `/export <path>` writes there instead: a relative path is relative to the session directory, a leading `~` is your home directory, and spaces in the path are kept.

The file starts with the session title, id, directory, model and export time. Your messages and the assistant's text are written as they were. Each tool call is a heading with the tool name and a short summary of its input, followed by its result in a fenced block and, for edits and new files, a `diff` block. Long results and diffs are cut with a note, and terminal colour codes are removed. A session too large to send at once is cut from its end, and the file says how many later entries were left out.

`/export` never overwrites a file and creates no directories: if the target exists, or its directory does not, it says so and writes nothing. When it finishes it shows the absolute path it wrote.

## Side Questions

`/btw` treats the complete non-empty text after the command as one question, including internal spaces and line breaks. Side questions are shown in a separate status field while pending and produce a bordered `Claude · BTW` transcript card only when answered. During an active turn, the card is inserted at the current point in the agent's output, with subsequent output below it; when idle, it is a standalone transcript entry with the same design. You can continue using the normal composer while up to ten side questions are unresolved; Rust queues and dispatches them one at a time in submission order. Each question sees main-conversation context at SDK dispatch time, not a snapshot from submission. Failures free capacity immediately and remain visible briefly as error rows.

## SDK-Advertised Commands

The active SDK session can advertise additional slash commands. These are not documented as a fixed table here because they can change with SDK behavior, session capabilities, account state, and future upstream changes.

Use:

```text
/docs commands
```

to inspect the current session's full command list. The output includes app-owned commands and SDK-advertised commands, with descriptions when the SDK provides them.

Commands that Claude Code runs locally, such as `/rename`, `/color` or `/context`, reply in the chat under the command as they do in Claude Code. `/color` runs in Claude Code and its reply shows, but Claude Rust does not draw a session colour.

`/search` and the remappable `Ctrl+F` shortcut open a local search viewer. Type or paste a query, use Enter or Down for the next match and Shift+Enter or Up for the previous match, and press Esc to return to chat. The highlighted excerpt is from retained session content only; older content evicted by the app's history limit and archived transcripts are not searched.

Rewinding a conversation keeps the model, effort and permission mode currently selected in the session. On resume, the public Agent SDK can return a local command as XML-like user text without identifying its origin. The history preserves that text exactly, so a command such as `/rename` may appear in its raw record form until the SDK exposes a reliable command marker.

## Status Tab

`/status` opens the Status tab. Besides the session, account, model and settings rows it has two measured sections.

**Startup** lists the launch milestones the app observed, as offsets from the moment the app started: the bridge launch, the `SessionStart` hooks that finished before the session connected (with their count), the SDK connection, and the first frame in which the composer accepts a prompt. A milestone that has not happened is not shown, and nothing is estimated. The offsets include any wait on the workspace trust prompt, because the bridge starts only after trust is accepted.

**Hooks this session** gives the number of hook runs, how many failed, how many are still running, their total time, and the three slowest. It covers the last 200 runs of the active session and says how many older runs are not kept. It starts empty again when the session changes or the conversation is cleared. Hooks of the same event and matcher share one SDK name, so two runs of different commands can look alike here.

## Session Name

The Status tab shows the session name. Its rename draft (`r`) starts from that name, and title generation (`g`) uses it as the description when there is one.

The name is whatever the Agent SDK's session API reports as the session's title; claude-rs keeps no title of its own. It is read when a session connects or resumes, when the session is replaced, when the conversation resets, after each turn, and after a Status-tab rename or title generation. A `/rename` typed in the composer therefore shows once its turn completes. For a session nobody named, the API reports a generated title instead, so such a session shows that title after its first turn.

If the SDK omits a saved custom title while resuming, claude-rs restores that title through the SDK session API so other clients see it too.

## Session Commands

`/ultracode on` enables session-scoped dynamic-workflow orchestration while retaining the current thinking effort; `/ultracode off` disables it. Both require an idle turn. `/ultracode status` can be used during a turn and reports the latest verified SDK snapshot: on, available and off, requested but unavailable, unavailable and off, or unknown. Availability depends on SDK session capabilities and model support. The footer shows `Ultracode` only when it is verified as effective. Changing effort preserves active Ultracode, and changing models refreshes its state. This command does not persist a preference or control the one-turn `ultracode` keyword trigger.

If the SDK accepts `/ultracode on` but reports Ultracode unavailable, the command shows an error explaining that the request was saved but Ultracode remains inactive. The verified requested-but-unavailable state remains visible through `/ultracode status`, which reports it as information.

`/model`, `/mode`, `/effort`, `/thinking`, `/agent`, `/fast`, and `/ultracode` change the current session; `/config` edits saved defaults for new sessions. Choices are checked against SDK capabilities and acknowledged only after the SDK accepts them. Model, effort, fast mode, and Ultracode show reported runtime results, including caps or fallbacks, rather than echoing requests. The starting permission mode comes from SDK initialization, before the first prompt. Saved permission defaults remain inherited by the native session. Both `/mode` and the mode-cycle shortcut keep the confirmed mode visible while awaiting SDK acceptance; a rejected change leaves it unchanged.

`/effort reset` uses the current model's native session default; it does not reload the saved per-model effort. `max` is session-only. `/thinking reset` removes the temporary override and restores the inherited thinking preference. Thinking is a preference, and the model may still require or restrict thinking. `/agent reset` clears the active main-thread agent rather than restoring its saved default.

`/fast` changes fast mode only for the active session. It does not rewrite the persisted Fast mode setting. Use the settings surface to choose the fast-mode preference applied when future sessions start. If an accepted change cannot be verified, the footer shows `FAST:?`; retry explicitly with `/fast on` or `/fast off`.

`/agents` shows subagents from the current SDK session. Up/Down selects one, `r` refreshes the list, and `s` asks the SDK to stop a selected running subagent task. Completed agents remain inspectable. The panel reads saved agent messages on selection and does not stop tasks outside the current session.
