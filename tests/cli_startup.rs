// SPDX-License-Identifier: Apache-2.0

use claude_code_rust::{Cli, StartupLaunch};
use std::process::Command;

#[test]
fn interactive_startup_flags_normalize_to_one_launch() {
    let cases = [
        (vec!["claude-rs"], StartupLaunch::NewSession, None),
        (
            vec!["claude-rs", "explain this project"],
            StartupLaunch::NewSession,
            Some("explain this project"),
        ),
        (vec!["claude-rs", "--resume"], StartupLaunch::SessionPicker, None),
        (
            vec!["claude-rs", "--resume", "--", "initial prompt"],
            StartupLaunch::SessionPicker,
            Some("initial prompt"),
        ),
        (
            vec!["claude-rs", "--resume=session-1", "initial prompt"],
            StartupLaunch::ResumeSession { session_id: "session-1".into() },
            Some("initial prompt"),
        ),
        (
            vec!["claude-rs", "-r", "session-1", "continue reviewing"],
            StartupLaunch::ResumeSession { session_id: "session-1".into() },
            Some("continue reviewing"),
        ),
        (
            vec!["claude-rs", "resume", "session-1", "continue reviewing"],
            StartupLaunch::ResumeSession { session_id: "session-1".into() },
            Some("continue reviewing"),
        ),
        (vec!["claude-rs", "resume"], StartupLaunch::SessionPicker, None),
        (
            vec!["claude-rs", "-c", "continue reviewing"],
            StartupLaunch::ContinueSession,
            Some("continue reviewing"),
        ),
    ];
    for (args, launch, prompt) in cases {
        let cli = Cli::parse_from_args(&args).expect("parse startup");
        cli.validate().expect("valid launch");
        assert_eq!(cli.startup_launch(), launch, "{args:?}");
        assert_eq!(cli.initial_prompt(), prompt, "{args:?}");
    }
}

#[test]
fn session_options_support_existing_resume_syntax() {
    let cli = Cli::parse_from_args([
        "claude-rs",
        "resume",
        "session-1",
        "--model",
        "opus",
        "--effort",
        "max",
        "--permission-mode",
        "plan",
        "--agent",
        "reviewer",
    ])
    .expect("parse options");
    cli.validate().expect("valid launch");
    assert_eq!(cli.session_options.model.as_deref(), Some("opus"));
    assert_eq!(cli.session_options.effort, Some(claude_code_rust::agent::model::EffortLevel::Max));
    assert_eq!(cli.session_options.permission_mode.unwrap().as_stored(), "plan");
    assert_eq!(cli.session_options.agent.as_deref(), Some("reviewer"));
}

#[test]
fn contradictory_or_invalid_startup_arguments_are_rejected() {
    for args in [
        vec!["claude-rs", "--continue", "--resume", "session-1"],
        vec!["claude-rs", "--resume", "session-1", "resume"],
        vec!["claude-rs", "--continue", "doctor"],
        vec!["claude-rs", "doctor", "--model", "opus"],
        vec!["claude-rs", "--effort", "invalid"],
        vec!["claude-rs", "--permission-mode", "invalid"],
        vec!["claude-rs", "--model", ""],
        vec!["claude-rs", "--agent", ""],
        vec!["claude-rs", "--name", ""],
        vec!["claude-rs", "-n", "\u{1b}\t "],
        vec!["claude-rs", "doctor", "--name", "Release prep"],
        vec!["claude-rs", "--system-prompt", "a", "--system-prompt-file", "prompt.md"],
        vec!["claude-rs", "--append-system-prompt", "a", "--append-system-prompt-file", "a.md"],
        vec!["claude-rs", "--system-prompt", ""],
        vec!["claude-rs", "doctor", "--append-system-prompt", "a"],
        vec!["claude-rs", "--fork-session"],
        vec!["claude-rs", "--fork-session", "explain this project"],
    ] {
        assert!(
            Cli::parse_from_args(&args).and_then(|cli| cli.validate()).is_err(),
            "accepted {args:?}"
        );
    }
}

#[test]
fn executable_generates_every_shell_without_a_bridge() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
            .args(["--bridge-script", "missing-bridge.js", "completions", shell])
            .output()
            .expect("run completions");
        assert!(output.status.success(), "{shell}: {}", String::from_utf8_lossy(&output.stderr));
        let script = String::from_utf8(output.stdout).expect("UTF-8 script");
        assert!(script.contains("claude-rs"), "{shell}");
        assert!(script.contains("permission-mode"), "{shell}");
        assert!(script.contains("model"), "{shell}");
    }
}

#[test]
fn executable_rejects_startup_conflicts_before_resolving_the_bridge() {
    for args in [
        vec!["--continue", "--resume", "session-1"],
        vec!["doctor", "--model", "opus"],
        vec!["--effort", "invalid"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
            .args(["--bridge-script", "missing-bridge.js"])
            .args(&args)
            .output()
            .expect("run invalid arguments");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("error:"), "{error}");
        assert!(!error.contains("missing-bridge.js"), "runtime resolved the bridge: {error}");
    }
}

#[test]
fn executable_rejects_a_missing_added_directory_before_resolving_the_bridge() {
    let project = tempfile::tempdir().expect("project");
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .args(["--bridge-script", "missing-bridge.js", "--add-dir", "not-there", "-C"])
        .arg(project.path())
        .output()
        .expect("run with a missing directory");
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    let missing = project.path().join("not-there");
    assert!(
        error.contains(&format!("--add-dir: {} is not an existing directory", missing.display())),
        "{error}"
    );
    assert!(!error.contains("missing-bridge.js"), "runtime resolved the bridge: {error}");
}

#[test]
fn executable_rejects_a_bad_working_directory_or_bridge_script_as_usage_errors() {
    let temp = tempfile::tempdir().expect("temp");
    let missing = temp.path().join("missing");
    let file = temp.path().join("notes.txt");
    std::fs::write(&file, "").expect("file");
    let cases: [(&[&std::ffi::OsStr], String); 3] = [
        (
            &["-C".as_ref(), missing.as_os_str()],
            format!("{} is not an existing directory", missing.display()),
        ),
        (
            &["-C".as_ref(), file.as_os_str()],
            format!("{} is not an existing directory", file.display()),
        ),
        (
            &["--bridge-script".as_ref(), missing.as_os_str()],
            format!("--bridge-script: {} is not an existing file", missing.display()),
        ),
    ];
    for (args, expected) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run with a bad path");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(&expected), "{error}");
        assert!(!error.contains("doctor"), "usage error suggests doctor: {error}");
    }
}

#[test]
fn executable_refuses_a_mistyped_command_but_sends_it_after_a_separator() {
    let home = tempfile::tempdir().expect("home");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_claude-rs"))
            .args(args)
            .env("HOME", home.path())
            .env("CLAUDE_CONFIG_DIR", home.path().join("profile"))
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run claude-rs")
    };

    let typo = run(&["doctr"]);
    assert_eq!(typo.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&typo.stderr).contains("did you mean `doctor`?"),
        "{}",
        String::from_utf8_lossy(&typo.stderr)
    );

    // Past argument checks, the prompt launch stops only at the missing terminal.
    let escaped = run(&["--", "doctr"]);
    assert_eq!(escaped.status.code(), Some(25), "{}", String::from_utf8_lossy(&escaped.stderr));
}

#[test]
fn interactive_launch_without_a_terminal_exits_before_terminal_setup_and_logging() {
    let home = tempfile::tempdir().expect("home");
    let project = home.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .arg("hello")
        .current_dir(&project)
        // The runtime log directory derives from the home directory on Unix.
        .env("HOME", home.path())
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env("CLAUDE_CONFIG_DIR", home.path().join("profile"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run without a terminal");

    assert_eq!(output.status.code(), Some(25));
    assert!(!output.stdout.contains(&0x1b), "escape sequences reached stdout");
    assert!(output.stdout.is_empty(), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "claude-rs needs an interactive terminal (stdin and stdout must be a TTY)\n"
    );
    if cfg!(unix) {
        let mut pending = vec![home.path().to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir") {
                let path = entry.expect("entry").path();
                assert!(
                    path.extension().is_none_or(|extension| extension != "log"),
                    "launch wrote a log: {}",
                    path.display()
                );
                if path.is_dir() {
                    pending.push(path);
                }
            }
        }
    }
}

#[test]
fn executable_generates_manuals_without_a_bridge() {
    let dir = tempfile::tempdir().expect("manual directory");
    let output_dir = dir.path().join("nested/man1");
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .args(["--bridge-script", "missing-bridge.js", "man"])
        .arg(&output_dir)
        .output()
        .expect("run man generator");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let manual = std::fs::read_to_string(output_dir.join("claude-rs.1")).expect("root manual");
    for flag in ["model", "effort", "permission", "continue", "resume"] {
        assert!(manual.contains(flag), "missing {flag}");
    }
    assert!(output_dir.join("claude-rs-config-show.1").is_file());
    assert!(output_dir.join("claude-rs-completions.1").is_file());
    assert!(!output_dir.join("claude-rs-man.1").exists());
}
