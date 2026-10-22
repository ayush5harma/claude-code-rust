# Your first session

Use this walkthrough to ask a question about a project, continue the conversation, and reopen it after exiting. You need an [installed application](installation.md), the Claude CLI on PATH for sign-in, and a project directory you are comfortable letting Claude inspect. Account access and usage limits are controlled by Anthropic; see the [billing note](about.md#billing-note).

## Open your project

Open a terminal in your project directory and run:

```bash
claude-rs
```

The directory you start in is the session's working directory. Check the location shown by the app before proceeding. To select a different directory at launch, use `claude-rs -C path/to/project` with your own path.

If the app asks whether to trust the project, review the directory before accepting. If authentication is required, use the sign-in flow offered by the app or run `claude auth login` from your shell and start again. See [login troubleshooting](troubleshooting.md#login-and-authentication-failures) if sign-in does not complete.

## Ask a question

When the composer is ready, type the following and press `Enter`:

```text
Read this project's README and summarize its purpose and how to run it. Do not change files.
```

Choose a project with a README for this example. Claude may read files or request permission to use a tool. Review the requested action and choose from the displayed options. Declining an action can prevent Claude from completing the task; you can still explain what you want in a follow-up.

You should see the reply and any tool output in the conversation. The answer depends on the project; success is a completed response explaining its purpose and run instructions, with the composer available for the next message.

If no response arrives, distinguish a waiting permission or question from a connection failure. Respond to the visible prompt, or use the [startup troubleshooting steps](troubleshooting.md#the-app-will-not-start-or-the-bridge-will-not-spawn).

## Continue the conversation

Ask a follow-up:

```text
Which file should I read first to understand the main workflow?
```

Finished output remains in your terminal's scrollback. Use `Page Up` to read earlier output inside the app and `Ctrl+End` to return to the latest output. `Esc` cancels an active turn; it does not undo work already completed.

Enter `/help` to open built-in help. Close the fullscreen view with `Esc` to return to the conversation. For multiline input and context-specific controls, see [Keyboard shortcuts](shortcuts.md).

## Exit and resume

After the response completes, press `Ctrl+Q` to quit. When a session was established, the app prints a command for resuming it.

From the same project directory, run:

```bash
claude-rs --resume
```

Choose the conversation you just used and press `Enter`. Its saved history should appear, and you can ask another question. The picker searches by name, session ID, or branch. To continue the most recent conversation directly, use `claude-rs --continue`; if none exists, that command starts a new session.

## Exit Status

`claude-rs` reports how it ended in its exit status, so scripts and CI jobs can tell a usage error from a bridge failure. `claude-rs --help` ends with this list; `-h` leaves it out.

| Status | Meaning |
| --- | --- |
| `0` | Success, including `--help` and `--version`. |
| `1` | Any other failure, such as `doctor --strict` finding a hard failure or `config show` reading an invalid file. |
| `2` | Invalid command line: an unknown or misspelled command or flag, a missing value, conflicting options, or a path that does not exist. |
| `20` | The bridge runtime was not found. |
| `21` | The bridge process failed to start or crashed. |
| `22` | Bridge initialization, protocol, SDK or connection failure, including a timeout. |
| `23` | The requested session could not be resumed. |
| `24` | Authentication is required. |
| `25` | An interactive launch had no terminal: stdout must be a TTY, and so must stdin unless `/dev/tty` can be opened. |

Statuses 20 to 24 come with a failure report on stderr naming a next step; see [Diagnostics](diagnostics.md#failure-reports). Status 25 is described under [Terminal requirement](cli.md#terminal-requirement).

## Environment

| Variable | Effect |
| --- | --- |
| `CLAUDE_RS_AGENT_BRIDGE` | Agent bridge script to run. The npm launcher sets it to the bundled script. |
| `CLAUDE_RS_AGENT_BRIDGE_RUNTIME` | Runtime that runs the bridge script; debug builds only. |
| `CLAUDE_RUST_NO_UPDATE_CHECK` | `1`, `true` or `yes` disables update checks, as `--no-update-check` does. |
| `CLAUDE_CONFIG_DIR` | Claude profile directory to use instead of `~/.claude`; see [Settings](settings.md#where-settings-are-saved). |
| `RUST_LOG` | Tracing filter used without `--log-filter` or `--diagnostics-preset`; setting it turns on detailed logging. |
| `NO_COLOR` | Disables colour in support command output. |

The installer scripts read their own `CLAUDE_RS_*` variables; see [Installation](installation.md).

## Next steps

- [Work with conversations](conversations.md): resume, fork, review Git changes, copy responses, export, and inspect subagents.
- [Configure claude-rs](configure.md): save preferences and distinguish them from current-session choices.
- [Use MCP servers and plugins](integrations.md): inspect connections and load local plugins.
- [CLI reference](cli.md): launch flags, shell completions, and local manuals.
