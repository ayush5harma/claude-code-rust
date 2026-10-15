// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

pub mod agent;
pub mod app;
pub mod appearance;
mod claude_paths;
pub mod cli;
pub mod error;
pub mod failure;
pub mod install_method;
mod json_file;
pub mod logging;
pub mod permission_mode;
pub mod ui;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum DiagnosticsPreset {
    Runtime,
    Session,
    Render,
    Bridge,
    Full,
}

impl DiagnosticsPreset {
    #[must_use]
    pub fn filter_directives(&self) -> &'static str {
        match self {
            Self::Runtime => {
                "info,bridge.lifecycle=debug,bridge.protocol=debug,app.session=debug,app.tool=debug,app.command=debug,app.permission=debug,app.notify=debug,app.network=debug,app.update=debug,app.file_index=debug"
            }
            Self::Session => {
                "info,bridge.lifecycle=debug,bridge.protocol=debug,app.session=debug,app.permission=debug,app.notify=debug,app.command=debug"
            }
            Self::Render => "info,app.render=trace,app.cache=debug,app.input=debug,app.paste=debug",
            Self::Bridge => {
                "info,bridge.lifecycle=debug,bridge.protocol=debug,bridge.sdk=debug,bridge.permission=debug,bridge.mcp=debug"
            }
            Self::Full => {
                "info,app.render=trace,bridge.lifecycle=debug,bridge.protocol=debug,bridge.sdk=debug,bridge.permission=debug,bridge.mcp=debug,app.session=debug,app.tool=debug,app.command=debug,app.permission=debug,app.notify=debug,app.network=debug,app.update=debug,app.cache=debug,app.input=debug,app.paste=debug,app.config=debug,app.auth=debug,app.file_index=debug"
            }
        }
    }
}

/// The examples closing `-h`, repeated at the head of `--help`'s longer epilogue.
macro_rules! help_examples {
    () => {
        "Examples:\n  claude-rs --diagnostics-preset session\n  claude-rs --diagnostics-preset render"
    };
}

/// Exit statuses and environment variables, closing `--help`. `docs/src/usage.md` has the
/// same two lists; `tests/docs_cli_contract.rs` checks both against `AppError`.
macro_rules! help_exit_status_and_environment {
    () => {
        "Exit status:
  0   Success, including --help and --version
  1   Any other failure, such as `doctor --strict` finding a hard failure
  2   Invalid command line
  20  Bridge runtime not found
  21  Bridge process failed to start or crashed
  22  Bridge initialization, protocol or connection failure
  23  The requested session could not be resumed
  24  Authentication required
  25  Interactive launch without a terminal on stdout (or on stdin without /dev/tty)

Environment:
  CLAUDE_RS_AGENT_BRIDGE          Agent bridge script to run
  CLAUDE_RS_AGENT_BRIDGE_RUNTIME  Runtime for the bridge script (debug builds only)
  CLAUDE_RUST_NO_UPDATE_CHECK     1, true or yes disables update checks, as --no-update-check does
  CLAUDE_CONFIG_DIR               Claude profile directory to use instead of ~/.claude
  RUST_LOG                        Tracing filter used without --log-filter or --diagnostics-preset;
                                  setting it turns on detailed logging
  NO_COLOR                        Disables colour in support command output"
    };
}

#[derive(Parser, Debug)]
#[command(
    name = "claude-rs",
    version = env!("CARGO_PKG_VERSION"),
    about = "Native Rust terminal for Claude Code",
    // clap's own `[PROMPT] [COMMAND]` reads as if both could be given; they cannot.
    override_usage = "claude-rs [OPTIONS] [PROMPT]\n       claude-rs [OPTIONS] <COMMAND>"
)]
#[command(
    after_help = help_examples!(),
    after_long_help = concat!(help_examples!(), "\n\n", help_exit_status_and_environment!())
)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Initial prompt to send after the interactive session is ready.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Resume by exact session ID or title, or open the picker with no value.
    // Clap uses the nested option for absent flag, picker, and explicit ID.
    #[allow(clippy::option_option)]
    #[arg(long, short = 'r', num_args = 0..=1, value_name = "ID", conflicts_with = "continue_session")]
    pub resume: Option<Option<String>>,

    /// Continue the most recent session in the working directory (including git worktrees), or start a new one if there is none.
    #[arg(long = "continue", short = 'c')]
    pub continue_session: bool,

    /// Disable startup update checks.
    #[arg(long)]
    pub no_update_check: bool,

    /// Working directory (defaults to cwd)
    #[arg(long, short = 'C', value_parser = parse_existing_dir)]
    pub dir: Option<std::path::PathBuf>,

    /// Path to the agent bridge script (defaults to agent-sdk/dist/bridge.js).
    // Checked for interactive launches only (`Cli::session_config`): support commands
    // never start the bridge, and `doctor` reports on a bad script instead of refusing it.
    // Hidden: a development override (as is CLAUDE_RS_AGENT_BRIDGE), and the default it
    // names is not where installed builds find their bridge.
    #[arg(long, hide = true)]
    pub bridge_script: Option<std::path::PathBuf>,

    // After the flattened groups, whose help heading would otherwise carry over to these.
    #[command(flatten)]
    pub session_options: SessionOptions,

    #[command(flatten)]
    pub session_config: SessionConfigArgs,

    // Each of these, and RUST_LOG, turns on detailed logging (`logging`'s
    // `detailed_diagnostics_requested`), so each help line says so.
    /// Turn on detailed logging: info-level diagnostics on top of the always-on warning and error baseline.
    #[arg(long, help_heading = "Diagnostics Options")]
    pub enable_logs: bool,

    /// Turn on detailed logging with a named filter preset; ignored when `--log-filter` is given.
    #[arg(long, value_enum, help_heading = "Diagnostics Options")]
    pub diagnostics_preset: Option<DiagnosticsPreset>,

    /// Turn on detailed logging and write it to this file instead of a new timestamped file.
    #[arg(long, value_name = "PATH", help_heading = "Diagnostics Options")]
    pub log_file: Option<std::path::PathBuf>,

    /// Turn on detailed logging with these tracing filter directives (example: `info,app.render=trace`).
    ///
    /// Overrides `--diagnostics-preset`. Without it, `RUST_LOG` is used, and setting `RUST_LOG`
    /// also turns on detailed logging.
    #[arg(long, value_name = "FILTER", value_parser = parse_log_filter, help_heading = "Diagnostics Options")]
    pub log_filter: Option<String>,

    /// Append to the `--log-file` file instead of truncating it.
    #[arg(long, requires = "log_file", help_heading = "Diagnostics Options")]
    pub log_append: bool,
}

/// Whether `value` came after a `--` in the raw arguments, where only positionals follow.
fn after_separator(args: &[std::ffi::OsString], value: &str) -> bool {
    args.iter().skip(1).skip_while(|arg| *arg != "--").skip(1).any(|arg| arg == value)
}

/// The visible command a one-word prompt is a likely typo of. clap takes an unknown word
/// as the prompt, so `claude-rs doctr` would send "doctr" to Claude.
fn mistyped_command(prompt: &str) -> Option<String> {
    use clap::CommandFactory;
    let word: Vec<char> = prompt.to_lowercase().chars().collect();
    if word.len() < 4 || word.iter().any(|ch| ch.is_whitespace()) {
        return None;
    }
    Cli::command()
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(clap::Command::get_name)
        .find(|name| is_typo_of(&word, &name.chars().collect::<Vec<_>>()))
        .map(str::to_owned)
}

/// One slip of the fingers away from `name`: a different case, two adjacent characters
/// swapped, or one character dropped or doubled, never the first or the last. A real word
/// is usually a substitution ("logo", "lots" for `logs`) or a changed ending ("doctors",
/// "completion"), and those are left as prompts; so are words one edit away with a
/// different first letter ("dogs", "actor").
fn is_typo_of(word: &[char], name: &[char]) -> bool {
    if word == name {
        return true;
    }
    if word.first() != name.first() {
        return false;
    }
    if word.len() == name.len() {
        let differing: Vec<usize> = (0..word.len()).filter(|&i| word[i] != name[i]).collect();
        return matches!(differing[..], [i, j] if j == i + 1 && word[i] == name[j] && word[j] == name[i]);
    }
    let (longer, shorter) = if word.len() > name.len() { (word, name) } else { (name, word) };
    longer.len() == shorter.len() + 1
        && (1..shorter.len())
            .any(|i| longer[..i] == shorter[..i] && longer[i + 1..] == shorter[i..])
}

/// Checked while parsing, so a bad filter is a usage error quoting the user's own text,
/// not a startup failure quoting the directives logging appends to it.
fn parse_log_filter(value: &str) -> Result<String, String> {
    tracing_subscriber::EnvFilter::try_new(value)
        .map(|_| value.to_owned())
        .map_err(|error| error.to_string())
}

/// Per-launch options, separate from persisted application preferences.
#[derive(Args, Debug, Clone, Default, PartialEq, Eq)]
#[command(next_help_heading = "Session Options")]
pub struct SessionOptions {
    /// Starting model alias or full model ID; does not change saved preferences.
    #[arg(long, value_name = "MODEL", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub model: Option<String>,

    /// Starting reasoning effort; availability depends on the selected model.
    #[arg(long, value_enum)]
    pub effort: Option<agent::model::EffortLevel>,

    /// Starting permission mode; does not change saved preferences.
    #[arg(long, value_enum)]
    pub permission_mode: Option<permission_mode::DefaultPermissionMode>,

    /// Main-thread agent to use for this session.
    #[arg(long, value_name = "NAME", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub agent: Option<String>,

    /// Name the new or resumed session.
    #[arg(long, short = 'n', value_name = "NAME", value_parser = parse_session_name)]
    pub name: Option<String>,

    /// Resume into a new session ID, leaving the original session unchanged.
    #[arg(long)]
    pub fork_session: bool,
}

/// `--dir` is made absolute once, here, so the session, relative `--add-dir` and file
/// flags, and the config commands all see the same existing directory.
fn parse_existing_dir(value: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::absolute(value).map_err(|error| format!("cannot resolve it: {error}"))?;
    if !path.is_dir() {
        return Err(format!("{} is not an existing directory", path.display()));
    }
    Ok(path)
}

/// Session names are shown in the terminal, so control characters are dropped,
/// as the title converter does for titles the SDK reports.
fn parse_session_name(value: &str) -> Result<String, String> {
    let name: String = value.chars().filter(|ch| !ch.is_control()).collect();
    let name = name.trim();
    if name.is_empty() {
        return Err("the session name must contain visible characters".to_owned());
    }
    Ok(name.to_owned())
}

// Session configuration for every session this process starts, including `/new` and
// `/resume`, as Claude Code keeps these flags for its whole process. Relative paths
// resolve against `--dir`, or the current directory without it. (A doc comment here
// would replace the command's about text in `--help`.)
#[derive(Args, Debug, Clone, Default, PartialEq, Eq)]
#[command(next_help_heading = "Session Options")]
pub struct SessionConfigArgs {
    /// Additional directory Claude may access (repeatable).
    #[arg(long = "add-dir", value_name = "DIR")]
    pub add_dirs: Vec<std::path::PathBuf>,

    /// Settings JSON file, or inline JSON, applied as Claude Code's --settings layer.
    #[arg(long, value_name = "FILE-OR-JSON")]
    pub settings: Option<String>,

    /// MCP server config file, or inline JSON, with an `mcpServers` object (repeatable).
    #[arg(long = "mcp-config", value_name = "FILE-OR-JSON")]
    pub mcp_configs: Vec<String>,

    /// Use only the MCP servers from --mcp-config, ignoring all other MCP configuration.
    #[arg(long)]
    pub strict_mcp_config: bool,

    /// Replace Claude Code's system prompt for new sessions.
    #[arg(long, value_name = "TEXT", conflicts_with = "system_prompt_file", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub system_prompt: Option<String>,

    /// Replace Claude Code's system prompt for new sessions with a file's text.
    #[arg(long, value_name = "FILE")]
    pub system_prompt_file: Option<std::path::PathBuf>,

    /// Append text to the system prompt of new sessions.
    #[arg(long, value_name = "TEXT", conflicts_with = "append_system_prompt_file", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub append_system_prompt: Option<String>,

    /// Append a file's text to the system prompt of new sessions.
    #[arg(long, value_name = "FILE")]
    pub append_system_prompt_file: Option<std::path::PathBuf>,
}

/// A launch flag's parsed value, which clap leaves empty when the flag is absent.
trait LaunchFlag {
    fn is_given(&self) -> bool;
}

impl<T> LaunchFlag for Option<T> {
    fn is_given(&self) -> bool {
        self.is_some()
    }
}

impl LaunchFlag for bool {
    fn is_given(&self) -> bool {
        *self
    }
}

impl<T> LaunchFlag for Vec<T> {
    fn is_given(&self) -> bool {
        !self.is_empty()
    }
}

/// Moves a value given after `resume` into the root's slot; a flag is accepted in one
/// position only.
fn merge_flag<T: LaunchFlag>(root: &mut T, resumed: T, flag: &str) -> Result<(), String> {
    if !resumed.is_given() {
        return Ok(());
    }
    if root.is_given() {
        return Err(format!("the argument '{flag}' cannot be used both before and after `resume`"));
    }
    *root = resumed;
    Ok(())
}

// The flag lists below destructure exhaustively, so a new field fails to compile until
// it is listed in both.
impl SessionOptions {
    fn given_flags(&self) -> impl Iterator<Item = &'static str> {
        let Self { model, effort, permission_mode, agent, name, fork_session } = self;
        [
            ("--model", model.is_given()),
            ("--effort", effort.is_given()),
            ("--permission-mode", permission_mode.is_given()),
            ("--agent", agent.is_given()),
            ("--name", name.is_given()),
            ("--fork-session", fork_session.is_given()),
        ]
        .into_iter()
        .filter_map(|(flag, given)| given.then_some(flag))
    }

    fn merge(&mut self, resumed: Self) -> Result<(), String> {
        let Self { model, effort, permission_mode, agent, name, fork_session } = resumed;
        merge_flag(&mut self.model, model, "--model")?;
        merge_flag(&mut self.effort, effort, "--effort")?;
        merge_flag(&mut self.permission_mode, permission_mode, "--permission-mode")?;
        merge_flag(&mut self.agent, agent, "--agent")?;
        merge_flag(&mut self.name, name, "--name")?;
        merge_flag(&mut self.fork_session, fork_session, "--fork-session")
    }
}

impl SessionConfigArgs {
    fn given_flags(&self) -> impl Iterator<Item = &'static str> {
        let Self {
            add_dirs,
            settings,
            mcp_configs,
            strict_mcp_config,
            system_prompt,
            system_prompt_file,
            append_system_prompt,
            append_system_prompt_file,
        } = self;
        [
            ("--add-dir", add_dirs.is_given()),
            ("--settings", settings.is_given()),
            ("--mcp-config", mcp_configs.is_given()),
            ("--strict-mcp-config", strict_mcp_config.is_given()),
            ("--system-prompt", system_prompt.is_given()),
            ("--system-prompt-file", system_prompt_file.is_given()),
            ("--append-system-prompt", append_system_prompt.is_given()),
            ("--append-system-prompt-file", append_system_prompt_file.is_given()),
        ]
        .into_iter()
        .filter_map(|(flag, given)| given.then_some(flag))
    }

    fn merge(&mut self, resumed: Self) -> Result<(), String> {
        let Self {
            add_dirs,
            settings,
            mcp_configs,
            strict_mcp_config,
            system_prompt,
            system_prompt_file,
            append_system_prompt,
            append_system_prompt_file,
        } = resumed;
        merge_flag(&mut self.add_dirs, add_dirs, "--add-dir")?;
        merge_flag(&mut self.settings, settings, "--settings")?;
        merge_flag(&mut self.mcp_configs, mcp_configs, "--mcp-config")?;
        merge_flag(&mut self.strict_mcp_config, strict_mcp_config, "--strict-mcp-config")?;
        merge_flag(&mut self.system_prompt, system_prompt, "--system-prompt")?;
        merge_flag(&mut self.system_prompt_file, system_prompt_file, "--system-prompt-file")?;
        merge_flag(&mut self.append_system_prompt, append_system_prompt, "--append-system-prompt")?;
        merge_flag(
            &mut self.append_system_prompt_file,
            append_system_prompt_file,
            "--append-system-prompt-file",
        )?;
        // clap checks these pairs within one position only.
        for (text, file, text_flag, file_flag) in [
            (
                self.system_prompt.is_given(),
                self.system_prompt_file.is_given(),
                "--system-prompt",
                "--system-prompt-file",
            ),
            (
                self.append_system_prompt.is_given(),
                self.append_system_prompt_file.is_given(),
                "--append-system-prompt",
                "--append-system-prompt-file",
            ),
        ] {
            if text && file {
                return Err(format!(
                    "the argument '{text_flag}' cannot be used with '{file_flag}'"
                ));
            }
        }
        Ok(())
    }
}

impl SessionConfigArgs {
    /// Resolve paths against `base` and check that they exist, so a typo stops the launch
    /// with a CLI error instead of surfacing inside a running session.
    pub fn resolve(&self, base: &std::path::Path) -> Result<agent::wire::SessionConfig, String> {
        let additional_directories = self
            .add_dirs
            .iter()
            .map(|dir| {
                let path = absolute_path(base, dir, "--add-dir")?;
                if !std::path::Path::new(&path).is_dir() {
                    return Err(format!("--add-dir: {path} is not an existing directory"));
                }
                Ok(path)
            })
            .collect::<Result<_, String>>()?;
        let settings = self
            .settings
            .as_deref()
            .map(|value| json_or_file_text(base, value, "--settings"))
            .transpose()?;
        let mcp_configs = self
            .mcp_configs
            .iter()
            .map(|value| json_or_file_text(base, value, "--mcp-config"))
            .collect::<Result<_, String>>()?;
        let system_prompt = text_or_file(
            base,
            self.system_prompt.as_deref(),
            self.system_prompt_file.as_deref(),
            "--system-prompt-file",
        )?;
        let append_system_prompt = text_or_file(
            base,
            self.append_system_prompt.as_deref(),
            self.append_system_prompt_file.as_deref(),
            "--append-system-prompt-file",
        )?;
        Ok(agent::wire::SessionConfig {
            additional_directories,
            settings,
            mcp_configs,
            strict_mcp_config: self.strict_mcp_config,
            system_prompt,
            append_system_prompt,
        })
    }
}

/// The text of a flag pair whose two spellings clap keeps mutually exclusive.
fn text_or_file(
    base: &std::path::Path,
    text: Option<&str>,
    file: Option<&std::path::Path>,
    file_flag: &str,
) -> Result<Option<String>, String> {
    match (text, file) {
        (Some(text), _) => Ok(Some(text.to_owned())),
        (None, Some(file)) => read_text_file(base, file, file_flag).map(Some),
        (None, None) => Ok(None),
    }
}

fn read_text_file(
    base: &std::path::Path,
    file: &std::path::Path,
    flag: &str,
) -> Result<String, String> {
    let path = absolute_path(base, file, flag)?;
    std::fs::read_to_string(&path).map_err(|error| format!("{flag}: cannot read {path}: {error}"))
}

/// Inline JSON is passed through; anything else is a file whose text is read once here.
/// The bridge owns parsing, so only the leading brace is inspected.
fn json_or_file_text(base: &std::path::Path, value: &str, flag: &str) -> Result<String, String> {
    if value.trim_start().starts_with('{') {
        return Ok(value.to_owned());
    }
    read_text_file(base, std::path::Path::new(value), flag)
}

fn absolute_path(
    base: &std::path::Path,
    path: &std::path::Path,
    flag: &str,
) -> Result<String, String> {
    let absolute = std::path::absolute(base.join(path))
        .map_err(|error| format!("{flag}: cannot resolve {}: {error}", path.display()))?;
    absolute
        .into_os_string()
        .into_string()
        .map_err(|path| format!("{flag}: {} is not valid UTF-8", path.to_string_lossy()))
}

impl Cli {
    /// Parse a command line. Use this rather than clap's own parse: session options are
    /// accepted before `resume` and after it, and this moves the second spelling into
    /// [`Cli::session_options`] and [`Cli::session_config`], the only fields read later.
    pub fn parse_from_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString>,
    {
        use clap::CommandFactory;
        let args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
        let mut cli = <Self as Parser>::try_parse_from(&args)?;
        if cli.command.is_none()
            && let Some(prompt) = cli.prompt.as_deref()
            && !after_separator(&args, prompt)
            && let Some(command) = mistyped_command(prompt)
        {
            return Err(Self::command().error(
                clap::error::ErrorKind::InvalidSubcommand,
                format!(
                    "unknown command `{prompt}`; did you mean `{command}`? To send it as a prompt, run `claude-rs -- {prompt}`"
                ),
            ));
        }
        if let Some(Command::Resume { session_options, session_config, .. }) = &mut cli.command {
            let options = std::mem::take(session_options);
            let config = std::mem::take(session_config);
            cli.session_options
                .merge(options)
                .and_then(|()| cli.session_config.merge(config))
                .map_err(|message| {
                    Self::command().error(clap::error::ErrorKind::ArgumentConflict, message)
                })?;
        }
        Ok(cli)
    }

    pub fn validate(&self) -> Result<(), clap::Error> {
        use clap::CommandFactory;
        let mut startup_arguments = [
            ("--resume", self.resume.is_some()),
            ("--continue", self.continue_session),
            ("a prompt", self.prompt.is_some()),
        ]
        .into_iter()
        .filter_map(|(argument, given)| given.then_some(argument));
        let conflict = match &self.command {
            Some(Command::Resume { .. }) => startup_arguments
                .next()
                .map(|argument| format!("{argument} cannot be combined with the `resume` command")),
            Some(command) => startup_arguments
                .chain(self.session_options.given_flags())
                .chain(self.session_config.given_flags())
                .next()
                .map(|argument| {
                    format!(
                        "{argument} applies only to interactive launches, not `{}`",
                        command.name()
                    )
                }),
            None => None,
        };
        if let Some(message) = conflict {
            return Err(Self::command().error(clap::error::ErrorKind::ArgumentConflict, message));
        }
        if self.session_options.fork_session && !self.startup_launch().resume_or_pick() {
            return Err(Self::command().error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "--fork-session requires --resume, --continue or the resume subcommand",
            ));
        }
        Ok(())
    }
    /// The session working directory: `--dir`, or the current directory without it.
    #[must_use]
    pub fn startup_cwd(&self) -> std::path::PathBuf {
        self.dir.clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
    }

    /// Resolve the session configuration flags against the session working directory, and
    /// check the files an interactive launch needs, so a typo stops the launch with a CLI
    /// error rather than a bridge failure report.
    pub fn session_config(&self) -> Result<agent::wire::SessionConfig, clap::Error> {
        use clap::CommandFactory;
        let invalid =
            |message| Self::command().error(clap::error::ErrorKind::ValueValidation, message);
        let config = self.session_config.resolve(&self.startup_cwd()).map_err(invalid)?;
        if let Some(script) = &self.bridge_script
            && !script.is_file()
        {
            return Err(invalid(format!(
                "--bridge-script: {} is not an existing file",
                script.display()
            )));
        }
        Ok(config)
    }

    /// Normalize both resume spellings into one startup intent.
    #[must_use]
    pub fn startup_launch(&self) -> StartupLaunch {
        let resume = match &self.command {
            Some(Command::Resume { session_id, .. }) => Some(session_id.as_ref()),
            _ => self.resume.as_ref().map(Option::as_ref),
        };
        match resume {
            Some(Some(id)) => StartupLaunch::ResumeSession { session_id: id.clone() },
            Some(None) => StartupLaunch::SessionPicker,
            None if self.continue_session => StartupLaunch::ContinueSession,
            None => StartupLaunch::NewSession,
        }
    }

    #[must_use]
    pub fn initial_prompt(&self) -> Option<&str> {
        match &self.command {
            Some(Command::Resume { prompt, .. }) => prompt.as_deref(),
            _ => self.prompt.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StartupLaunch {
    #[default]
    NewSession,
    ResumeSession {
        session_id: String,
    },
    SessionPicker,
    ContinueSession,
}

impl StartupLaunch {
    #[must_use]
    pub fn resume_id(&self) -> Option<&str> {
        match self {
            Self::ResumeSession { session_id } => Some(session_id),
            _ => None,
        }
    }

    #[must_use]
    pub fn resume_requested(&self) -> bool {
        matches!(self, Self::ResumeSession { .. } | Self::ContinueSession)
    }

    /// Whether the launch resumes an existing session, directly or through the picker.
    #[must_use]
    pub fn resume_or_pick(&self) -> bool {
        self.resume_requested() || matches!(self, Self::SessionPicker)
    }
}

// One value per process, parsed once: the size of `Resume` costs nothing worth boxing for.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    /// Resume a previous session by exact ID or title, or pick from recent sessions
    Resume {
        /// Session ID or displayed title to resume directly. Omit to show a session picker.
        session_id: Option<String>,
        /// Initial prompt to send after resuming.
        prompt: Option<String>,
        // Accepted here as well as before `resume`; `Cli::parse_from_args` moves them into
        // the root's fields and leaves these empty.
        #[command(flatten)]
        session_options: SessionOptions,
        #[command(flatten)]
        session_config: SessionConfigArgs,
    },
    /// Print a shell completion script.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Generate manual pages for the CLI and its subcommands.
    #[command(hide = true)]
    Man { out_dir: std::path::PathBuf },
    /// Install an update after exit, detached from the terminal.
    #[command(hide = true)]
    UpdateWorker(UpdateWorkerArgs),
    /// Run deterministic installation and runtime diagnostics
    Doctor(DoctorArgs),
    /// Find runtime logs or create a redacted debug bundle
    Logs(LogsArgs),
    /// Inspect and export redacted configuration
    Config(ConfigArgs),
}

impl Command {
    /// The name the command is invoked by.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Resume { .. } => "resume",
            Self::Completions { .. } => "completions",
            Self::Man { .. } => "man",
            Self::UpdateWorker(_) => "update-worker",
            Self::Doctor(_) => "doctor",
            Self::Logs(_) => "logs",
            Self::Config(_) => "config",
        }
    }
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct UpdateWorkerArgs {
    /// Version to install.
    pub version: String,

    /// Installation to update.
    #[arg(long, value_enum)]
    pub method: UpdateWorkerMethod,

    /// Script installation directory to update in place.
    #[arg(long)]
    pub install_dir: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum UpdateWorkerMethod {
    Npm,
    Script,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct DoctorArgs {
    /// Emit a machine-readable JSON report.
    #[arg(long)]
    pub json: bool,

    /// Exit non-zero when hard runtime prerequisites fail.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LogsArgs {
    /// Print only the runtime log directory path.
    #[arg(long, conflicts_with_all = ["latest", "tail", "bundle"])]
    pub path: bool,

    /// Print only the latest discovered log path.
    #[arg(long, conflicts_with_all = ["path", "tail", "bundle"])]
    pub latest: bool,

    /// Print the last N redacted lines from the latest discovered log.
    #[arg(long, value_name = "LINES", conflicts_with_all = ["path", "latest", "bundle"])]
    pub tail: Option<usize>,

    /// Create a redacted ZIP bundle for support.
    #[arg(long, conflicts_with_all = ["path", "latest", "tail"])]
    pub bundle: bool,

    /// Write the bundle ZIP to this path.
    #[arg(long, value_name = "PATH", requires = "bundle")]
    pub output: Option<std::path::PathBuf>,

    /// Skip interactive confirmation for bundle creation.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: Option<ConfigCommand>,
}

#[derive(Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum ConfigCommand {
    /// Print resolved config file paths
    Path(ConfigPathArgs),
    /// Show a concise redacted config summary
    Show(ConfigShowArgs),
    /// Export a redacted support-safe config snapshot
    Export(ConfigExportArgs),
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigPathArgs {
    /// Emit machine-readable JSON path metadata.
    #[arg(long)]
    pub json: bool,

    /// Print only one config file path for scripting.
    #[arg(long, value_enum)]
    pub which: Option<ConfigFileSelector>,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigShowArgs {
    /// Emit machine-readable redacted JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ConfigExportArgs {
    /// Write the redacted export JSON to this new file.
    #[arg(long, value_name = "PATH")]
    pub output: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum ConfigFileSelector {
    Settings,
    ProjectSettings,
    LocalSettings,
    Preferences,
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, ConfigArgs, ConfigCommand, ConfigExportArgs, ConfigFileSelector,
        ConfigPathArgs, ConfigShowArgs, DoctorArgs, LogsArgs, SessionConfigArgs, SessionOptions,
    };
    use clap::CommandFactory;

    #[test]
    fn cli_without_subcommand_starts_new_session() {
        let cli = Cli::parse_from_args(["claude-rs"]).expect("parse");
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_resume_without_id_requests_picker() {
        let cli = Cli::parse_from_args(["claude-rs", "resume"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Resume {
                session_id: None,
                prompt: None,
                session_options: SessionOptions::default(),
                session_config: SessionConfigArgs::default(),
            })
        );
    }

    #[test]
    fn cli_resume_with_id_resumes_directly() {
        let cli = Cli::parse_from_args(["claude-rs", "resume", "abc-123"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Resume {
                session_id: Some("abc-123".to_owned()),
                prompt: None,
                session_options: SessionOptions::default(),
                session_config: SessionConfigArgs::default(),
            })
        );
    }

    #[test]
    fn cli_resume_flag_selects_the_same_session_as_the_subcommand() {
        let flag = Cli::parse_from_args(["claude-rs", "--resume", "abc-123"]).expect("flag");
        let command = Cli::parse_from_args(["claude-rs", "resume", "abc-123"]).expect("command");
        assert_eq!(flag.startup_launch(), command.startup_launch());
    }

    #[test]
    fn cli_rejects_removed_perf_flags() {
        let cases: &[&[&str]] = &[
            &["claude-rs", "--enable-perf"],
            &["claude-rs", "--perf-log", "perf.jsonl"],
            &["claude-rs", "--perf-append"],
        ];
        for args in cases {
            let error = Cli::parse_from_args(args.iter().copied()).expect_err("removed flag");
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn cli_help_omits_perf_flags_and_retains_runtime_logging() {
        let help = Cli::command().render_long_help().to_string();
        for flag in ["--enable-perf", "--perf-log", "--perf-append"] {
            assert!(!help.contains(flag), "help still advertises {flag}");
        }
        for flag in ["--enable-logs", "--log-file", "--log-append", "--diagnostics-preset"] {
            assert!(help.contains(flag), "runtime logging flag {flag} missing");
        }
    }

    #[test]
    fn cli_doctor_defaults_to_human_output() {
        let cli = Cli::parse_from_args(["claude-rs", "doctor"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Doctor(DoctorArgs { json: false, strict: false })));
    }

    #[test]
    fn cli_doctor_accepts_json_and_strict() {
        let cli =
            Cli::parse_from_args(["claude-rs", "doctor", "--json", "--strict"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Doctor(DoctorArgs { json: true, strict: true })));
    }

    #[test]
    fn cli_logs_defaults_to_summary() {
        let cli = Cli::parse_from_args(["claude-rs", "logs"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Logs(LogsArgs {
                path: false,
                latest: false,
                tail: None,
                bundle: false,
                output: None,
                yes: false,
            }))
        );
    }

    #[test]
    fn cli_logs_accepts_modes() {
        let cli = Cli::parse_from_args(["claude-rs", "logs", "--tail", "200"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Logs(LogsArgs {
                path: false,
                latest: false,
                tail: Some(200),
                bundle: false,
                output: None,
                yes: false,
            }))
        );

        let cli =
            Cli::parse_from_args(["claude-rs", "logs", "--bundle", "--yes", "--output", "out.zip"])
                .expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Logs(LogsArgs {
                path: false,
                latest: false,
                tail: None,
                bundle: true,
                output: Some(std::path::PathBuf::from("out.zip")),
                yes: true,
            }))
        );
    }

    #[test]
    fn cli_logs_rejects_conflicting_modes() {
        assert!(Cli::parse_from_args(["claude-rs", "logs", "--path", "--latest"]).is_err());
        assert!(Cli::parse_from_args(["claude-rs", "logs", "--output", "out.zip"]).is_err());
    }

    #[test]
    fn cli_config_accepts_path_modes() {
        let cli = Cli::parse_from_args(["claude-rs", "config", "path"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Path(ConfigPathArgs { json: false, which: None })),
            }))
        );

        let cli = Cli::parse_from_args([
            "claude-rs",
            "config",
            "path",
            "--json",
            "--which",
            "local-settings",
        ])
        .expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Path(ConfigPathArgs {
                    json: true,
                    which: Some(ConfigFileSelector::LocalSettings),
                })),
            }))
        );
    }

    #[test]
    fn cli_config_defaults_to_summary() {
        let cli = Cli::parse_from_args(["claude-rs", "config"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Config(ConfigArgs { command: None })));
    }

    #[test]
    fn cli_config_accepts_show_and_export() {
        let cli = Cli::parse_from_args(["claude-rs", "config", "show", "--json"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Show(ConfigShowArgs { json: true })),
            }))
        );

        let cli =
            Cli::parse_from_args(["claude-rs", "config", "export", "--output", "config.json"])
                .expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Export(ConfigExportArgs {
                    output: Some(std::path::PathBuf::from("config.json")),
                })),
            }))
        );
    }

    #[test]
    fn added_directory_must_be_an_existing_directory() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("notes.txt"), "").expect("file");
        for dir in ["missing", "notes.txt"] {
            let cli = Cli::parse_from_args(["claude-rs", "--add-dir", dir]).expect("parse");
            let error = cli.session_config.resolve(project.path()).expect_err(dir);
            let path = project.path().join(dir);
            assert_eq!(
                error,
                format!("--add-dir: {} is not an existing directory", path.display())
            );
        }
    }

    #[test]
    fn unreadable_settings_file_is_a_cli_error() {
        let project = tempfile::tempdir().expect("project");
        let cli = Cli::parse_from_args(["claude-rs", "--settings", "missing.json"]).expect("parse");
        let error = cli.session_config.resolve(project.path()).expect_err("missing file");
        let path = project.path().join("missing.json");
        assert!(
            error.starts_with(&format!("--settings: cannot read {}: ", path.display())),
            "{error}"
        );
    }

    #[test]
    fn fork_session_is_accepted_with_every_resume_spelling() {
        let cases: &[&[&str]] = &[
            &["claude-rs", "--resume", "--fork-session"],
            &["claude-rs", "-r", "session-1", "--fork-session"],
            &["claude-rs", "--continue", "--fork-session"],
            &["claude-rs", "resume", "--fork-session"],
            &["claude-rs", "resume", "session-1", "--fork-session"],
        ];
        for args in cases {
            let cli = Cli::parse_from_args(args.iter().copied()).expect("parse");
            cli.validate().expect("fork with a resume");
            assert!(cli.session_options.fork_session, "{args:?}");
        }
    }

    #[test]
    fn cli_exposes_package_version() {
        assert_eq!(Cli::command().get_version(), Some(env!("CARGO_PKG_VERSION")));
    }
}
