// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
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
        let cli = Cli::try_parse_from(&args).expect("parse startup");
        cli.validate().expect("valid launch");
        assert_eq!(cli.startup_launch(), launch, "{args:?}");
        assert_eq!(cli.initial_prompt(), prompt, "{args:?}");
    }
}

#[test]
fn session_options_support_existing_resume_syntax() {
    let cli = Cli::try_parse_from([
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
    ] {
        assert!(
            Cli::try_parse_from(&args).and_then(|cli| cli.validate()).is_err(),
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
fn executable_refuses_a_bad_dir_or_bridge_script_before_startup() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let file = scratch.path().join("notes.txt");
    std::fs::write(&file, "").expect("file");
    let missing_dir = scratch.path().join("missing-dir");
    let missing_script = scratch.path().join("missing-bridge.js");
    let scratch_dir = scratch.path().to_path_buf();
    let cases = [
        ("--dir", &missing_dir, "directory does not exist"),
        ("--dir", &file, "not a directory"),
        ("--bridge-script", &missing_script, "bridge script does not exist"),
        ("--bridge-script", &scratch_dir, "bridge script is not a file"),
    ];
    for (flag, path, reason) in cases {
        let (profile, output) =
            run_with_scratch_profile(&[flag.as_ref(), path.as_os_str()], scratch.path());
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{flag} {}: {error}", path.display());
        assert!(output.stdout.is_empty(), "{flag}: {}", String::from_utf8_lossy(&output.stdout));
        assert!(error.contains(flag), "{error}");
        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains(reason), "{error}");
        // The trust prompt and settings are never reached.
        let saved = std::fs::read_dir(profile.path()).expect("profile").count();
        assert_eq!(saved, 0, "{flag} left files in the profile");
    }
}

#[test]
fn resume_with_a_bad_path_is_refused_like_a_plain_launch() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let missing_dir = scratch.path().join("missing-dir");
    let missing_script = scratch.path().join("missing-bridge.js");
    for (flag, path) in [("--dir", &missing_dir), ("--bridge-script", &missing_script)] {
        let (profile, output) = run_with_scratch_profile(
            &[flag.as_ref(), path.as_os_str(), "resume".as_ref(), "session-1".as_ref()],
            scratch.path(),
        );
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{flag}: {error}");
        assert!(error.contains(flag) && error.contains(&path.display().to_string()), "{error}");
        assert_eq!(std::fs::read_dir(profile.path()).expect("profile").count(), 0, "{flag}");
    }
}

#[test]
fn executable_resolves_a_relative_dir_against_the_current_directory() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    std::fs::create_dir(scratch.path().join("project")).expect("project");
    // `project` exists next to the working directory, so only the script is wrong.
    let (_profile, output) = run_with_scratch_profile(
        &["-C".as_ref(), "project".as_ref(), "--bridge-script".as_ref(), "missing.js".as_ref()],
        scratch.path(),
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{error}");
    assert!(error.contains("--bridge-script"), "{error}");
    assert!(!error.contains("--dir"), "{error}");

    let (_profile, output) =
        run_with_scratch_profile(&["-C".as_ref(), "elsewhere".as_ref()], scratch.path());
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{error}");
    assert!(error.contains("--dir"), "{error}");
}

#[test]
fn doctor_still_reports_a_bad_bridge_script_instead_of_refusing_it() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let missing = scratch.path().join("missing-bridge.js");
    let (_profile, output) = run_with_scratch_profile(
        &["--bridge-script".as_ref(), missing.as_os_str(), "doctor".as_ref(), "--json".as_ref()],
        scratch.path(),
    );
    assert_ne!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("bridge script does not exist"), "{report}");
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

#[cfg(unix)]
#[test]
fn unreadable_dir_and_bridge_script_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().expect("scratch directory");
    let dir = scratch.path().join("locked-dir");
    let script = scratch.path().join("locked.js");
    std::fs::create_dir(&dir).expect("dir");
    std::fs::write(&script, "").expect("script");
    let locked = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(&dir, locked.clone()).expect("lock dir");
    std::fs::set_permissions(&script, locked).expect("lock script");
    let open = std::fs::Permissions::from_mode(0o755);
    // Root reads through mode 000, which leaves nothing to refuse: skip explicitly rather
    // than pass without asserting.
    if std::fs::read_dir(&dir).is_ok() || std::fs::File::open(&script).is_ok() {
        std::fs::set_permissions(&dir, open.clone()).expect("unlock dir");
        std::fs::set_permissions(&script, open).expect("unlock script");
        eprintln!("skipped: permissions are not enforced for this user (root?)");
        return;
    }
    let results = [
        Cli::try_parse_from(["claude-rs".as_ref(), "-C".as_ref(), dir.as_os_str()])
            .expect("parse")
            .validate(),
        Cli::try_parse_from(["claude-rs".as_ref(), "--bridge-script".as_ref(), script.as_os_str()])
            .expect("parse")
            .validate(),
    ];
    // Let the scratch directory be removed.
    std::fs::set_permissions(&dir, open.clone()).expect("unlock dir");
    std::fs::set_permissions(&script, open).expect("unlock script");
    let [dir_result, script_result] = results;
    let dir_error = dir_result.expect_err("unreadable dir").to_string();
    let script_error = script_result.expect_err("unreadable script").to_string();
    assert!(dir_error.contains("--dir") && dir_error.contains("cannot be read"), "{dir_error}");
    assert!(
        script_error.contains("--bridge-script") && script_error.contains("cannot be read"),
        "{script_error}"
    );
}

#[cfg(unix)]
#[test]
fn a_path_behind_an_unsearchable_parent_is_reported_as_inaccessible_not_missing() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = scratch.path().join("locked-parent");
    std::fs::create_dir(&parent).expect("parent");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).expect("lock");
    let open = std::fs::Permissions::from_mode(0o755);
    // Root can search a mode-000 directory, so nothing would be refused.
    let enforced = std::fs::metadata(parent.join("x"))
        .is_err_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied);
    if !enforced {
        std::fs::set_permissions(&parent, open).expect("unlock");
        eprintln!("skipped: permissions are not enforced for this user (root?)");
        return;
    }
    let behind = parent.join("x.js");
    let results = [
        Cli::try_parse_from(["claude-rs".as_ref(), "-C".as_ref(), behind.as_os_str()])
            .expect("parse")
            .validate(),
        Cli::try_parse_from(["claude-rs".as_ref(), "--bridge-script".as_ref(), behind.as_os_str()])
            .expect("parse")
            .validate(),
    ];
    std::fs::set_permissions(&parent, open).expect("unlock");
    let [dir_result, script_result] = results;
    let dir_error = dir_result.expect_err("dir behind locked parent").to_string();
    let script_error = script_result.expect_err("script behind locked parent").to_string();
    assert!(dir_error.contains("cannot be accessed"), "{dir_error}");
    assert!(script_error.contains("cannot be accessed"), "{script_error}");
    assert!(!script_error.contains("does not exist"), "{script_error}");
}
