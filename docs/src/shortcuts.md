# Keyboard Shortcuts

Keyboard shortcuts are context-sensitive. Use `/docs shortcuts` in the app to show the live shortcuts for the current state.

## Global

| Shortcut | Action |
| --- | --- |
| `Ctrl+Q` | Quit. |
| `Ctrl+L` | Redraw. |
| `Ctrl+Z` on Unix | Suspend the process. |

In a fullscreen view, `Ctrl+C` closes the view and returns to chat. After shutdown begins, pressing `Ctrl+C` again forces any remaining cleanup to stop.

## Chat Reading

| Shortcut | Action |
| --- | --- |
| `Page Up` | Pause following and read earlier output. |
| `Page Down` | Read later output. |
| `Ctrl+End` | Jump to the latest output. Following resumes when Auto-scroll is On. |

See [Presentation and scrolling](settings.md#presentation-and-scrolling) for the Auto-scroll setting.

## Settings Surface

The fullscreen settings surface has its own keys for tabs, panes, search and editors. See [Opening and navigating](settings.md#opening-and-navigating), [Editing a value](settings.md#editing-a-value) and [Lists and structured settings](settings.md#lists-and-structured-settings).

## Chat Input

| Shortcut | Action |
| --- | --- |
| `Enter` | Submit. |
| `Shift+Enter`, `Ctrl+Enter` | Insert newline. |
| `Esc` | Cancel the active assistant turn. |
| `Ctrl+C` | Clear the local draft, or quit when the draft is empty. |
| `Tab` | Focus prompts or accept suggestions. |
| `Shift+Tab` | Cycle mode. |
| `Up` | Move up through text, or recall the latest user message when the input is empty. |
| `Left` | Open Claude Code's agent view when the input is empty; otherwise move left through text. |
| `Down`, `Right` | Move through text. |
| `Home`, `End` | Move to line start or line end. |
| `Ctrl+Left`, `Ctrl+Right` | Move by word. |
| `Alt+Left`, `Alt+Right` | Move by word. |
| `Ctrl+Backspace`, `Ctrl+Delete` | Delete by word. |
| `Alt+Backspace`, `Alt+Delete` | Delete by word. |

### Agent View

As in Claude Code, `Left` on an empty prompt opens the agent view: claude-rs hands the terminal to `claude agents` (the stock list of background sessions, where you can peek, attach, reply, dispatch and stop them) and comes back when it exits. Press `Esc`, or `Ctrl+C` twice, in the agent view to return. A `Ctrl+C` pressed while the view is still starting never quits claude-rs or its session: it closes the view, or is dropped if claude-rs had not handed over the terminal yet. Repeated `Left` presses open the view once.

Your claude-rs session keeps running while the agent view is open. A turn in progress continues and its output appears when you return. Permission prompts and questions wait in the transcript for you, but their timers keep running: a question with a configured timeout can time out and continue while the view is open. Notifications are still delivered as usual: the terminal can ring its bell or show a desktop notification over the agent view.

claude-rs runs the same Claude Code the session runs (`CLAUDE_CODE_EXECUTABLE`), or `claude` from `PATH` when that is not set. When `Left` would open the view, the footer's first row shows Claude Code's background sessions after the mode badges, for example `← 2 agents · 1 awaiting input · 1 working`: the sessions `claude agents --json` lists, without completed ones. claude-rs reads them from Claude Code's own files in its config directory (`jobs/*/state.json`, the daemon roster and the session registry) every 10 seconds and right after the agent view closes, re-reading only files that changed, so the status costs a few file checks rather than a Claude Code process. If those files are in a layout claude-rs does not recognise, it runs `claude agents --json` instead, at most once a minute. The status is hidden when there are none, when neither source can be read, when the input is not empty, while a permission prompt or question has focus, and while the input is unavailable.

Claude Code's `leftArrowOpensAgents` setting (in its global config, `.claude.json`, also under `/config` in Claude Code) turns both off when it is `false`: `Left` then only moves the cursor and the footer shows no agent status. claude-rs re-reads it when the file changes, so a change applies within 10 seconds. The action is `app.open_agents_or_move_left`; bind it to another key to open the agent view from an empty prompt with that key instead, and the footer then names that key in place of `←`. `Ctrl+B` always moves left. claude-rs does not take over `/agents`: that command stays Claude Code's subagent configuration.

Readline-style bindings are also supported:

| Shortcut | Action |
| --- | --- |
| `Ctrl+A`, `Ctrl+E` | Move to line start or line end. |
| `Ctrl+B`, `Ctrl+F` | Move one character. |
| `Ctrl+D` | Delete after cursor. |
| `Ctrl+H` | Delete before cursor. |
| `Ctrl+K` | Kill to line end. |
| `Ctrl+U` | Kill to line start. |
| `Ctrl+W` | Delete previous word. |
| `Ctrl+Y` | Yank. |
| `Alt+B`, `Alt+F` | Move by word. |
| `Alt+D` | Delete next word. |

## Undo And Redo

| Platform | Undo | Redo |
| --- | --- | --- |
| macOS | `Cmd+Z` | `Cmd+Shift+Z`, `Cmd+Y` |
| Windows | `Ctrl+Z` | `Ctrl+Shift+Z` |
| Unix except macOS | `Ctrl+_`, `Ctrl+/` | `Ctrl+Shift+Z` |

On Unix except macOS, `Ctrl+Z` is reserved for process suspend.

## Autocomplete

| Shortcut | Action |
| --- | --- |
| `Up`, `Down` | Move through candidates. |
| `Enter`, `Tab` | Accept the selected candidate. |
| `Esc` | Cancel autocomplete. |

## Inline Permissions

| Shortcut | Action |
| --- | --- |
| `Left`, `Up` | Move to the previous option. |
| `Right`, `Down` | Move to the next option. |
| `Enter` | Confirm the focused option. |
| `Esc` | Cancel. |
| `Tab` | Move focus. |

Letter shortcuts such as `Ctrl+A`, `Ctrl+Y`, or `Ctrl+N` are not permission shortcuts.

## Inline Questions

| Shortcut | Action |
| --- | --- |
| `Left`, `Up` | Move to the previous option. |
| `Right`, `Down` | Move to the next option. |
| `Home`, `End` | Move to first or last option. |
| `Space` | Toggle/select where applicable. |
| `Enter` | Submit. |
| `Esc` | Cancel. |
| `Tab` | Toggle notes or move focus. |
| `Shift+Tab` | Move focus backward. |
