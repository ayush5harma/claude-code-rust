# Usage

Start a new session in the current directory:

```bash
claude-rs
```

Start with an initial prompt:

```bash
claude-rs "Explain this project"
```

The prompt is sent automatically after project trust, startup dialogs, and session selection are complete. The TUI stays open for follow-up messages.

Start in a specific working directory:

```bash
claude-rs -C path/to/project
```

Resume a previous session:

```bash
claude-rs resume
```

Resume by session id:

```bash
claude-rs resume <session_id_or_title>
```

The Claude-style flags use the same resume workflow:

```bash
claude-rs --resume
claude-rs -r <session_id> "Continue reviewing the changes"
claude-rs --continue
claude-rs -c "Finish the review"
```

`--resume` without a value opens the session picker. `--continue` selects the most recently modified SDK-visible session for the working directory, including its git worktrees; this includes sessions created by `claude-rs`. Resume accepts an exact session ID or exact displayed title. IDs take priority; when multiple sessions share a title, use an ID. A launch whose session cannot be resumed (an unknown session, or a title shared by several, whose IDs are listed) exits with that reason instead of opening an empty session. Transcript-path lookup is not supported. Use `--resume=<session_id_or_title>` to make the value boundary explicit, or `--resume -- "prompt"` to open the picker with an initial prompt. `--resume`, `--continue`, and the `resume` subcommand cannot be combined.

The picker shows saved session size when the SDK supplies it. This is storage size, not context tokens. For a saved session of at least 1 MiB, press `c` to resume and request `/compact` before an initial prompt; Enter resumes normally. Compact-first may use a model and remains an explicit choice. If compaction fails, the initial prompt stays unsent and a draft already typed in the composer is preserved.

Choose how the initial session starts:

```bash
claude-rs --model opus --effort high --permission-mode plan "Review this project"
claude-rs --agent reviewer "Review the current changes"
claude-rs resume <session_id> --model sonnet --effort medium
```

`--model`, `--effort`, `--permission-mode`, and `--agent` apply to the initial new or resumed session before its first prompt. They do not write saved preferences. Later sessions started inside the TUI use saved settings. Model aliases, full model IDs, agent names, and model-specific effort availability are resolved by the SDK. Effort values are `low`, `medium`, `high`, `xhigh`, and `max`; `max` is a session-only override. Permission modes are `default`, `auto`, `acceptEdits`, `plan`, `dontAsk`, and `bypassPermissions`.

Name the session as you start it:

```bash
claude-rs -n "Release prep"
claude-rs --continue --name "Release prep, day two"
```

`-n, --name` names the initial session. A new or forked session takes the name as its title; a resumed or continued session is renamed before it connects, through the same SDK rename the Status tab uses. Control characters are removed, and a name with no visible characters is rejected. The SDK reports a new session's title after its first turn, so the Status tab shows the name from then on. Sessions started later inside the TUI are not renamed.

Branch from an existing conversation:

```bash
claude-rs --continue --fork-session
claude-rs -r <session_id> --fork-session -n "Try the other approach"
```

`--fork-session` requires `--resume`, `--continue` or the `resume` subcommand. The resumed history continues under a new session ID, which the app shows and uses from then on, including the resume hint after exit; the original session is left as it was. Like `--name`, it applies only to the initial session.

Configure every session the process starts:

```bash
claude-rs --add-dir ../shared-lib --add-dir ~/notes
claude-rs --settings ./ci-settings.json
claude-rs --settings '{"spinnerTipsEnabled": false}'
claude-rs --mcp-config ./mcp.json --mcp-config '{"mcpServers": {"docs": {"command": "docs-mcp"}}}'
claude-rs --mcp-config ./mcp.json --strict-mcp-config
claude-rs --append-system-prompt "Answer in German."
claude-rs --system-prompt-file ./release-bot.md
```

`--add-dir` gives Claude access to directories beyond the working directory. Unlike the options above, it applies to every session the process starts, including `/new` and sessions opened with `/resume`, as Claude Code keeps it for its whole process. Relative paths resolve against `--dir`, or the current directory without it, and each must be an existing directory; otherwise `claude-rs` exits with an error before starting.

`--settings` takes a JSON object inline (a value starting with `{`) or a path to a JSON file, read once at startup. It becomes Claude Code's flag-settings layer, which outranks the user, project and local settings files. Two host restrictions stay in force over it: `feedbackDrafts` is always `off`, and `crossSessionInbound` is `refuse` unless the value in effect is `accept` or `refuse`, because draft review and held peer-message review need surfaces `claude-rs` does not have. A file that cannot be read stops the launch; invalid JSON, or JSON that is not an object, fails the session start with a message naming `--settings`. The Config window shows the saved settings files, not this layer.

`--mcp-config` takes the same file-or-inline form, once per config, in Claude Code's `.mcp.json` shape: an object with an `mcpServers` object. The servers of all configs are merged in command-line order, and a later server with the same name replaces an earlier one. They join the session's other MCP servers in the MCP window under the dynamic scope. `--strict-mcp-config` drops every other MCP source (project `.mcp.json`, user settings, plugins and agent definitions), so only the `--mcp-config` servers remain; on its own it starts the session without MCP servers. A config without an `mcpServers` object, or a server that is not an object, fails the session start with a message naming the config.

`--system-prompt` and `--system-prompt-file` replace Claude Code's default system prompt; `--append-system-prompt` and `--append-system-prompt-file` add to it, or to the replacement. Each pair is mutually exclusive, a text value must not be empty, and a file is read once at startup. `claude-rs` always keeps one instruction of its own in the system prompt, which tells Claude not to stop the bridge runtime process: appended text follows it, and a replacement prompt is followed by it. The SDK records a conversation's system prompt on its first request and reuses it on resume, so these options shape new sessions; a resumed session keeps the prompt it started with.

An initial prompt whose text matches a subcommand name can be passed after `--`, for example `claude-rs -- "doctor"`. Startup arguments cannot be combined with support commands such as `doctor`, `logs`, `config`, or `completions`.

After the TUI exits, the app prints a resume hint whenever a session was established, using the active or most recently established session id.

An interactive launch needs a terminal. When stdin or stdout is not a terminal (a pipe, a redirect, a job without a TTY), `claude-rs` prints one line to stderr and exits with status 25 before it writes to stdout or starts a runtime log. Support commands such as `doctor`, `logs`, `config` and `completions` work without a terminal.

## Support And Diagnostics

Use support commands when you need a repeatable snapshot of the local environment or diagnostic logs.

| Command | Purpose |
| --- | --- |
| `claude-rs doctor` | Run installation, runtime, config path, log path, npm metadata, and credential checks. |
| `claude-rs doctor --json` | Emit the diagnostics report as JSON. |
| `claude-rs logs` | Show diagnostics log locations and useful follow-up commands. |
| `claude-rs logs --tail <LINES>` | Print redacted lines from the latest discovered log. |
| `claude-rs logs --bundle --yes` | Write a redacted ZIP debug bundle. |
| `claude-rs config` | Show resolved config paths and file states. |
| `claude-rs config show` | Show a concise redacted config summary. |
| `claude-rs config export --output <PATH>` | Write a redacted config export without overwriting existing files. |

See [Diagnostics](diagnostics.md) for full options, logging presets, bundle contents, and sharing guidance.

## CLI Options

The installed `claude-rs --help` command exposes these options; the session options are listed under their own heading:

| Option | Purpose |
| --- | --- |
| `-r, --resume [ID_OR_TITLE]` | Resume by exact ID or displayed title, or open the picker without a value. |
| `-c, --continue` | Continue the latest session for the working directory and its git worktrees; start new if none exists. |
| `--no-update-check` | Disable startup update checks. |
| `-C, --dir <DIR>` | Run in a specific working directory. |
| `--bridge-script <PATH>` | Use a specific Agent SDK bridge script. |
| `--enable-logs` | Expand the always-on baseline to detailed info-level diagnostics. |
| `--diagnostics-preset <runtime|session|render|bridge|full>` | Use a named diagnostics filter. |
| `--log-file <PATH>` | Write tracing diagnostics to a specific file. |
| `--log-filter <FILTER>` | Use explicit tracing filter directives. |
| `--log-append` | Append to the active log file instead of resetting it on startup. |
| `--model <MODEL>` | Set the initial session model without changing saved preferences. |
| `--effort <LEVEL>` | Set the initial session effort; availability depends on the model. |
| `--permission-mode <MODE>` | Set the initial session permission mode without changing saved preferences. |
| `--agent <NAME>` | Select the main-thread agent for the initial session. |
| `-n, --name <NAME>` | Name the initial new or resumed session. |
| `--fork-session` | Resume into a new session ID, leaving the original session unchanged. |
| `--add-dir <DIR>` | Let Claude access another directory in every session; repeatable. |
| `--settings <FILE-OR-JSON>` | Apply a settings file or inline JSON object as the flag-settings layer of every session. |
| `--mcp-config <FILE-OR-JSON>` | Add the MCP servers of a config file or inline JSON object to every session; repeatable. |
| `--strict-mcp-config` | Use only the `--mcp-config` servers, ignoring all other MCP configuration. |
| `--system-prompt <TEXT>` | Replace Claude Code's system prompt for new sessions. |
| `--system-prompt-file <FILE>` | Replace Claude Code's system prompt for new sessions with a file's text. |
| `--append-system-prompt <TEXT>` | Append text to the system prompt of new sessions. |
| `--append-system-prompt-file <FILE>` | Append a file's text to the system prompt of new sessions. |

See [Diagnostics](diagnostics.md) before enabling verbose logs.

## Shell Completions

`claude-rs completions <shell>` prints a completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish`. It works with script, npm, archive, and source installations and does not start the bridge or require credentials. All five scripts suggest CLI commands and flags. Bash, Zsh, and Fish also suggest fixed choices such as effort levels; the generated PowerShell and Elvish scripts currently complete command and option names. They do not query live session IDs, model catalogs, or agents. The TUI's slash-command completion is separate.

For PowerShell, enable completions in the current session:

```powershell
claude-rs completions powershell | Out-String | Invoke-Expression
```

Put that same line in your PowerShell `$PROFILE` to enable completions in future sessions. For Bash, run the following and add it to `~/.bashrc` for future shells:

```bash
source <(claude-rs completions bash)
```

For Zsh, generate the script:

```zsh
mkdir -p ~/.zfunc
claude-rs completions zsh > ~/.zfunc/_claude-rs
```

Add `fpath=(~/.zfunc $fpath)` to `~/.zshrc` before its completion initialization. If completion is not already initialized, add `autoload -Uz compinit` followed by `compinit` after the `fpath` line.

For Fish:

```fish
mkdir -p ~/.config/fish/completions
claude-rs completions fish > ~/.config/fish/completions/claude-rs.fish
```

Regenerate scripts saved to disk after upgrading `claude-rs`. The PowerShell and Bash startup lines above generate completions from the currently installed binary each time the shell starts.

## Man Pages

A man page is a local command reference opened in a terminal with the Unix `man` viewer.

Unix release archives include generated manuals under `share/man/man1/`, including a root page and pages for visible subcommands. The Unix installer links them into `share/man/man1/` beside the installation's `bin` directory, normally `~/.local/share/man/man1/`. Open the manual with:

```bash
man claude-rs
man claude-rs-config-show
```

If your man viewer does not search that directory, use `MANPATH="$HOME/.local/share/man:" man claude-rs`; the trailing colon preserves its default search paths. Custom `--bin-dir` installations use the corresponding sibling `share/man` directory. Uninstall removes only manual links pointing into the script installation.

For npm, source, or manually extracted installations, generate the pages locally with the packaging command:

```bash
claude-rs man ./man-pages
man ./man-pages/claude-rs.1
```

The `man` generator is hidden from normal command help because it is primarily a packaging utility. It works on Windows too, although viewing the output requires a Unix-compatible man viewer. Help, completion scripts, and manual pages all derive from the same CLI definitions.

## Core UI

The main screen is a terminal-owned chat view. The app renders messages, tool calls, diffs, permissions, questions, autocomplete, and status directly through Crossterm and Ratatui.

Common surfaces:

- Chat input for prompts and multiline text.
- File, slash-command, and subagent autocomplete.
- Inline permission prompts for tool decisions.
- Inline questions for agent-requested choices or text.
- Fullscreen settings, status, usage, MCP, plugins, and help tabs.
- Session picker when running `claude-rs resume` without a session id.

Use `/help` for the fullscreen help tab, `/config` for settings, and `/docs <topic>` for live in-chat help generated from the running app state.

## Hook Notices

Hooks run invisibly, so a slow or failing one used to leave no trace. claude-rs asks the Agent SDK to report every hook event and shows a one-line notice in the transcript when a hook finishes:

- `Hook SessionStart:startup took 2.5 s`, dim, when it took 500 ms or longer;
- `Hook Stop failed (exit 1): <error output excerpt>`, as a warning, when it failed, with the SDK's exit code and a bounded excerpt of its error output (one line, control characters removed);
- `Hook <name> was cancelled`, as a warning, when it was cancelled.

Fast, successful hooks show nothing. The name is the SDK's: for events with a matcher it reads `Event:matcher`, and it does not identify the command. `presentation.hookNotices` in Settings > Hooks chooses `slow_and_failed` (the default), `all` (every finished hook) or `off`; the 500 ms threshold is fixed. A hook's duration is the time between the bridge seeing its start and its end, because the SDK reports no hook timing. `SessionStart` hooks finish before the app has read its settings, so their notices appear once the saved choice is known. The Status tab lists hook timings whatever the setting; see [Status Tab](commands.md#status-tab).

## More Usage Topics

- [Slash Commands](commands.md)
- [Keyboard Shortcuts](shortcuts.md)
- [Settings](settings.md)
- [Diagnostics](diagnostics.md)
- [Help](help.md)
