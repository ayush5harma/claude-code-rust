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

As in Claude Code, `Left` on an empty prompt opens the agent view: claude-rs hands the terminal to `claude agents` (the stock list of background sessions, where you can peek, attach, reply, dispatch and stop them) and comes back when it exits. Press `Esc`, or `Ctrl+C` twice, in the agent view to return. Your claude-rs session keeps running while the agent view is open; anything it asks you waits in the transcript until you return.

claude-rs runs the same Claude Code the session runs (`CLAUDE_CODE_EXECUTABLE`), or `claude` from `PATH` when that is not set. While the input is empty, the footer's first row shows Claude Code's background sessions after the mode badges, for example `← 2 agents · 1 awaiting input · 1 working`, refreshed every 10 seconds and right after the agent view closes. It is hidden when there are none or when `claude agents --json` fails.

Claude Code's `leftArrowOpensAgents` setting (in its global config, `.claude.json`, also under `/config` in Claude Code) turns both off when it is `false`: `Left` then only moves the cursor and the footer shows no agent status. The action is `app.open_agents_or_move_left`; bind it to another key to open the agent view from an empty prompt with that key instead. `Ctrl+B` always moves left. claude-rs does not take over `/agents`: that command stays Claude Code's subagent configuration.

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
