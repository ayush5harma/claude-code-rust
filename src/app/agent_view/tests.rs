// SPDX-License-Identifier: Apache-2.0

use super::status::{AgentStatus, parse_agents_json};
use super::{executable_from, left_arrow_opens_agents};
use std::ffi::OsString;

#[cfg(unix)]
mod poll {
    use super::super::poll_status;
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

    #[tokio::test]
    async fn a_poll_runs_agents_json_and_counts_its_reply() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_claude(
            dir.path(),
            r#"printf '%s' '[{"kind":"background","state":"blocked"},{"kind":"interactive"}]'"#,
        );

        let status = poll_status(program, None, cwd(dir.path()), Duration::from_secs(5)).await;

        assert_eq!(status, Some(AgentStatus { background: 1, awaiting_input: 1, working: 0 }));
        assert_eq!(std::fs::read_to_string(dir.path().join("argv")).unwrap(), "agents --json\n");
    }

    #[tokio::test]
    async fn a_failing_or_disabled_poll_hides_the_status() {
        let dir = tempfile::tempdir().unwrap();
        let failing = fake_claude(dir.path(), "printf '[]'; exit 3");
        assert_eq!(poll_status(failing, None, cwd(dir.path()), Duration::from_secs(5)).await, None);

        let off = tempfile::tempdir().unwrap();
        let program = fake_claude(off.path(), "printf '[]'");
        let preferences = off.path().join(".claude.json");
        std::fs::write(&preferences, r#"{"leftArrowOpensAgents":false}"#).unwrap();
        let status =
            poll_status(program, Some(preferences), cwd(off.path()), Duration::from_secs(5)).await;
        assert_eq!(status, None);
        assert!(!off.path().join("argv").exists(), "a disabled agent view must not run claude");
    }

    #[tokio::test]
    async fn a_stuck_poll_is_abandoned_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let program = fake_claude(dir.path(), "exec sleep 30");
        let started = Instant::now();

        let status = poll_status(program, None, cwd(dir.path()), Duration::from_millis(200)).await;

        assert_eq!(status, None);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
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

fn preferences(contents: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude.json");
    if let Some(contents) = contents {
        std::fs::write(&path, contents).unwrap();
    }
    (dir, path)
}

#[test]
fn left_arrow_setting_defaults_on_and_only_false_turns_it_off() {
    let cases = [
        (None, true),
        (Some("{}"), true),
        (Some(r#"{"leftArrowOpensAgents":true}"#), true),
        (Some(r#"{"leftArrowOpensAgents":false}"#), false),
        (Some("not json"), true),
    ];
    for (contents, expected) in cases {
        let (_dir, path) = preferences(contents);
        assert_eq!(left_arrow_opens_agents(Some(&path)), expected, "{contents:?}");
    }
    assert!(left_arrow_opens_agents(None));
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
