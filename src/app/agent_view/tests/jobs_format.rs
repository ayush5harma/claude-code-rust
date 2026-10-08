// SPDX-License-Identifier: Apache-2.0

//! Pins the Claude Code files the agent status reads, and the counts stock's
//! `claude agents --json` (without `--all`) would give for them.

use super::super::jobs::{JobsScanner, Scan};
use super::AgentStatus;
use std::collections::HashSet;
use std::path::Path;

const NOW_MS: i64 = 1_791_500_000_000;
/// Long before `NOW_MS`, so the 5 s new-job grace never applies.
const OLD: &str = "2026-08-18T18:30:03.865Z";
const LIVE_PID: u32 = 4242;
const DEAD_PID: u32 = 4343;

fn alive(pid: u32) -> bool {
    [LIVE_PID].contains(&pid)
}

fn write_job(config: &Path, id: &str, state_json: &str) {
    let dir = config.join("jobs").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("state.json"), state_json).unwrap();
}

fn job(state: &str, tempo: &str) -> String {
    format!(
        r#"{{"state":"{state}","tempo":"{tempo}","name":"n","sessionId":"s","createdAt":"{OLD}","updatedAt":"{OLD}","template":"bg","respawnFlags":["--model","opus"]}}"#
    )
}

fn write_roster(config: &Path, workers: &[(&str, u32)]) {
    let workers: Vec<String> =
        workers.iter().map(|(short, pid)| format!(r#""{short}":{{"pid":{pid}}}"#)).collect();
    std::fs::create_dir_all(config.join("daemon")).unwrap();
    std::fs::write(
        config.join("daemon/roster.json"),
        format!(r#"{{"proto":1,"workers":{{{}}}}}"#, workers.join(",")),
    )
    .unwrap();
}

fn write_session(config: &Path, pid: u32, body: &str) {
    std::fs::create_dir_all(config.join("sessions")).unwrap();
    std::fs::write(config.join(format!("sessions/{pid}.json")), body).unwrap();
}

fn scan(scanner: &mut JobsScanner) -> Scan {
    scanner.scan(NOW_MS, &alive)
}

fn counted(background: usize, awaiting_input: usize, working: usize) -> Scan {
    Scan::Counted(AgentStatus { background, awaiting_input, working })
}

#[test]
fn each_job_state_counts_as_stock_lists_it() {
    let config = tempfile::tempdir().unwrap();
    let c = config.path();
    // Alive through the daemon roster: reported as stored.
    write_job(c, "aaaa0001", &job("working", "active"));
    write_job(c, "aaaa0002", &job("blocked", "blocked"));
    write_roster(c, &[("aaaa0001", LIVE_PID), ("aaaa0002", LIVE_PID), ("gone0003", DEAD_PID)]);
    // Ended jobs are not listed without --all.
    write_job(c, "bbbb0001", &job("done", "idle"));
    write_job(c, "bbbb0002", &job("failed", "idle"));
    write_job(c, "bbbb0003", &job("stopped", "idle"));
    // Not alive and older than 5 s: blocked stays blocked, the rest fail.
    write_job(c, "cccc0001", &job("blocked", "blocked"));
    write_job(c, "cccc0002", &job("working", "active"));
    write_job(c, "gone0003", &job("working", "active"));
    // Not part of the listing.
    std::fs::write(c.join("jobs/pins.json"), "[]").unwrap();
    std::fs::create_dir_all(c.join("jobs/no-state-yet")).unwrap();

    assert_eq!(scan(&mut JobsScanner::new(c.to_path_buf())), counted(3, 2, 1));
}

#[test]
fn live_sessions_override_and_add_to_the_job_files() {
    let config = tempfile::tempdir().unwrap();
    let c = config.path();
    write_job(c, "dddd0001", &job("done", "idle"));
    write_job(c, "dddd0002", &job("blocked", "blocked"));
    write_job(c, "dddd0003", &job("working", "active"));
    write_session(c, LIVE_PID, r#"{"pid":4242,"kind":"bg","jobId":"dddd0001","status":"idle"}"#);
    write_session(c, 4244, r#"{"pid":4244,"kind":"bg","jobId":"dddd0002","status":"busy"}"#);
    write_session(c, 4245, r#"{"pid":4245,"kind":"bg","jobId":"dddd0003","status":"waiting"}"#);
    // A live bg session without a job is listed; dead and interactive ones are not.
    write_session(c, 4246, r#"{"pid":4246,"kind":"bg","status":"busy"}"#);
    write_session(c, DEAD_PID, r#"{"pid":4343,"kind":"bg","status":"busy"}"#);
    write_session(c, 4247, r#"{"pid":4247,"kind":"interactive","status":"busy"}"#);
    let alive_pids: HashSet<u32> = [LIVE_PID, 4244, 4245, 4246, 4247].into();

    let scan = JobsScanner::new(c.to_path_buf()).scan(NOW_MS, &|pid| alive_pids.contains(&pid));

    // done with a live session: listed, neither working nor awaiting input;
    // blocked but busy: working; working but waiting: awaiting input.
    assert_eq!(scan, counted(4, 1, 1));
}

#[test]
fn a_recurring_done_job_and_a_new_job_stay_listed() {
    let config = tempfile::tempdir().unwrap();
    let c = config.path();
    write_job(
        c,
        "eeee0001",
        &format!(r#"{{"state":"done","tempo":"idle","createdAt":"{OLD}","routine":null}}"#),
    );
    let just_now = chrono::DateTime::from_timestamp_millis(NOW_MS - 1000).unwrap().to_rfc3339();
    write_job(
        c,
        "eeee0002",
        &format!(r#"{{"state":"starting","tempo":"active","createdAt":"{just_now}"}}"#),
    );

    assert_eq!(scan(&mut JobsScanner::new(c.to_path_buf())), counted(2, 0, 2));
}

#[test]
fn an_unknown_layout_asks_for_the_cli_fallback() {
    for garbage in ["{not json", r#"{"tempo":"idle"}"#, r#"{"state":7}"#, "[]"] {
        let config = tempfile::tempdir().unwrap();
        write_job(config.path(), "ffff0001", garbage);

        let scan = scan(&mut JobsScanner::new(config.path().to_path_buf()));

        assert!(matches!(scan, Scan::Unrecognised(_)), "{garbage}: {scan:?}");
    }
}

#[test]
fn no_jobs_directory_means_no_agents() {
    let config = tempfile::tempdir().unwrap();
    assert_eq!(scan(&mut JobsScanner::new(config.path().to_path_buf())), counted(0, 0, 0));
}

#[test]
fn files_are_reread_only_when_they_change_and_vanished_jobs_drop_out() {
    let config = tempfile::tempdir().unwrap();
    let c = config.path();
    let blocked = job("blocked", "blocked");
    write_job(c, "gggg0001", &blocked);
    write_job(c, "gggg0002", &blocked);
    let mut scanner = JobsScanner::new(c.to_path_buf());
    assert_eq!(scan(&mut scanner), counted(2, 2, 0));

    // Unchanged size and mtime: the cached parse stands.
    let state = c.join("jobs/gggg0001/state.json");
    super::rewrite_keeping_stamp(&state, &job("stopped", "idle___"));
    assert_eq!(scan(&mut scanner), counted(2, 2, 0));

    std::fs::write(&state, job("stopped", "idle")).unwrap();
    assert_eq!(scan(&mut scanner), counted(1, 1, 0));

    std::fs::remove_dir_all(c.join("jobs/gggg0002")).unwrap();
    assert_eq!(scan(&mut scanner), counted(0, 0, 0));
}

/// Real-machine parity and cost check, run by hand:
/// `AGENT_PARITY_CONFIG_DIR=~/.claude-personal AGENT_PARITY_CLAUDE=<stock claude>
///  cargo test --lib agent_view_files_match_agents_json -- --ignored --nocapture`.
/// Compares the file reading with the stock listing and prints the CPU of
/// 100 cached polls.
#[test]
#[ignore = "reads the developer's real Claude Code profile"]
#[cfg(unix)]
fn agent_view_files_match_agents_json() {
    let config = std::path::PathBuf::from(std::env::var_os("AGENT_PARITY_CONFIG_DIR").unwrap());
    let claude = std::env::var_os("AGENT_PARITY_CLAUDE").unwrap();
    let output = std::process::Command::new(&claude)
        .args(["agents", "--json"])
        .env("CLAUDE_CONFIG_DIR", &config)
        .output()
        .unwrap();
    let stock = crate::app::agent_view::status::parse_agents_json(&output.stdout).unwrap();
    let mut scanner = JobsScanner::new(config.clone());
    let now = chrono::Utc::now().timestamp_millis();
    let files = scanner.scan(now, &super::super::pid_alive);
    println!("config {}: stock {stock:?}, files {files:?}", config.display());
    assert_eq!(files, Scan::Counted(stock));

    let cpu = || {
        // SAFETY: getrusage fills the zeroed struct it is given.
        #[allow(unsafe_code)]
        unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            libc::getrusage(libc::RUSAGE_SELF, &raw mut usage);
            let micros = |time: libc::timeval| time.tv_sec * 1_000_000 + i64::from(time.tv_usec);
            micros(usage.ru_utime) + micros(usage.ru_stime)
        }
    };
    let (started, cpu_before) = (std::time::Instant::now(), cpu());
    for _ in 0..100 {
        let _ = scanner.scan(now, &super::super::pid_alive);
    }
    println!("100 cached polls: {:?} wall, {} us CPU", started.elapsed(), cpu() - cpu_before);
}
