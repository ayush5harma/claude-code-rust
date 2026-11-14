// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use claude_code_rust::{Cli, StartupLaunch};
#[cfg(unix)]
use std::process::Command;

#[path = "support/startup_audit.rs"]
mod audit_tests;

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
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("docs")).unwrap();
    let options = [
        "--model",
        "opus",
        "--effort",
        "max",
        "--permission-mode",
        "plan",
        "--agent",
        "reviewer",
        "--add-dir",
        "docs",
        "--name",
        "Experiment",
    ];
    for before in [true, false] {
        let mut args = vec!["claude-rs", "--dir", project.path().to_str().unwrap()];
        if before {
            args.extend(options);
        }
        args.extend(["resume", "session-1", "review prompt"]);
        if !before {
            args.extend(options);
        }
        let cli = Cli::parse_from_args(args).unwrap();
        cli.validate().unwrap();
        assert_eq!(
            cli.startup_launch(),
            StartupLaunch::ResumeSession { session_id: "session-1".into() }
        );
        assert_eq!(cli.initial_prompt(), Some("review prompt"));
        assert_eq!(cli.session_options.model.as_deref(), Some("opus"));
        assert_eq!(
            cli.session_options.effort,
            Some(claude_code_rust::agent::model::EffortLevel::Max)
        );
        assert_eq!(cli.session_options.permission_mode.unwrap().as_stored(), "plan");
        assert_eq!(cli.session_options.agent.as_deref(), Some("reviewer"));
        assert_eq!(cli.session_options.name.as_deref(), Some("Experiment"));
        assert_eq!(
            cli.resolve_session_config().unwrap().additional_directories,
            [project.path().join("docs").to_str().unwrap()]
        );
    }
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
fn executable_rejects_a_missing_added_directory_before_starting_the_bridge() {
    let project = tempfile::tempdir().expect("project");
    let script = project.path().join("inert-bridge.js");
    std::fs::write(&script, b"throw new Error('must never run');").expect("script");
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .arg("--bridge-script")
        .arg(&script)
        .args(["--add-dir", "not-there", "-C"])
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
    assert!(!error.contains("must never run"), "the bridge started: {error}");
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
            format!("'--dir <DIR>': directory does not exist: {}", missing.display()),
        ),
        (
            &["-C".as_ref(), file.as_os_str()],
            format!("'--dir <DIR>': not a directory: {}", file.display()),
        ),
        (
            &["--bridge-script".as_ref(), missing.as_os_str()],
            format!(
                "'--bridge-script <BRIDGE_SCRIPT>': bridge script does not exist: {}",
                missing.display()
            ),
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
fn help_into_a_closed_pipe_exits_cleanly() {
    let (reader, writer) = std::io::pipe().expect("pipe");
    // Closed before the child starts, as `claude-rs --help | head -1` can leave it.
    drop(reader);
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .arg("--help")
        .stdout(writer)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run help");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

/// `echo x | claude-rs hi` from a terminal: crossterm reads keys from `/dev/tty`, so piped
/// stdin is not a reason to refuse. The launch is made to stop right after the terminal
/// check, at a missing bridge script, so it never needs a bridge.
#[cfg(unix)]
#[test]
fn piped_stdin_with_a_terminal_on_stdout_passes_the_terminal_check() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read as _;

    let home = tempfile::tempdir().expect("home");
    let project = home.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let missing_bridge = home.path().join("missing-bridge.js");
    let pair = native_pty_system()
        .openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 })
        .expect("open pty");
    let mut command = CommandBuilder::new("/bin/sh");
    // The pty is the shell's controlling terminal and stdout; stdin becomes a pipe.
    command.args([
        "-c",
        "echo x | exec \"$0\" --no-update-check hi",
        env!("CARGO_BIN_EXE_claude-rs"),
    ]);
    command.cwd(&project);
    command.env("HOME", home.path());
    command.env("CLAUDE_CONFIG_DIR", home.path().join("profile"));
    command.env("CLAUDE_RS_AGENT_BRIDGE", &missing_bridge);
    command.env("CLAUDE_ARCHIVE_SKIP", "1");
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let mut child = pair.slave.spawn_command(command).expect("spawn claude-rs");
    drop(pair.slave);
    let output = std::thread::spawn(move || {
        let mut output = Vec::new();
        // macOS reports EIO rather than EOF once the last writer has closed.
        let _ = reader.read_to_end(&mut output);
        String::from_utf8_lossy(&output).into_owned()
    });
    let status = child.wait().expect("wait for claude-rs");
    drop(pair.master);
    let output = output.join().expect("pty output");

    assert!(!output.contains("needs an interactive terminal"), "{output}");
    assert_eq!(status.exit_code(), 21, "{output}");
    // The error report shortens the home directory, which is this test's HOME.
    assert!(output.contains("~/missing-bridge.js"), "{output}");
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

/// Runs the real binary with a scratch profile and returns it with its output.
#[cfg(unix)]
#[allow(clippy::expect_used)]
fn run_with_scratch_profile(
    args: &[&std::ffi::OsStr],
    cwd: &std::path::Path,
) -> (tempfile::TempDir, std::process::Output) {
    let profile = tempfile::tempdir().expect("profile directory");
    let output = Command::new(env!("CARGO_BIN_EXE_claude-rs"))
        .args(args)
        .current_dir(cwd)
        .env("CLAUDE_CONFIG_DIR", profile.path())
        .output()
        .expect("run claude-rs");
    (profile, output)
}

#[test]
fn launch_paths_that_exist_are_accepted_for_every_interactive_launch() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let script = scratch.path().join("bridge.js");
    std::fs::write(&script, "").expect("script");
    let paths: [&std::ffi::OsStr; 4] =
        ["-C".as_ref(), scratch.path().as_os_str(), "--bridge-script".as_ref(), script.as_os_str()];
    for selection in [&[][..], &["--resume"][..], &["--continue"][..], &["resume", "session-1"][..]]
    {
        let mut args: Vec<&std::ffi::OsStr> = vec!["claude-rs".as_ref()];
        if selection.first() == Some(&"resume") {
            args.extend(paths);
            args.extend(selection.iter().map(std::ffi::OsStr::new));
        } else {
            args.extend(selection.iter().map(std::ffi::OsStr::new));
            args.extend(paths);
        }
        let cli = Cli::try_parse_from(&args).expect("parse");
        cli.validate().unwrap_or_else(|error| panic!("{args:?}: {error}"));
    }
}
/// Sets `path`'s mode for the length of a test and opens it again on drop, so a failed
/// assertion still leaves the scratch directory removable.
#[cfg(unix)]
struct ModeGuard(std::path::PathBuf);

#[cfg(unix)]
impl ModeGuard {
    #[allow(clippy::expect_used)]
    fn set(path: &std::path::Path, mode: u32) -> Self {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        Self(path.to_path_buf())
    }
}

#[cfg(unix)]
impl Drop for ModeGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// Root passes every permission check, which leaves nothing to refuse: such a run skips
/// explicitly rather than passing without asserting.
/// The probe opens `behind_lock`, a path the test locked away, rather than repeating the
/// check under test, so a broken check fails instead of being skipped.
#[cfg(unix)]
fn permissions_enforced(behind_lock: &std::path::Path) -> bool {
    let enforced = std::fs::File::open(behind_lock)
        .is_err_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied);
    if !enforced {
        eprintln!("skipped: permissions are not enforced for this user (root?)");
    }
    enforced
}

#[cfg(unix)]
#[test]
fn a_dir_needs_both_read_and_search_permission() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let make = |name: &str| {
        let dir = scratch.path().join(name);
        std::fs::create_dir(&dir).expect("dir");
        std::fs::write(dir.join("inside.txt"), "").expect("file inside");
        dir
    };
    // Read without search lists the names but cannot enter the directory or open
    // anything in it; search without read enters it but cannot list it.
    let no_search = make("read-only");
    let no_read = make("search-only");
    let both = make("read-and-search");
    let _guards = [
        ModeGuard::set(&no_search, 0o600),
        ModeGuard::set(&no_read, 0o300),
        ModeGuard::set(&both, 0o500),
    ];
    if !permissions_enforced(&no_search.join("inside.txt")) {
        return;
    }
    for (dir, reason) in [(&no_search, "cannot be entered"), (&no_read, "cannot be read")] {
        let (profile, output) =
            run_with_scratch_profile(&["--dir".as_ref(), dir.as_os_str()], scratch.path());
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{}: {error}", dir.display());
        assert!(error.contains("--dir") && error.contains(reason), "{error}");
        assert!(error.contains(&dir.display().to_string()), "{error}");
        assert_eq!(std::fs::read_dir(profile.path()).expect("profile").count(), 0);
    }
    let cli = Cli::try_parse_from(["claude-rs".as_ref(), "-C".as_ref(), both.as_os_str()])
        .expect("parse");
    cli.validate().unwrap_or_else(|error| panic!("read and search: {error}"));
}

#[cfg(unix)]
#[test]
fn a_dir_behind_an_unsearchable_parent_is_reported_as_inaccessible_not_missing() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = scratch.path().join("locked-parent");
    std::fs::create_dir_all(parent.join("project")).expect("project");
    let _guard = ModeGuard::set(&parent, 0o000);
    if !permissions_enforced(&parent.join("project")) {
        return;
    }
    let project = parent.join("project");
    let error = Cli::try_parse_from(["claude-rs".as_ref(), "-C".as_ref(), project.as_os_str()])
        .expect("parse")
        .validate()
        .expect_err("dir behind a locked parent")
        .to_string();
    assert!(error.contains("cannot be accessed"), "{error}");
    assert!(!error.contains("does not exist"), "{error}");
}

/// Run under a pseudo-terminal: a launch without one stops at the terminal check (exit 25)
/// before logging starts, so the logging failure is only reachable from a terminal.
#[cfg(unix)]
#[test]
fn explicit_logging_failure_exits_one_before_bridge_startup_and_preserves_input() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read as _;

    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("inert bridge.js");
    std::fs::write(&script, b"throw new Error('must never run');").unwrap();
    let blocker = root.path().join("blocker");
    std::fs::write(&blocker, b"keep blocker").unwrap();
    let target = blocker.join("explicit.log");
    let pair = native_pty_system()
        .openpty(PtySize { rows: 24, cols: 200, pixel_width: 0, pixel_height: 0 })
        .expect("open pty");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_claude-rs"));
    command.arg("--bridge-script");
    command.arg(&script);
    command.arg("--log-file");
    command.arg(&target);
    command.arg("--no-update-check");
    command.cwd(root.path());
    command.env_remove("RUST_LOG");
    command.env("CLAUDE_CONFIG_DIR", root.path().join("profile"));
    command.env("CLAUDE_RS_CONFIG_DIR", root.path().join("app-profile"));
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let mut child = pair.slave.spawn_command(command).expect("spawn claude-rs");
    drop(pair.slave);
    let output = std::thread::spawn(move || {
        let mut output = Vec::new();
        // macOS reports EIO rather than EOF once the last writer has closed.
        let _ = reader.read_to_end(&mut output);
        String::from_utf8_lossy(&output).into_owned()
    });
    let status = child.wait().expect("wait for claude-rs");
    drop(pair.master);
    let error = output.join().expect("pty output");

    assert_eq!(status.exit_code(), 1, "{error}");
    assert!(error.contains("failed to create log directory"), "{error}");
    assert!(error.contains(blocker.to_str().unwrap()), "{error}");
    assert!(!error.contains("claude-rs resume"), "{error}");
    assert_eq!(std::fs::read(blocker).unwrap(), b"keep blocker");
    assert_eq!(std::fs::read(script).unwrap(), b"throw new Error('must never run');");
    assert!(!root.path().join("app-profile").exists());
    assert!(!root.path().join("profile").exists());
}

// Default diagnostics lookup honors XDG_DATA_HOME on Linux. Other platforms must
// use an independently verified native data-directory isolation mechanism.
#[cfg(all(target_os = "linux", debug_assertions))]
#[tokio::test]
async fn wrapped_startup_errors_keep_their_typed_exit_and_report_without_starting_a_session() {
    for (missing_runtime, code, category) in
        [(true, 20, "environment"), (false, 21, "bridge_spawn")]
    {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let script = root.path().join("inert bridge.js");
        std::fs::write(&script, b"throw new Error('must never run');").unwrap();
        let missing =
            root.path().join(if missing_runtime { "missing-runtime" } else { "missing-script.js" });
        let mut probe = tokio::process::Command::new(env!("CARGO_BIN_EXE_claude-rs"));
        probe
            .args(["logs", "--path"])
            .current_dir(&project)
            .env("XDG_DATA_HOME", &data)
            .env("HOME", root.path().join("home"))
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let selected = tokio::time::timeout(std::time::Duration::from_secs(20), probe.output())
            .await
            .unwrap()
            .unwrap();
        assert!(selected.status.success());
        assert_eq!(
            String::from_utf8(selected.stdout).unwrap(),
            format!("{}\n", data.join("claude-code-rust/logs/runtime").display())
        );
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_claude-rs"));
        command
            .current_dir(&project)
            .args(["--no-update-check", "--log-file"])
            .arg(root.path().join("explicit.log"))
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("HOME", root.path().join("home"))
            .env("CLAUDE_CONFIG_DIR", root.path().join("profile"))
            .env("CLAUDE_RS_CONFIG_DIR", root.path().join("app-profile"))
            .env_remove("RUST_LOG")
            .env_remove("CLAUDE_RS_AGENT_BRIDGE")
            .env("CLAUDE_RS_AGENT_BRIDGE_RUNTIME", &missing)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        if missing_runtime {
            command.arg("--bridge-script").arg(&script);
        } else {
            command.env("CLAUDE_RS_AGENT_BRIDGE", &missing);
        }
        let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(output.stdout.is_empty());
        let text = String::from_utf8(output.stderr).unwrap();
        let field = |name: &str| {
            text.lines().find_map(|line| line.trim().strip_prefix(&format!("{name}: ")))
        };
        assert_eq!(field("category"), Some(category));
        assert_eq!(field("exit_code"), Some(code.to_string().as_str()));
        assert_eq!(field("next_step"), Some("claude-rs doctor --strict"));
        assert!(field("detail").unwrap().contains(missing.to_str().unwrap()));
        assert!(!text.contains("claude-rs resume"));
        assert!(!root.path().join("app-profile").exists());
        assert!(!root.path().join("profile").exists());
        assert_eq!(std::fs::read(script).unwrap(), b"throw new Error('must never run');");
    }
}
