// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

pub mod agent;
pub mod app;
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

#[derive(Parser, Debug)]
#[command(
    name = "claude-rs",
    version = env!("CARGO_PKG_VERSION"),
    about = "Native Rust terminal for Claude Code"
)]
#[command(
    after_help = "Examples:\n  claude-rs --enable-logs --diagnostics-preset session\n  claude-rs --enable-logs --diagnostics-preset render"
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

    /// Continue the most recent session in the working directory (including git worktrees).
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
    #[arg(long)]
    pub bridge_script: Option<std::path::PathBuf>,

    /// Enable detailed info-level diagnostics in addition to the always-on warning/error baseline.
    #[arg(long)]
    pub enable_logs: bool,

    /// Named diagnostics preset for common logging workflows.
    /// Ignored when `--log-filter` is provided explicitly.
    #[arg(long, value_enum)]
    pub diagnostics_preset: Option<DiagnosticsPreset>,

    /// Write tracing diagnostics to a file.
    ///
    /// When omitted, interactive runs use a timestamped default log path.
    #[arg(long, value_name = "PATH")]
    pub log_file: Option<std::path::PathBuf>,

    /// Tracing filter directives (example: `info,app.render=trace`).
    /// Overrides `--diagnostics-preset` and falls back to `RUST_LOG` when omitted.
    #[arg(long, value_name = "FILTER")]
    pub log_filter: Option<String>,

    /// Append to an explicit `--log-file`.
    ///
    /// Without `--log-file`, appends to the legacy shared default log for compatibility.
    #[arg(long)]
    pub log_append: bool,

    // Last, because a flattened help heading also applies to the arguments that follow it.
    #[command(flatten)]
    pub session_options: SessionOptions,

    #[command(flatten)]
    pub session_config: SessionConfigArgs,
}

/// Per-launch options, separate from persisted application preferences.
#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Session Options")]
pub struct SessionOptions {
    /// Starting model alias or full model ID; does not change saved preferences.
    #[arg(long, global = true, value_name = "MODEL", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub model: Option<String>,

    /// Starting reasoning effort; availability depends on the selected model.
    #[arg(long, global = true, value_enum)]
    pub effort: Option<agent::model::EffortLevel>,

    /// Starting permission mode; does not change saved preferences.
    #[arg(long, global = true, value_enum)]
    pub permission_mode: Option<permission_mode::DefaultPermissionMode>,

    /// Main-thread agent to use for this session.
    #[arg(long, global = true, value_name = "NAME", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub agent: Option<String>,

    /// Name the new or resumed session.
    #[arg(long, short = 'n', global = true, value_name = "NAME", value_parser = parse_session_name)]
    pub name: Option<String>,

    /// Resume into a new session ID, leaving the original session unchanged.
    #[arg(long, global = true)]
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
#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Session Options")]
pub struct SessionConfigArgs {
    /// Additional directory Claude may access (repeatable).
    #[arg(long = "add-dir", global = true, value_name = "DIR")]
    pub add_dirs: Vec<std::path::PathBuf>,

    /// Settings JSON file, or inline JSON, applied as Claude Code's --settings layer.
    #[arg(long, global = true, value_name = "FILE-OR-JSON")]
    pub settings: Option<String>,

    /// MCP server config file, or inline JSON, with an `mcpServers` object (repeatable).
    #[arg(long = "mcp-config", global = true, value_name = "FILE-OR-JSON")]
    pub mcp_configs: Vec<String>,

    /// Use only the MCP servers from --mcp-config, ignoring all other MCP configuration.
    #[arg(long, global = true)]
    pub strict_mcp_config: bool,

    /// Replace Claude Code's system prompt for new sessions.
    #[arg(long, global = true, value_name = "TEXT", conflicts_with = "system_prompt_file", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub system_prompt: Option<String>,

    /// Replace Claude Code's system prompt for new sessions with a file's text.
    #[arg(long, global = true, value_name = "FILE")]
    pub system_prompt_file: Option<std::path::PathBuf>,

    /// Append text to the system prompt of new sessions.
    #[arg(long, global = true, value_name = "TEXT", conflicts_with = "append_system_prompt_file", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub append_system_prompt: Option<String>,

    /// Append a file's text to the system prompt of new sessions.
    #[arg(long, global = true, value_name = "FILE")]
    pub append_system_prompt_file: Option<std::path::PathBuf>,
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
    pub fn validate(&self) -> Result<(), clap::Error> {
        use clap::CommandFactory;
        let has_session_options = self.session_options.model.is_some()
            || self.session_options.effort.is_some()
            || self.session_options.permission_mode.is_some()
            || self.session_options.agent.is_some()
            || self.session_options.name.is_some()
            || self.session_options.fork_session
            || !self.session_config.add_dirs.is_empty()
            || self.session_config.settings.is_some()
            || !self.session_config.mcp_configs.is_empty()
            || self.session_config.strict_mcp_config
            || self.session_config.system_prompt.is_some()
            || self.session_config.system_prompt_file.is_some()
            || self.session_config.append_system_prompt.is_some()
            || self.session_config.append_system_prompt_file.is_some();
        let conflict = match &self.command {
            Some(Command::Resume { .. }) => {
                self.resume.is_some() || self.continue_session || self.prompt.is_some()
            }
            Some(_) => {
                self.resume.is_some()
                    || self.continue_session
                    || self.prompt.is_some()
                    || has_session_options
            }
            None => false,
        };
        if conflict {
            return Err(Self::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "interactive startup arguments cannot be combined with this subcommand",
            ));
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

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    /// Resume a previous session by exact ID or title, or pick from recent sessions
    Resume {
        /// Session ID or displayed title to resume directly. Omit to show a session picker.
        session_id: Option<String>,
        /// Initial prompt to send after resuming.
        prompt: Option<String>,
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
        ConfigPathArgs, ConfigShowArgs, DoctorArgs, LogsArgs,
    };
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_without_subcommand_starts_new_session() {
        let cli = Cli::try_parse_from(["claude-rs"]).expect("parse");
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_resume_without_id_requests_picker() {
        let cli = Cli::try_parse_from(["claude-rs", "resume"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Resume { session_id: None, prompt: None }));
    }

    #[test]
    fn cli_resume_with_id_resumes_directly() {
        let cli = Cli::try_parse_from(["claude-rs", "resume", "abc-123"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Resume { session_id: Some("abc-123".to_owned()), prompt: None })
        );
    }

    #[test]
    fn cli_resume_flag_selects_the_same_session_as_the_subcommand() {
        let flag = Cli::try_parse_from(["claude-rs", "--resume", "abc-123"]).expect("flag");
        let command = Cli::try_parse_from(["claude-rs", "resume", "abc-123"]).expect("command");
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
            let error = Cli::try_parse_from(args.iter().copied()).expect_err("removed flag");
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
        let cli = Cli::try_parse_from(["claude-rs", "doctor"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Doctor(DoctorArgs { json: false, strict: false })));
    }

    #[test]
    fn cli_doctor_accepts_json_and_strict() {
        let cli =
            Cli::try_parse_from(["claude-rs", "doctor", "--json", "--strict"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Doctor(DoctorArgs { json: true, strict: true })));
    }

    #[test]
    fn cli_logs_defaults_to_summary() {
        let cli = Cli::try_parse_from(["claude-rs", "logs"]).expect("parse");
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
        let cli = Cli::try_parse_from(["claude-rs", "logs", "--tail", "200"]).expect("parse");
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
            Cli::try_parse_from(["claude-rs", "logs", "--bundle", "--yes", "--output", "out.zip"])
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
        assert!(Cli::try_parse_from(["claude-rs", "logs", "--path", "--latest"]).is_err());
        assert!(Cli::try_parse_from(["claude-rs", "logs", "--output", "out.zip"]).is_err());
    }

    #[test]
    fn cli_config_accepts_path_modes() {
        let cli = Cli::try_parse_from(["claude-rs", "config", "path"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Path(ConfigPathArgs { json: false, which: None })),
            }))
        );

        let cli = Cli::try_parse_from([
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
        let cli = Cli::try_parse_from(["claude-rs", "config"]).expect("parse");
        assert_eq!(cli.command, Some(Command::Config(ConfigArgs { command: None })));
    }

    #[test]
    fn cli_config_accepts_show_and_export() {
        let cli = Cli::try_parse_from(["claude-rs", "config", "show", "--json"]).expect("parse");
        assert_eq!(
            cli.command,
            Some(Command::Config(ConfigArgs {
                command: Some(ConfigCommand::Show(ConfigShowArgs { json: true })),
            }))
        );

        let cli = Cli::try_parse_from(["claude-rs", "config", "export", "--output", "config.json"])
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
            let cli = Cli::try_parse_from(["claude-rs", "--add-dir", dir]).expect("parse");
            let error = cli.session_config.resolve(project.path()).expect_err(dir);
            let path = project.path().join(dir);
            assert_eq!(
                error,
                format!("--add-dir: {} is not an existing directory", path.display())
            );
        }
    }

    #[test]
    fn working_directory_must_be_an_existing_directory() {
        let project = tempfile::tempdir().expect("project");
        let file = project.path().join("notes.txt");
        std::fs::write(&file, "").expect("file");
        let missing = project.path().join("missing");
        for dir in [&missing, &file] {
            let error = Cli::try_parse_from(["claude-rs".as_ref(), "-C".as_ref(), dir.as_os_str()])
                .expect_err("not a directory");
            assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
            assert_eq!(error.exit_code(), 2);
            let message = error.to_string();
            assert!(
                message.contains(&format!("{} is not an existing directory", dir.display())),
                "{message}"
            );
        }
    }

    #[test]
    fn a_missing_working_directory_is_blamed_before_added_directories() {
        let project = tempfile::tempdir().expect("project");
        let missing = project.path().join("missing");
        let error = Cli::try_parse_from([
            "claude-rs".as_ref(),
            "-C".as_ref(),
            missing.as_os_str(),
            "--add-dir".as_ref(),
            "x".as_ref(),
        ])
        .expect_err("missing working directory");
        let message = error.to_string();
        assert!(message.contains("--dir"), "{message}");
        assert!(!message.contains(&missing.join("x").display().to_string()), "{message}");
    }

    #[test]
    fn relative_working_directory_is_made_absolute() {
        let cli = Cli::try_parse_from(["claude-rs", "-C", "src"]).expect("parse");
        let expected = std::env::current_dir().expect("cwd").join("src");
        assert_eq!(cli.dir.as_deref(), Some(expected.as_path()));
        assert_eq!(cli.startup_cwd(), expected);
    }

    #[test]
    fn bridge_script_must_be_an_existing_file_for_an_interactive_launch() {
        let project = tempfile::tempdir().expect("project");
        let missing = project.path().join("bridge.js");
        for script in [&missing, &project.path().to_path_buf()] {
            let cli = Cli::try_parse_from([
                "claude-rs".as_ref(),
                "--bridge-script".as_ref(),
                script.as_os_str(),
            ])
            .expect("parse");
            cli.validate().expect("valid arguments");
            let error = cli.session_config().expect_err("missing bridge script");
            assert_eq!(error.exit_code(), 2);
            let message = error.to_string();
            assert!(
                message.contains(&format!(
                    "--bridge-script: {} is not an existing file",
                    script.display()
                )),
                "{message}"
            );
        }
        std::fs::write(&missing, "").expect("bridge script");
        let cli = Cli::try_parse_from([
            "claude-rs".as_ref(),
            "--bridge-script".as_ref(),
            missing.as_os_str(),
        ])
        .expect("parse");
        cli.session_config().expect("existing bridge script");
    }

    #[test]
    fn unreadable_settings_file_is_a_cli_error() {
        let project = tempfile::tempdir().expect("project");
        let cli = Cli::try_parse_from(["claude-rs", "--settings", "missing.json"]).expect("parse");
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
            let cli = Cli::try_parse_from(args.iter().copied()).expect("parse");
            cli.validate().expect("fork with a resume");
            assert!(cli.session_options.fork_session, "{args:?}");
        }
    }

    #[test]
    fn cli_exposes_package_version() {
        assert_eq!(Cli::command().get_version(), Some(env!("CARGO_PKG_VERSION")));
    }
}
