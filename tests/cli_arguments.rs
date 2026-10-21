// SPDX-License-Identifier: Apache-2.0

//! Command-line parsing and validation at the public `Cli` boundary. Kept out of the
//! library's unit tests: libtest builds one array of every test in a crate, and past
//! 2048 tests that array trips clippy's `large_stack_arrays` (16 KiB) in `lib test`.

use clap::CommandFactory;
use claude_code_rust::{Cli, Command, StartupLaunch};

#[test]
fn a_mistyped_command_is_refused_instead_of_sent_as_a_prompt() {
    for (args, typo, command) in [
        (&["claude-rs", "doctr"][..], "doctr", "doctor"),
        (&["claude-rs", "lgos"][..], "lgos", "logs"),
        (&["claude-rs", "Config"][..], "Config", "config"),
        (&["claude-rs", "-c", "resme"][..], "resme", "resume"),
        (&["claude-rs", "doctro"][..], "doctro", "doctor"),
        (&["claude-rs", "logss"][..], "logss", "logs"),
    ] {
        let error = Cli::parse_from_args(args.iter().copied()).expect_err("typo");
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand, "{args:?}");
        assert_eq!(error.exit_code(), 2);
        assert!(
            error.to_string().contains(&format!(
                "unknown command `{typo}`; did you mean `{command}`? To send it as a prompt, run `claude-rs -- {typo}`"
            )),
            "{error}"
        );
    }
}

#[test]
fn words_that_are_not_command_typos_stay_prompts() {
    for (args, prompt) in [
        (&["claude-rs", "--", "doctr"][..], "doctr"),
        (&["claude-rs", "--", "doctor"][..], "doctor"),
        (&["claude-rs", "--resume", "--", "doctr"][..], "doctr"),
        (&["claude-rs", "hello"][..], "hello"),
        (&["claude-rs", "go"][..], "go"),
        (&["claude-rs", "doctr the tests"][..], "doctr the tests"),
    ] {
        let cli = Cli::parse_from_args(args.iter().copied()).expect("prompt");
        cli.validate().expect("valid launch");
        assert_eq!(cli.initial_prompt(), Some(prompt), "{args:?}");
        assert!(cli.command.is_none());
    }
    // Ordinary words within a typo or two of a command name. An earlier rule (two
    // edits for longer names, one for short ones) refused every one of them.
    for word in [
        "rename",
        "result",
        "reuse",
        "assume",
        "docker",
        "factor",
        "actor",
        "confirm",
        "log",
        "logo",
        "lots",
        "dogs",
        "completion",
        "doctors",
        "configs",
        "resumes",
    ] {
        let cli = Cli::parse_from_args(["claude-rs", word]).unwrap_or_else(|error| {
            panic!("{word} was refused: {error}");
        });
        assert_eq!(cli.initial_prompt(), Some(word));
    }
    let cli = Cli::parse_from_args(["claude-rs", "resume"]).expect("resume");
    assert!(matches!(cli.command, Some(Command::Resume { .. })));
    let cli = Cli::parse_from_args(["claude-rs", "--resume", "doctr"]).expect("resume by id");
    assert_eq!(
        cli.startup_launch(),
        StartupLaunch::ResumeSession { session_id: "doctr".to_owned() }
    );
}

#[test]
fn usage_offers_a_prompt_or_a_command_but_not_both() {
    let usage = Cli::command().render_usage().to_string();
    let lines: Vec<&str> = usage.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        ["Usage: claude-rs [OPTIONS] [PROMPT]", "claude-rs [OPTIONS] <COMMAND>"],
        "{usage}"
    );
    let cli = Cli::parse_from_args(["claude-rs", "hello", "doctor"]).expect("parse");
    cli.validate().expect_err("a prompt and a command");
    // The override is the root's alone; subcommands keep their generated usage.
    let mut command = Cli::command();
    command.build();
    let doctor = command.find_subcommand_mut("doctor").expect("doctor").render_usage();
    assert_eq!(doctor.to_string(), "Usage: claude-rs doctor [OPTIONS]");
}

#[test]
fn log_append_requires_an_explicit_log_file() {
    let error = Cli::parse_from_args(["claude-rs", "--log-append"]).expect_err("no log file");
    assert_eq!(error.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    assert!(error.to_string().contains("--log-file"), "{error}");
    let cli = Cli::parse_from_args(["claude-rs", "--log-append", "--log-file", "run.log"])
        .expect("append to a log file");
    assert!(cli.log_append);
}

#[test]
fn an_invalid_log_filter_is_a_usage_error_quoting_the_given_text() {
    let error = Cli::parse_from_args(["claude-rs", "--log-filter", "app.render=loud"])
        .expect_err("invalid filter");
    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    assert_eq!(error.exit_code(), 2);
    let message = error.to_string();
    assert!(message.contains("'app.render=loud'"), "{message}");
    assert!(!message.contains("tui_markdown"), "{message}");
    let cli = Cli::parse_from_args(["claude-rs", "--log-filter", "info,app.render=trace"])
        .expect("valid filter");
    assert_eq!(cli.log_filter.as_deref(), Some("info,app.render=trace"));
}

#[test]
fn help_groups_diagnostics_options_after_session_options() {
    let help = Cli::command().render_help().to_string();
    let position = |text: &str| help.find(text).unwrap_or_else(|| panic!("{text}\n{help}"));
    let options = position("\nOptions:\n");
    let session = position("\nSession Options:\n");
    let diagnostics = position("\nDiagnostics Options:\n");
    let examples = position("\nExamples:\n");
    assert!(options < session && session < diagnostics && diagnostics < examples, "{help}");
    assert!((session..diagnostics).contains(&position("--model")), "{help}");
    for flag in ["--enable-logs", "--diagnostics-preset", "--log-file", "--log-filter"] {
        assert!((diagnostics..examples).contains(&position(flag)), "{flag}\n{help}");
    }
    assert!(!help[examples..].contains("--enable-logs"), "{help}");
    assert!(!help.contains("--bridge-script"), "{help}");

    let command = Cli::command();
    for id in ["enable_logs", "diagnostics_preset", "log_file", "log_filter"] {
        let arg = command.get_arguments().find(|arg| arg.get_id() == id).expect(id);
        let text = arg.get_help().map(ToString::to_string).unwrap_or_default();
        assert!(text.starts_with("Turn on detailed logging"), "{id}: {text}");
    }
}

#[test]
fn session_flags_before_and_after_resume_parse_to_the_same_launch() {
    let after = Cli::parse_from_args([
        "claude-rs",
        "resume",
        "session-1",
        "--model",
        "opus",
        "--add-dir",
        "src",
        "--fork-session",
    ])
    .expect("after resume");
    let before = Cli::parse_from_args([
        "claude-rs",
        "--model",
        "opus",
        "--add-dir",
        "src",
        "--fork-session",
        "resume",
        "session-1",
    ])
    .expect("before resume");
    let split = Cli::parse_from_args([
        "claude-rs",
        "--model",
        "opus",
        "resume",
        "session-1",
        "--add-dir",
        "src",
        "--fork-session",
    ])
    .expect("split around resume");
    for cli in [&after, &before, &split] {
        cli.validate().expect("valid launch");
        assert_eq!(cli.session_options.model.as_deref(), Some("opus"));
        assert!(cli.session_options.fork_session);
        assert_eq!(cli.session_config.add_dirs, [std::path::PathBuf::from("src")]);
        assert_eq!(
            cli.startup_launch(),
            StartupLaunch::ResumeSession { session_id: "session-1".to_owned() }
        );
    }
    assert_eq!(after.command, before.command);
    assert_eq!(split.command, before.command);
}
