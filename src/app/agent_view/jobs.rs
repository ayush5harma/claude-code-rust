// SPDX-License-Identifier: Apache-2.0

//! Background-session counts read straight from Claude Code's files, so a
//! poll costs a few `stat` calls instead of starting the stock binary.
//!
//! This ports what `claude agents --json` (without `--all`) does in Claude
//! Code 2.1.293-2.1.295 (`printAgentsJson` and its job helpers):
//! - jobs are the directories of `<config>/jobs/` with a `state.json`
//!   (`pins.json` is a file, not a job);
//! - a job is alive when the daemon roster (`daemon/roster.json`) lists a
//!   worker for it whose pid is alive, or a live `kind: "bg"` session in
//!   `sessions/<pid>.json` carries its `jobId`;
//! - a job that is neither terminal nor alive, and older than 5 s, is shown
//!   as blocked when its state is "blocked" (and it is not a one-shot exec),
//!   otherwise as failed;
//! - the reported state is working for a busy live session; done, failed or
//!   stopped for a terminal job (state done/failed/stopped with a tempo other
//!   than "active"), except a done recurring job; blocked for a "blocked"
//!   tempo or a waiting live session; working otherwise;
//! - a job is listed when it has a live session or is working or blocked;
//!   a live `bg` session without a job is listed too.
//!
//! Differences: stock asks the daemon over its socket for live workers and
//! falls back to the roster file, and it checks a pid's start time against
//! pid reuse; this reads the roster and checks only that the pid exists.

use super::status::AgentStatus;
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Stock's grace period before an unclaimed new job counts as dead.
const NEW_JOB_GRACE_MS: i64 = 5000;

/// What one scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scan {
    Counted(AgentStatus),
    /// The files are not in the layout this reader knows; the caller falls
    /// back to `claude agents --json`.
    Unrecognised(String),
}

/// A file's identity for the cache: re-read only when it changes.
pub(super) type Stamp = (SystemTime, u64);

struct Cached<T> {
    stamp: Stamp,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    state: String,
    tempo: Option<String>,
    created_at_ms: Option<i64>,
    /// `template: "exec"` with no respawn flags: stock's one-shot exec.
    one_shot_exec: bool,
    /// A routine, a self-waking job, a session cron or a /loop.
    recurring: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredSession {
    pid: u32,
    kind: Option<String>,
    status: Option<String>,
    job_id: Option<String>,
    parked: bool,
}

/// Reads one Claude config directory, caching every parsed file by its
/// modification time and size. Owned by the poller and used off the UI
/// thread.
pub(crate) struct JobsScanner {
    config_dir: PathBuf,
    jobs: HashMap<OsString, Cached<Result<Job, String>>>,
    sessions: HashMap<OsString, Cached<Option<RegisteredSession>>>,
    roster: Option<Cached<HashMap<String, u32>>>,
}

impl JobsScanner {
    pub(crate) fn new(config_dir: PathBuf) -> Self {
        Self { config_dir, jobs: HashMap::new(), sessions: HashMap::new(), roster: None }
    }

    pub(crate) fn scan(&mut self, now_ms: i64, pid_alive: &dyn Fn(u32) -> bool) -> Scan {
        let jobs = match self.read_jobs() {
            Ok(jobs) => jobs,
            Err(reason) => return Scan::Unrecognised(reason),
        };
        let sessions = self.read_sessions();
        let roster = self.read_roster();
        Scan::Counted(count(&jobs, &sessions, &roster, now_ms, pid_alive))
    }

    fn read_jobs(&mut self) -> Result<Vec<(String, Job)>, String> {
        let dir = self.config_dir.join("jobs");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.jobs.clear();
                return Ok(Vec::new());
            }
            Err(error) => return Err(format!("cannot list {}: {error}", dir.display())),
        };
        let mut seen = HashSet::new();
        let mut jobs = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            let path = entry.path().join("state.json");
            // Stock skips a job directory without a readable state.
            let Some(stamp) = stamp_of(&path) else {
                continue;
            };
            seen.insert(name.clone());
            let job = cached(&mut self.jobs, &name, stamp, || parse_job(&path));
            match job {
                Ok(job) => jobs.push((name.to_string_lossy().into_owned(), job.clone())),
                Err(reason) => return Err(reason.clone()),
            }
        }
        self.jobs.retain(|name, _| seen.contains(name));
        Ok(jobs)
    }

    fn read_sessions(&mut self) -> Vec<RegisteredSession> {
        let Ok(entries) = std::fs::read_dir(self.config_dir.join("sessions")) else {
            self.sessions.clear();
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut sessions = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().ends_with(".json") {
                continue;
            }
            let path = entry.path();
            let Some(stamp) = stamp_of(&path) else {
                continue;
            };
            seen.insert(name.clone());
            if let Some(session) = cached(&mut self.sessions, &name, stamp, || parse_session(&path))
            {
                sessions.push(session.clone());
            }
        }
        self.sessions.retain(|name, _| seen.contains(name));
        sessions
    }

    fn read_roster(&mut self) -> HashMap<String, u32> {
        let path = self.config_dir.join("daemon").join("roster.json");
        let Some(stamp) = stamp_of(&path) else {
            self.roster = None;
            return HashMap::new();
        };
        if self.roster.as_ref().is_none_or(|roster| roster.stamp != stamp) {
            self.roster = Some(Cached { stamp, value: parse_roster(&path) });
        }
        self.roster.as_ref().map(|roster| roster.value.clone()).unwrap_or_default()
    }
}

pub(super) fn stamp_of(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn cached<'a, T>(
    cache: &'a mut HashMap<OsString, Cached<T>>,
    name: &OsString,
    stamp: Stamp,
    read: impl FnOnce() -> T,
) -> &'a T {
    match cache.entry(name.clone()) {
        Entry::Occupied(mut entry) => {
            if entry.get().stamp != stamp {
                entry.insert(Cached { stamp, value: read() });
            }
            &entry.into_mut().value
        }
        Entry::Vacant(entry) => &entry.insert(Cached { stamp, value: read() }).value,
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn parse_job(path: &Path) -> Result<Job, String> {
    let document = read_json(path).ok_or_else(|| format!("{} does not parse", path.display()))?;
    let state = document
        .get("state")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} has no state", path.display()))?
        .to_owned();
    let text = |key: &str| document.get(key).and_then(Value::as_str).map(str::to_owned);
    let starts_loop =
        |key: &str| text(key).is_some_and(|value| value.trim().to_lowercase().starts_with("/loop"));
    let respawn_flags_empty =
        document.get("respawnFlags").and_then(Value::as_array).is_none_or(Vec::is_empty);
    let session_cron = document
        .get("inFlight")
        .and_then(|in_flight| in_flight.get("kinds"))
        .and_then(Value::as_array)
        .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("session_cron")));
    Ok(Job {
        tempo: text("tempo"),
        created_at_ms: text("createdAt")
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
            .map(|at| at.timestamp_millis()),
        one_shot_exec: text("template").as_deref() == Some("exec") && respawn_flags_empty,
        // Stock tests `routine !== undefined`, so a JSON null counts.
        recurring: document.get("routine").is_some()
            || document.get("selfWake").and_then(Value::as_bool) == Some(true)
            || session_cron
            || starts_loop("intent")
            || starts_loop("initialPrompt"),
        state,
    })
}

fn parse_session(path: &Path) -> Option<RegisteredSession> {
    let document = read_json(path)?;
    let text = |key: &str| document.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(RegisteredSession {
        pid: u32::try_from(document.get("pid")?.as_u64()?).ok()?,
        kind: text("kind"),
        status: text("status"),
        job_id: text("jobId").filter(|id| !id.is_empty()),
        parked: document.get("parkedJobId").is_some(),
    })
}

fn parse_roster(path: &Path) -> HashMap<String, u32> {
    let Some(document) = read_json(path) else {
        return HashMap::new();
    };
    let Some(workers) = document.get("workers").and_then(Value::as_object) else {
        return HashMap::new();
    };
    workers
        .iter()
        .filter_map(|(short, worker)| {
            let pid = u32::try_from(worker.get("pid")?.as_u64()?).ok()?;
            Some((short.clone(), pid))
        })
        .collect()
}

fn is_terminal(state: &str, tempo: Option<&str>) -> bool {
    matches!(state, "done" | "failed" | "stopped") && tempo != Some("active")
}

fn count(
    jobs: &[(String, Job)],
    sessions: &[RegisteredSession],
    roster: &HashMap<String, u32>,
    now_ms: i64,
    pid_alive: &dyn Fn(u32) -> bool,
) -> AgentStatus {
    let live: Vec<&RegisteredSession> = sessions
        .iter()
        .filter(|session| session.kind.as_deref() == Some("bg") && pid_alive(session.pid))
        .collect();
    let live_by_job: HashMap<&str, &RegisteredSession> = live
        .iter()
        .filter_map(|session| session.job_id.as_deref().map(|id| (id, *session)))
        .collect();
    let alive_workers: HashSet<&str> = roster
        .iter()
        .filter(|(_, pid)| pid_alive(**pid))
        .map(|(short, _)| short.as_str())
        .collect();

    let mut status = AgentStatus::default();
    for (id, job) in jobs {
        let session = live_by_job.get(id.as_str()).copied();
        let (state, tempo) = effective_state(job, id, session, &alive_workers, now_ms);
        let reported = reported_state(job, &state, tempo.as_deref(), session);
        if session.is_none() && !matches!(reported, Reported::Working | Reported::Blocked) {
            continue;
        }
        status.background += 1;
        match reported {
            Reported::Working => status.working += 1,
            Reported::Blocked => status.awaiting_input += 1,
            Reported::Ended => {}
        }
    }
    // A live background session with no job is listed without a state.
    status.background +=
        live.iter().filter(|session| session.job_id.is_none() && !session.parked).count();
    status
}

/// Stock's `Mnt`: a job nobody runs any more is shown as blocked or failed.
fn effective_state(
    job: &Job,
    id: &str,
    session: Option<&RegisteredSession>,
    alive_workers: &HashSet<&str>,
    now_ms: i64,
) -> (String, Option<String>) {
    let unchanged = (job.state.clone(), job.tempo.clone());
    if is_terminal(&job.state, job.tempo.as_deref())
        || session.is_some()
        || alive_workers.contains(id)
        || job.created_at_ms.is_some_and(|created| now_ms - created < NEW_JOB_GRACE_MS)
    {
        return unchanged;
    }
    if job.state == "blocked" && !job.one_shot_exec {
        (job.state.clone(), Some("blocked".to_owned()))
    } else {
        ("failed".to_owned(), Some("idle".to_owned()))
    }
}

enum Reported {
    Working,
    Blocked,
    Ended,
}

/// Stock's per-entry `state` in `claude agents --json`.
fn reported_state(
    job: &Job,
    state: &str,
    tempo: Option<&str>,
    session: Option<&RegisteredSession>,
) -> Reported {
    let session_status = session.and_then(|session| session.status.as_deref());
    if session_status == Some("busy") {
        return Reported::Working;
    }
    if is_terminal(state, tempo) && !(state == "done" && job.recurring) {
        return Reported::Ended;
    }
    if tempo == Some("blocked") || session_status == Some("waiting") {
        return Reported::Blocked;
    }
    Reported::Working
}
