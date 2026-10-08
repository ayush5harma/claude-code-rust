// SPDX-License-Identifier: Apache-2.0

use super::status::{AgentStatus, parse_agents_json};
use super::{PreferencesCache, executable_from, left_arrow_opens_agents};
use std::ffi::OsString;

mod jobs_format;

#[cfg(unix)]
mod cli {
    use super::super::{CliFallback, run_agents_json};
    use super::AgentStatus;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// A stand-in `claude` that records each argv and answers `agents --json`.
    fn fake_claude(dir: &Path, json_reply: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("claude");
        let argv = dir.join("argv");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$*\" >> '{}'\n{json_reply}\n", argv.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn cwd(dir: &Path) -> String {
        dir.to_string_lossy().into_owned()
    }

    fn runs(dir: &Path) -> usize {
        std::fs::read_to_string(dir.join("argv")).unwrap_or_default().lines().count()
    }

    #[tokio::test]
    async fn the_cli_fallback_runs_agents_json_and_counts_its_reply() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_claude(
            dir.path(),
            r#"printf '%s' '[{"kind":"background","state":"blocked"},{"kind":"interactive"}]'"#,
        );

        let status = run_agents_json(program, cwd(dir.path()), Duration::from_secs(5)).await;

        assert_eq!(status, Some(AgentStatus { background: 1, awaiting_input: 1, working: 0 }));
        assert_eq!(std::fs::read_to_string(dir.path().join("argv")).unwrap(), "agents --json\n");
    }

    #[tokio::test]
    async fn a_failing_cli_hides_the_status() {
        let dir = tempfile::tempdir().unwrap();
        let failing = fake_claude(dir.path(), "printf '[]'; exit 3");
        assert_eq!(run_agents_json(failing, cwd(dir.path()), Duration::from_secs(5)).await, None);
    }

    #[tokio::test]
    async fn the_cli_fallback_runs_at_most_once_a_minute() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_claude(dir.path(), r#"printf '%s' '[{"kind":"background"}]'"#);
        let mut fallback = CliFallback {
            program: Some(program),
            cwd: cwd(dir.path()),
            last_run: None,
            last: None,
        };

        let first = fallback.status(false).await;
        let second = fallback.status(false).await;

        let listed = Some(AgentStatus { background: 1, awaiting_input: 0, working: 0 });
        assert_eq!((first, second), (listed, listed));
        assert_eq!(runs(dir.path()), 1);
    }

    #[tokio::test]
    async fn an_explicit_refresh_reads_changed_jobs_without_waiting_for_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let listing = dir.path().join("listing.json");
        std::fs::write(&listing, "[]").unwrap();
        let program = fake_claude(dir.path(), &format!("cat '{}'", listing.display()));
        let mut fallback = CliFallback {
            program: Some(program),
            cwd: cwd(dir.path()),
            last_run: None,
            last: None,
        };
        assert_eq!(fallback.status(false).await, Some(AgentStatus::default()));
        std::fs::write(&listing, r#"[{"kind":"background","state":"blocked"}]"#).unwrap();
        assert_eq!(fallback.status(false).await, Some(AgentStatus::default()));
        assert_eq!(runs(dir.path()), 1);

        let changed = Some(AgentStatus { background: 1, awaiting_input: 1, working: 0 });
        assert_eq!(fallback.status(true).await, changed);
        assert_eq!(fallback.status(false).await, changed);
        assert_eq!(runs(dir.path()), 2);
    }

    /// `ps` state of `pid`, empty once it is gone.
    fn process_state(pid: &str) -> String {
        let output = std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output();
        String::from_utf8_lossy(&output.unwrap().stdout).trim().to_owned()
    }

    #[tokio::test]
    async fn a_stuck_cli_is_abandoned_and_its_child_killed_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let program =
            fake_claude(dir.path(), &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()));
        let started = Instant::now();

        let status = run_agents_json(program, cwd(dir.path()), Duration::from_secs(2)).await;

        assert_eq!(status, None);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_owned();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // Killed: gone, or a zombie until the runtime reaps it.
            let state = process_state(&pid);
            if state.is_empty() || state.starts_with('Z') {
                break;
            }
            assert!(Instant::now() < deadline, "poll child {pid} still running: {state}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

// Shaped like a real `claude agents --json` from Claude Code 2.1.293: two
// background jobs and interactive sessions with pid/status but no state.
const LISTING: &str = r#"[
  {"id":"9167ebaf","cwd":"/w","kind":"background","sessionId":"s1","name":"Sprint","state":"blocked"},
  {"id":"d7a20a16","cwd":"/w","kind":"background","sessionId":"s2","name":"flake","state":"stopped"},
  {"id":"0c1d2e3f","cwd":"/w","kind":"background","sessionId":"s3","state":"working","status":"busy"},
  {"pid":3349,"cwd":"/w","kind":"interactive","sessionId":"s4","name":"main","status":"busy"},
  {"pid":88677,"cwd":"/w","kind":"interactive","sessionId":"s5","status":"idle","state":"blocked"}
]"#;

#[test]
fn counts_only_background_sessions_by_state() {
    let status = parse_agents_json(LISTING.as_bytes()).unwrap();

    assert_eq!(status, AgentStatus { background: 3, awaiting_input: 1, working: 1 });
}

#[test]
fn an_empty_listing_has_no_agents() {
    assert_eq!(parse_agents_json(b"[]").unwrap(), AgentStatus::default());
}

#[test]
fn malformed_output_is_an_error_not_zero_agents() {
    assert!(parse_agents_json(b"").is_err());
    assert!(parse_agents_json(b"{\"kind\":\"background\"}").is_err());
    assert!(parse_agents_json(b"[{\"kind\":\"background\"").is_err());
    assert!(parse_agents_json(b"Usage: claude agents [options]").is_err());
}

#[test]
fn background_entries_without_a_known_state_still_count_as_agents() {
    let status =
        parse_agents_json(br#"[{"kind":"background"},{"kind":"background","state":"done"}]"#)
            .unwrap();

    assert_eq!(status, AgentStatus { background: 2, awaiting_input: 0, working: 0 });
}

#[test]
fn left_arrow_setting_defaults_on_and_only_false_turns_it_off() {
    let cases = [
        ("{}", true),
        (r#"{"leftArrowOpensAgents":true}"#, true),
        (r#"{"leftArrowOpensAgents":false}"#, false),
        ("not json", true),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude.json");
    assert!(left_arrow_opens_agents(&path), "a missing file keeps the default");
    for (contents, expected) in cases {
        std::fs::write(&path, contents).unwrap();
        assert_eq!(left_arrow_opens_agents(&path), expected, "{contents:?}");
    }
}

/// Rewrites `path` with `contents` of the same length and the old mtime, so
/// only a cache keyed on (mtime, size) can tell that nothing changed.
pub(super) fn rewrite_keeping_stamp(path: &std::path::Path, contents: &str) {
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(std::fs::metadata(path).unwrap().len(), contents.len() as u64);
    std::fs::write(path, contents).unwrap();
    std::fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
}

#[test]
fn the_setting_is_reparsed_only_when_the_config_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude.json");
    std::fs::write(&path, r#"{"leftArrowOpensAgents":false}"#).unwrap();
    let mut cache = PreferencesCache::new(Some(path.clone()));
    assert!(!cache.enabled());

    // Same size and mtime: the cached value stands, the file is not parsed.
    rewrite_keeping_stamp(&path, r#"{"leftArrowOpensAgents":true }"#);
    assert!(!cache.enabled());

    std::fs::write(&path, r#"{"leftArrowOpensAgents":true}"#).unwrap();
    assert!(cache.enabled());
    std::fs::remove_file(&path).unwrap();
    assert!(cache.enabled());
}

#[test]
fn executable_prefers_the_session_executable_and_rejects_a_missing_one() {
    let dir = tempfile::tempdir().unwrap();
    let stock = dir.path().join("claude-stock");
    std::fs::write(&stock, "").unwrap();

    let on_path = dir.path().join("claude-on-path");
    let path_lookup = || Some(on_path.clone());

    assert_eq!(executable_from(Some(OsString::from(&stock)), path_lookup), Some(stock));
    assert_eq!(
        executable_from(Some(OsString::from(dir.path().join("missing"))), path_lookup),
        None
    );
    assert_eq!(executable_from(Some(OsString::new()), path_lookup), Some(on_path.clone()));
    assert_eq!(executable_from(None, path_lookup), Some(on_path));
    assert_eq!(executable_from(None, || None), None);
}
