// SPDX-License-Identifier: Apache-2.0

// The test harness hands every test to its runner as one generated local array of references.
// Past 2048 tests that array exceeds Clippy's 16 KiB `large_stack_arrays` limit, which it reports
// without a span. Test builds only; production code keeps the lint.
#![cfg_attr(test, allow(clippy::large_stack_arrays))]

pub mod agent;
pub mod app;
pub mod appearance;
mod claude_paths;
pub mod cli;
mod epoch_time;
pub mod error;
pub mod failure;
pub mod install_method;
mod json_file;
pub mod logging;
mod markdown;
pub mod permission_mode;
pub mod session_config;
pub mod ui;

use clap::{Args, Parser, Subcommand, ValueEnum};
pub use session_config::SessionConfigArgs;

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
    #[arg(long, short = 'C')]
    pub dir: Option<std::path::PathBuf>,

    /// Path to the agent bridge script (defaults to agent-sdk/dist/bridge.js).
    // Checked for interactive launches only (`Cli::validate`): support commands
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
    ///
    /// The baseline log already records warnings and errors from the app, the bridge and the SDK.
    /// Run `claude-rs logs` to find it.
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

    /// Name a new session, or rename the one being resumed.
    #[arg(long, short = 'n', value_name = "TEXT", value_parser = parse_session_name)]
    pub name: Option<String>,

    /// Resume under a new session ID, leaving the original unchanged.
    #[arg(long)]
    pub fork_session: bool,
}

/// Session names are shown in the terminal, so they go through the one title sanitiser.
fn parse_session_name(value: &str) -> Result<String, String> {
    session_config::sanitize_session_title(value)
        .ok_or_else(|| "the session name must contain visible characters".to_owned())
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
        if matches!(self.command, None | Some(Command::Resume { .. })) {
            self.validate_launch_paths()?;
        }
        Ok(())
    }

    /// The session working directory: [`Cli::working_dir`], or `.` when the process
    /// directory cannot be read.
    #[must_use]
    pub fn startup_cwd(&self) -> std::path::PathBuf {
        self.working_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    /// Resolve the session configuration flags against the session working directory.
    pub fn resolve_session_config(&self) -> Result<agent::wire::SessionConfig, clap::Error> {
        use clap::CommandFactory;
        self.session_config.resolve(&self.startup_cwd()).map_err(|message| {
            Self::command().error(clap::error::ErrorKind::ValueValidation, message)
        })
    }

    /// Checks `--dir` and `--bridge-script` before the terminal, trust prompt or bridge
    /// exist, so a bad path is a usage error instead of a failure further in. Only
    /// interactive launches use them: `doctor` reports on a bad bridge script rather
    /// than refusing it, and the other support commands never start the bridge.
    fn validate_launch_paths(&self) -> Result<(), clap::Error> {
        use clap::CommandFactory;
        let usage_error = |id: &str, reason: String| {
            let mut command = Self::command();
            command.build();
            // clap renders the label the way its own value errors do ("--dir <DIR>").
            let flag = command
                .get_arguments()
                .find(|arg| arg.get_id() == id)
                .map_or_else(|| id.to_owned(), ToString::to_string);
            command.error(
                clap::error::ErrorKind::ValueValidation,
                format!("invalid value for '{flag}': {reason}"),
            )
        };
        if let Some(reason) = self.dir.as_deref().and_then(working_dir_problem) {
            return Err(usage_error("dir", reason));
        }
        // The same inspection `doctor` reports, so the two judge a script alike.
        if let Some(reason) = self
            .bridge_script
            .as_deref()
            .and_then(|script| agent::bridge::inspect_bridge_script(Some(script)).error)
        {
            return Err(usage_error("bridge_script", reason));
        }
        Ok(())
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

    /// The directory `--dir` names, or the process directory without it. The one answer
    /// for the session, `config` and `doctor`, so they never disagree about `--dir .`.
    ///
    /// # Errors
    ///
    /// Fails when the process directory is needed and cannot be read.
    pub fn working_dir(&self) -> std::io::Result<std::path::PathBuf> {
        resolve_working_dir(self.dir.as_deref(), std::env::current_dir)
    }
}

/// An absolute `--dir` is used as given. A relative one is joined to the process directory
/// and resolved the way the platform resolves its own working directory, so `--dir .`
/// names the same directory as no `--dir` at all: physically on Unix, where the process
/// directory is already physical, and lexically on Windows, which keeps a mapped drive
/// letter and adds no `\\?\` prefix. A relative `--dir` that cannot be resolved (support
/// commands do not validate it, and a launch's may vanish after validation) is kept
/// joined, so the failure surfaces where the directory is used.
fn resolve_working_dir(
    dir: Option<&std::path::Path>,
    process_cwd: impl FnOnce() -> std::io::Result<std::path::PathBuf>,
) -> std::io::Result<std::path::PathBuf> {
    let Some(dir) = dir else {
        return process_cwd();
    };
    if dir.is_absolute() {
        return Ok(dir.to_path_buf());
    }
    let joined = process_cwd()?.join(dir);
    Ok(resolve_relative_dir(&joined).unwrap_or(joined))
}

#[cfg(windows)]
fn resolve_relative_dir(joined: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::path::absolute(joined)
}

#[cfg(not(windows))]
fn resolve_relative_dir(joined: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    joined.canonicalize()
}

/// Why `dir` cannot be the session's working directory, judged the way the session will
/// use it: relative to the current directory, with no `~` expansion of its own. The
/// session lists the directory (read permission) and enters it and opens files inside it
/// (search permission), so both are required.
fn working_dir_problem(dir: &std::path::Path) -> Option<String> {
    let shown = dir.display();
    match std::fs::metadata(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Some(format!("directory does not exist: {shown}"));
        }
        Err(err) => return Some(format!("directory cannot be accessed: {shown}: {err}")),
        Ok(metadata) if !metadata.is_dir() => return Some(format!("not a directory: {shown}")),
        Ok(_) => {}
    }
    if let Err(err) = std::fs::read_dir(dir) {
        return Some(format!("directory cannot be read: {shown}: {err}"));
    }
    // Looking up `.` inside the directory is a path lookup through it, which needs search
    // permission and nothing else: a directory with read but no search permission lists
    // but fails here, as entering it would. Windows has no search bit and passes.
    std::fs::metadata(dir.join("."))
        .err()
        .map(|err| format!("directory cannot be entered: {shown}: {err}"))
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
    use super::Cli;
    use clap::Parser;

    /// The fixture root as the platform spells its own working directory: physical on Unix
    /// (macOS temp dirs sit behind a symlink), as given on Windows.
    fn platform_root(path: &std::path::Path) -> std::path::PathBuf {
        if cfg!(windows) {
            path.to_path_buf()
        } else {
            path.canonicalize().expect("canonical tempdir")
        }
    }

    #[test]
    fn a_relative_dir_resolves_to_the_absolute_directory_it_names() {
        let base = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(base.path().join("project").join("sub")).expect("project");
        // Every expected child is spelled out by name under the fixture root.
        let root = platform_root(base.path());
        let cases = [
            (".", root.clone()),
            ("project", root.join("project")),
            ("./project/./sub", root.join("project").join("sub")),
            ("project/sub/..", root.join("project")),
        ];
        for (dir, expected) in cases {
            let resolved = super::resolve_working_dir(Some(std::path::Path::new(dir)), || {
                Ok(base.path().into())
            })
            .expect("resolve");
            assert_eq!(resolved, expected, "--dir {dir}");
        }
    }

    #[test]
    fn an_absolute_dir_is_used_as_given_even_without_a_process_dir() {
        let base = tempfile::tempdir().expect("tempdir");
        let project = base.path().join("project");
        std::fs::create_dir_all(&project).expect("project");

        let resolved = super::resolve_working_dir(Some(&project), || {
            Err(std::io::Error::other("process directory unavailable"))
        })
        .expect("absolute dir needs no process dir");

        assert_eq!(resolved, project);
    }

    #[test]
    fn session_names_lose_control_characters_and_must_keep_visible_text() {
        let cli = Cli::try_parse_from(["claude-rs", "-n", "Release\u{7} prep "]).expect("name");
        assert_eq!(cli.session_options.name.as_deref(), Some("Release prep"));
        for name in ["", "\u{1b}\t "] {
            assert!(Cli::try_parse_from(["claude-rs", "--name", name]).is_err(), "{name:?}");
        }
    }

    #[test]
    fn session_configuration_resolves_against_the_working_directory_option() {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir(project.path().join("docs")).expect("docs");
        let dir = project.path().to_str().expect("UTF-8 path");
        let cli =
            Cli::try_parse_from(["claude-rs", "-C", dir, "--add-dir", "docs"]).expect("parse");
        let config = cli.resolve_session_config().expect("resolved");
        assert_eq!(
            config.additional_directories,
            [project.path().join("docs").to_str().expect("UTF-8 path")]
        );
        let cli =
            Cli::try_parse_from(["claude-rs", "-C", dir, "--add-dir", "absent"]).expect("parse");
        let error = cli.resolve_session_config().expect_err("missing directory");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn logs_modes_reject_only_declared_conflicts_and_require_bundle_for_output() {
        use clap::error::ErrorKind;
        let modes: &[&[&str]] = &[&["--path"], &["--latest"], &["--tail", "2"], &["--bundle"]];
        for (index, mode) in modes.iter().enumerate() {
            let mut args = vec!["claude-rs", "logs"];
            args.extend_from_slice(mode);
            assert!(Cli::try_parse_from(&args).is_ok());
            for other in &modes[index + 1..] {
                let mut conflicting = args.clone();
                conflicting.extend_from_slice(other);
                assert_eq!(
                    Cli::try_parse_from(conflicting).unwrap_err().kind(),
                    ErrorKind::ArgumentConflict
                );
            }
        }
        let error =
            Cli::try_parse_from(["claude-rs", "logs", "--output", "selected.zip"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--bundle"));
        assert!(
            Cli::try_parse_from([
                "claude-rs",
                "logs",
                "--bundle",
                "--yes",
                "--output",
                "selected.zip"
            ])
            .is_ok()
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
}

#[cfg(test)]
mod cli_launch_audit_tests;

#[cfg(test)]
mod diagnostics_test_support;
