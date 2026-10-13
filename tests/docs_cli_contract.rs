// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

//! Drift check between the `claude-rs` CLI and the CLI Options table in the manual.
//!
//! Only long flag names and their order are enforced. Prose in the Purpose column
//! is free to diverge, so documenting a flag well is never a test failure.

use clap::CommandFactory;
use claude_code_rust::Cli;
use claude_code_rust::error::AppError;

/// Long flags clap generates on its own. They are intentionally not in the table, and
/// neither are hidden flags, which `--help` does not show.
const IMPLICIT_ARGS: &[&str] = &["help", "version"];

/// Extract the long flag from the first column of a CLI Options row.
///
/// The cell may carry a short alias and a value placeholder, as in
/// `` `-C, --dir <DIR>` ``, and a placeholder may itself contain `|`, which
/// splits the cell early. Scanning for the first `--` token survives both.
fn long_flag_in_cell(cell: &str) -> Option<String> {
    let after_dashes = &cell[cell.find("--")? + 2..];
    let end = after_dashes
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(after_dashes.len());
    let flag = &after_dashes[..end];
    (!flag.is_empty()).then(|| flag.to_owned())
}

/// Collect the long flags listed in the CLI Options table of the manual.
///
/// The manual is read from disk rather than with `include_str!` so docs are not
/// embedded in any shipped artifact.
fn documented_long_flags() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/src/usage.md");
    let markdown = std::fs::read_to_string(path);
    assert!(markdown.is_ok(), "failed to read {path}");
    let markdown = markdown.unwrap_or_default();

    let mut flags = Vec::new();
    let mut in_options_section = false;

    for line in markdown.lines() {
        let line = line.trim();
        if let Some(heading) = line.strip_prefix("## ") {
            in_options_section = heading == "CLI Options";
            continue;
        }
        if !in_options_section || !line.starts_with('|') {
            continue;
        }
        let Some(cell) = line.split('|').nth(1) else {
            continue;
        };
        let cell = cell.trim();
        // The header and separator rows carry no backticked flag.
        if !cell.starts_with('`') {
            continue;
        }
        if let Some(flag) = long_flag_in_cell(cell) {
            flags.push(flag);
        }
    }

    flags
}

/// Every failure the app reports. The match makes a new variant a compile error here
/// until it is listed, so its exit code cannot go undocumented.
fn every_app_error() -> Vec<AppError> {
    let errors = vec![
        AppError::BridgeRuntimeNotFound,
        AppError::BridgeSpawnFailed,
        AppError::BridgeInitializationFailed,
        AppError::BridgeStdoutClosed,
        AppError::BridgeSdkFailure,
        AppError::BridgeTimeout,
        AppError::AdapterCrashed,
        AppError::ConnectionFailed,
        AppError::SessionResumeFailed(String::new()),
        AppError::AuthRequired,
    ];
    for error in &errors {
        match error {
            AppError::BridgeRuntimeNotFound
            | AppError::BridgeSpawnFailed
            | AppError::BridgeInitializationFailed
            | AppError::BridgeStdoutClosed
            | AppError::BridgeSdkFailure
            | AppError::BridgeTimeout
            | AppError::AdapterCrashed
            | AppError::ConnectionFailed
            | AppError::SessionResumeFailed(_)
            | AppError::AuthRequired => {}
        }
    }
    errors
}

/// Rows of a two-column table under `## {heading}` in the manual: first cell, unquoted.
fn table_keys(heading: &str) -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/src/usage.md");
    let markdown = std::fs::read_to_string(path).unwrap_or_default();
    let mut in_section = false;
    let mut keys = Vec::new();
    for line in markdown.lines().map(str::trim) {
        if let Some(title) = line.strip_prefix("## ") {
            in_section = title == heading;
            continue;
        }
        if let Some(cell) = line.split('|').nth(1).map(str::trim).filter(|_| in_section)
            && let Some(key) = cell.strip_prefix('`').and_then(|cell| cell.strip_suffix('`'))
        {
            keys.push(key.to_owned());
        }
    }
    keys
}

#[test]
fn every_exit_status_is_documented_in_help_and_manual() {
    let mut codes: Vec<i32> = every_app_error().iter().map(AppError::exit_code).collect();
    codes.extend([0, 1, 2, AppError::NO_TERMINAL_EXIT_CODE]);
    codes.sort_unstable();
    codes.dedup();

    let documented = table_keys("Exit Status");
    let help = Cli::command().render_long_help().to_string();
    let help_section = &help[help.find("Exit status:").expect("exit status in --help")..];
    for code in codes.iter().map(i32::to_string) {
        assert!(documented.contains(&code), "exit status {code} missing from docs/src/usage.md");
        assert!(
            help_section.lines().any(|line| line.split_whitespace().next() == Some(&code)),
            "exit status {code} missing from --help"
        );
    }
    assert_eq!(documented.len(), codes.len(), "undocumented or stale rows: {documented:?}");
}

#[test]
fn every_documented_environment_variable_is_in_help() {
    let documented = table_keys("Environment");
    assert!(documented.iter().any(|name| name == "CLAUDE_RS_AGENT_BRIDGE"), "{documented:?}");
    let help = Cli::command().render_long_help().to_string();
    let help_section = &help[help.find("Environment:").expect("environment in --help")..];
    let in_help: Vec<&str> =
        help_section.lines().skip(1).filter_map(|line| line.split_whitespace().next()).collect();
    for name in &documented {
        assert!(in_help.contains(&name.as_str()), "{name} missing from --help");
    }
}

#[test]
fn docs_cli_options_table_matches_clap() {
    let command = Cli::command();
    let expected: Vec<String> = command
        .get_arguments()
        .filter(|arg| !IMPLICIT_ARGS.contains(&arg.get_id().as_str()) && !arg.is_hide_set())
        .filter_map(|arg| arg.get_long().map(str::to_owned))
        .collect();

    let documented = documented_long_flags();

    for flag in &expected {
        assert!(
            documented.contains(flag),
            "--{flag} is a `claude-rs` option but is missing from the CLI Options table in docs/src/usage.md"
        );
    }
    for flag in &documented {
        assert!(
            expected.contains(flag),
            "--{flag} is listed in the CLI Options table in docs/src/usage.md but is not a `claude-rs` option"
        );
    }
    assert_eq!(
        documented, expected,
        "the CLI Options table in docs/src/usage.md must list flags in `Cli` declaration order"
    );
}
