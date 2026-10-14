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
| `Shift+Enter`, `Ctrl+Enter`, `\\` then `Enter` | Insert newline. The backslash is removed. |
| `Esc` | Cancel the active assistant turn. Press twice on an empty prompt to open the rewind picker. |
| `Ctrl+C` | Clear the local draft, or quit when the draft is empty. |
| `Ctrl+D` | Delete after the cursor, or quit when the draft is empty. |
| `Ctrl+G` | Edit the draft in `$VISUAL` or `$EDITOR`. A failed editor preserves the draft. |
| `Ctrl+O` | Show a snapshot of retained user, assistant and visible tool text; press again or `Esc` to close. Reopen to refresh. |
| `Ctrl+T` | Show Plan, Agents and Other tasks. Up/Down selects an agent; Enter opens its inspector. Press again or `Esc` to close. |
| `Alt+R` | Open the rewind picker directly. |
| `Ctrl+F` | Search retained transcript text. |
| `Tab` | Focus prompts or accept suggestions. |
| `Shift+Tab` | Cycle mode. |
| `Up` | Move up through text, or recall the latest user message when the input is empty. |
| `Down`, `Left`, `Right` | Move through text. |
| `Home`, `End` | Move to line start or line end. |
| `Ctrl+Left`, `Ctrl+Right` | Move by word. |
| `Alt+Left`, `Alt+Right` | Move by word. |
| `Ctrl+Backspace`, `Ctrl+Delete` | Delete by word. |
| `Alt+Backspace`, `Alt+Delete` | Delete by word. |

Readline-style bindings are also supported:

| Shortcut | Action |
| --- | --- |
| `Ctrl+A`, `Ctrl+E` | Move to line start or line end. |
| `Ctrl+B` | Move one character left. |
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
