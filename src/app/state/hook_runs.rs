// SPDX-License-Identifier: Apache-2.0

use crate::agent::model::{HookActivity, HookOutcome, HookPhase};
use std::collections::VecDeque;
use std::time::Duration;

/// Hook runs kept per session; older runs are counted, not kept.
pub const HOOK_RUN_CAPACITY: usize = 200;

/// A finished hook at or above this duration gets a notice under the default
/// `slow_and_failed` mode.
pub const HOOK_NOTICE_SLOW_THRESHOLD: Duration = Duration::from_millis(500);

/// The `presentation.hookNotices` setting: which finished hooks get a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookNoticeMode {
    #[default]
    SlowAndFailed,
    All,
    Off,
}

impl HookNoticeMode {
    #[must_use]
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "slow_and_failed" => Some(Self::SlowAndFailed),
            "all" => Some(Self::All),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// One hook execution as the SDK reported it. Times are the bridge's
/// observation times in epoch milliseconds: the SDK declares no hook timing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    pub id: String,
    pub name: String,
    pub event: String,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub outcome: Option<HookOutcome>,
    pub exit_code: Option<i64>,
    pub output: Option<String>,
    /// Finished, and no notice decision made for it yet.
    pub notice_pending: bool,
}

/// What the Status tab and the notice rule need to say about a run, decided
/// once here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookNotice {
    pub failed: bool,
    pub text: String,
}

impl HookRun {
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished_at_ms.is_some()
    }

    /// Started-to-finished time; absent when either end was not observed.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        let (started, finished) = (self.started_at_ms?, self.finished_at_ms?);
        Some(Duration::from_millis(finished.saturating_sub(started)))
    }

    /// The SDK's outcome decides; an exit code only speaks when it gave none.
    #[must_use]
    pub fn failed(&self) -> bool {
        match self.outcome {
            Some(outcome) => outcome != HookOutcome::Success,
            None => self.exit_code.is_some_and(|code| code != 0),
        }
    }

    /// `SessionStart:startup`-style names already lead with their event.
    #[must_use]
    pub fn label(&self) -> String {
        if self.name.starts_with(&self.event) {
            self.name.clone()
        } else {
            format!("{}:{}", self.event, self.name)
        }
    }

    /// The notice a finished run earns under `mode`, if any.
    #[must_use]
    pub fn notice(&self, mode: HookNoticeMode, slow_threshold: Duration) -> Option<HookNotice> {
        if !self.is_finished() || mode == HookNoticeMode::Off {
            return None;
        }
        let label = self.label();
        if self.failed() {
            let verdict = match (self.outcome, self.exit_code) {
                (Some(HookOutcome::Cancelled), _) => "was cancelled".to_owned(),
                (_, Some(code)) => format!("failed (exit {code})"),
                _ => "failed".to_owned(),
            };
            let detail = self.output.as_deref().map_or_else(String::new, |out| format!(": {out}"));
            return Some(HookNotice {
                failed: true,
                text: format!("Hook {label} {verdict}{detail}"),
            });
        }
        let duration = self.duration();
        let slow = duration.is_some_and(|duration| duration >= slow_threshold);
        if mode == HookNoticeMode::SlowAndFailed && !slow {
            return None;
        }
        let text = duration.map_or_else(
            || format!("Hook {label} finished"),
            |duration| format!("Hook {label} took {}", format_elapsed(duration)),
        );
        Some(HookNotice { failed: false, text })
    }
}

/// `620 ms`, `2.5 s`, `1 min 5 s`.
#[must_use]
pub fn format_elapsed(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis} ms");
    }
    if millis < 60_000 {
        let tenths = (millis + 50) / 100;
        return format!("{}.{} s", tenths / 10, tenths % 10);
    }
    let seconds = (millis + 500) / 1_000;
    format!("{} min {} s", seconds / 60, seconds % 60)
}

/// Count, total time and slowest runs of the retained hook runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSummary<'a> {
    pub runs: usize,
    pub finished: usize,
    pub failed: usize,
    pub total: Duration,
    pub slowest: [Option<&'a HookRun>; 3],
    /// Runs seen but no longer retained.
    pub dropped: u64,
}

/// Bounded record of the active session's hook runs.
#[derive(Debug, Default)]
pub struct HookRuns {
    runs: VecDeque<HookRun>,
    dropped: u64,
}

impl HookRuns {
    pub fn clear(&mut self) {
        self.runs.clear();
        self.dropped = 0;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Apply one bridge report. Returns the run when this report finished it.
    pub fn record(&mut self, activity: &HookActivity) -> Option<&HookRun> {
        let at = Some(activity.observed_at_ms);
        match activity.phase {
            HookPhase::Started => {
                self.push(HookRun {
                    id: activity.hook_id.clone(),
                    name: activity.hook_name.clone(),
                    event: activity.hook_event.clone(),
                    started_at_ms: at,
                    finished_at_ms: None,
                    outcome: None,
                    exit_code: None,
                    output: None,
                    notice_pending: false,
                });
                None
            }
            HookPhase::Progress => {
                if let Some(run) = self.open_run_mut(&activity.hook_id) {
                    run.output.clone_from(&activity.output);
                }
                None
            }
            HookPhase::Finished => {
                if self.open_run_mut(&activity.hook_id).is_none() {
                    // Its start was not observed, so it keeps no duration.
                    self.push(HookRun {
                        id: activity.hook_id.clone(),
                        name: activity.hook_name.clone(),
                        event: activity.hook_event.clone(),
                        started_at_ms: None,
                        finished_at_ms: None,
                        outcome: None,
                        exit_code: None,
                        output: None,
                        notice_pending: false,
                    });
                }
                let run = self.open_run_mut(&activity.hook_id)?;
                run.finished_at_ms = at;
                run.outcome = activity.outcome;
                run.exit_code = activity.exit_code;
                run.output.clone_from(&activity.output);
                run.notice_pending = true;
                Some(&*run)
            }
        }
    }

    /// Finished runs that have not been judged for a notice, oldest first.
    /// Each run is handed out once.
    pub fn take_pending_notices(&mut self) -> Vec<HookRun> {
        self.runs
            .iter_mut()
            .filter(|run| run.notice_pending)
            .map(|run| {
                run.notice_pending = false;
                run.clone()
            })
            .collect()
    }

    #[must_use]
    pub fn summary(&self) -> HookSummary<'_> {
        let mut slowest: [Option<&HookRun>; 3] = [None; 3];
        let mut finished = 0;
        let mut failed = 0;
        let mut total = Duration::ZERO;
        for run in &self.runs {
            if run.is_finished() {
                finished += 1;
                failed += usize::from(run.failed());
            }
            let Some(duration) = run.duration() else { continue };
            total += duration;
            for index in 0..slowest.len() {
                if slowest[index]
                    .is_none_or(|previous| duration > previous.duration().unwrap_or_default())
                {
                    slowest[index..].rotate_right(1);
                    slowest[index] = Some(run);
                    break;
                }
            }
        }
        HookSummary {
            runs: self.runs.len(),
            finished,
            failed,
            total,
            slowest,
            dropped: self.dropped,
        }
    }

    fn open_run_mut(&mut self, id: &str) -> Option<&mut HookRun> {
        self.runs.iter_mut().rev().find(|run| run.id == id && !run.is_finished())
    }

    fn push(&mut self, run: HookRun) {
        self.runs.push_back(run);
        while self.runs.len() > HOOK_RUN_CAPACITY {
            self.runs.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOW: Duration = Duration::from_millis(500);

    fn activity(id: &str, phase: HookPhase, at: u64) -> HookActivity {
        HookActivity {
            hook_id: id.to_owned(),
            hook_name: "SessionStart:startup".to_owned(),
            hook_event: "SessionStart".to_owned(),
            phase,
            outcome: None,
            exit_code: None,
            output: None,
            observed_at_ms: at,
        }
    }

    fn finished(id: &str, at: u64, outcome: HookOutcome, exit_code: Option<i64>) -> HookActivity {
        HookActivity { outcome: Some(outcome), exit_code, ..activity(id, HookPhase::Finished, at) }
    }

    fn run_of(started: u64, ended: u64, outcome: HookOutcome, exit_code: Option<i64>) -> HookRun {
        let mut runs = HookRuns::default();
        runs.record(&activity("h", HookPhase::Started, started));
        runs.record(&finished("h", ended, outcome, exit_code)).cloned().expect("finished run")
    }

    #[test]
    fn duration_is_the_time_between_the_started_and_finished_reports() {
        let run = run_of(1_000, 3_500, HookOutcome::Success, Some(0));
        assert_eq!(run.duration(), Some(Duration::from_millis(2_500)));
    }

    #[test]
    fn a_finish_without_an_observed_start_is_kept_without_a_duration() {
        let mut runs = HookRuns::default();
        let run = runs.record(&finished("lost", 9_000, HookOutcome::Success, None)).cloned();
        assert_eq!(run.and_then(|run| run.duration()), None);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn progress_updates_an_open_run_and_never_opens_one() {
        let mut runs = HookRuns::default();
        let progress = HookActivity {
            output: Some("warming".to_owned()),
            ..activity("h", HookPhase::Progress, 10)
        };
        assert!(runs.record(&progress).is_none());
        assert!(runs.is_empty());
        runs.record(&activity("h", HookPhase::Started, 5));
        runs.record(&progress);
        let run = runs.record(&finished("h", 20, HookOutcome::Success, None)).cloned();
        assert_eq!(run.and_then(|run| run.output), None, "the finish reports its own output");
    }

    #[test]
    fn a_reused_hook_id_closes_only_its_latest_open_run() {
        let mut runs = HookRuns::default();
        runs.record(&activity("same", HookPhase::Started, 0));
        runs.record(&finished("same", 100, HookOutcome::Success, None));
        runs.record(&activity("same", HookPhase::Started, 1_000));
        let second = runs.record(&finished("same", 4_000, HookOutcome::Success, None)).cloned();
        assert_eq!(second.and_then(|run| run.duration()), Some(Duration::from_millis(3_000)));
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn the_ring_keeps_the_newest_runs_and_counts_the_rest() {
        let mut runs = HookRuns::default();
        for index in 0..HOOK_RUN_CAPACITY + 5 {
            runs.record(&activity(&format!("h{index}"), HookPhase::Started, 0));
        }
        let summary = runs.summary();
        assert_eq!(summary.runs, 200);
        assert_eq!(summary.dropped, 5);
        // The oldest run was evicted, so its finish has no open run to close.
        let evicted = runs.record(&finished("h0", 1, HookOutcome::Success, None)).cloned();
        assert_eq!(evicted.and_then(|run| run.started_at_ms), None);
        let newest = runs.record(&finished("h204", 2_000, HookOutcome::Success, None)).cloned();
        assert_eq!(newest.and_then(|run| run.duration()), Some(Duration::from_millis(2_000)));
    }

    #[test]
    fn clear_forgets_runs_and_the_dropped_count() {
        let mut runs = HookRuns::default();
        for index in 0..=HOOK_RUN_CAPACITY {
            runs.record(&activity(&format!("h{index}"), HookPhase::Started, 0));
        }
        runs.clear();
        assert!(runs.is_empty());
        assert_eq!(runs.summary().dropped, 0);
    }

    #[test]
    fn the_summary_totals_finished_time_and_lists_the_slowest_three() {
        let mut runs = HookRuns::default();
        for (id, ms, outcome) in [
            ("a", 100_u64, HookOutcome::Success),
            ("b", 2_000, HookOutcome::Success),
            ("c", 700, HookOutcome::Error),
            ("d", 1_500, HookOutcome::Success),
        ] {
            runs.record(&activity(id, HookPhase::Started, 10_000));
            runs.record(&finished(id, 10_000 + ms, outcome, None));
        }
        runs.record(&activity("open", HookPhase::Started, 10_000));
        let summary = runs.summary();
        assert_eq!(summary.runs, 5);
        assert_eq!(summary.finished, 4);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.total, Duration::from_millis(4_300));
        let slowest: Vec<&str> =
            summary.slowest.iter().flatten().map(|run| run.id.as_str()).collect();
        assert_eq!(slowest, ["b", "d", "c"]);
    }

    #[test]
    fn slow_and_failed_notices_a_slow_hook_at_the_threshold_but_not_a_fast_one() {
        let at_threshold = run_of(0, 500, HookOutcome::Success, Some(0));
        let just_under = run_of(0, 499, HookOutcome::Success, Some(0));
        let slow = run_of(0, 2_500, HookOutcome::Success, Some(0));
        let mode = HookNoticeMode::SlowAndFailed;
        assert_eq!(
            at_threshold.notice(mode, SLOW),
            Some(HookNotice {
                failed: false,
                text: "Hook SessionStart:startup took 500 ms".to_owned()
            })
        );
        assert_eq!(just_under.notice(mode, SLOW), None);
        assert_eq!(
            slow.notice(mode, SLOW).map(|notice| notice.text),
            Some("Hook SessionStart:startup took 2.5 s".to_owned())
        );
    }

    #[test]
    fn a_failed_or_cancelled_hook_is_noticed_however_fast_it_was() {
        let mut runs = HookRuns::default();
        let pre_tool_use = |activity: HookActivity| HookActivity {
            hook_name: "PreToolUse:Bash".to_owned(),
            hook_event: "PreToolUse".to_owned(),
            ..activity
        };
        runs.record(&pre_tool_use(activity("f", HookPhase::Started, 0)));
        let failed = runs
            .record(&pre_tool_use(HookActivity {
                output: Some("denied by policy".to_owned()),
                ..finished("f", 20, HookOutcome::Error, Some(2))
            }))
            .cloned()
            .expect("finished");
        assert_eq!(
            failed.notice(HookNoticeMode::SlowAndFailed, SLOW),
            Some(HookNotice {
                failed: true,
                text: "Hook PreToolUse:Bash failed (exit 2): denied by policy".to_owned()
            })
        );
        let cancelled = run_of(0, 5, HookOutcome::Cancelled, None);
        assert_eq!(
            cancelled.notice(HookNoticeMode::SlowAndFailed, SLOW).map(|notice| notice.text),
            Some("Hook SessionStart:startup was cancelled".to_owned())
        );
    }

    #[test]
    fn the_outcome_decides_failure_and_the_exit_code_only_fills_in_when_there_is_none() {
        let success_with_code = run_of(0, 5, HookOutcome::Success, Some(1));
        assert!(!success_with_code.failed());
        let mut no_outcome = success_with_code;
        no_outcome.outcome = None;
        assert!(no_outcome.failed());
        no_outcome.exit_code = Some(0);
        assert!(!no_outcome.failed());
    }

    #[test]
    fn all_notices_every_finished_hook_and_off_none() {
        let fast = run_of(0, 12, HookOutcome::Success, Some(0));
        assert_eq!(
            fast.notice(HookNoticeMode::All, SLOW).map(|notice| notice.text),
            Some("Hook SessionStart:startup took 12 ms".to_owned())
        );
        let failed = run_of(0, 12, HookOutcome::Error, Some(1));
        assert_eq!(failed.notice(HookNoticeMode::Off, SLOW), None);
        let slow = run_of(0, 9_000, HookOutcome::Success, None);
        assert_eq!(slow.notice(HookNoticeMode::Off, SLOW), None);
    }

    #[test]
    fn an_unfinished_run_earns_no_notice_and_a_bare_name_gets_its_event() {
        let mut runs = HookRuns::default();
        runs.record(&activity("open", HookPhase::Started, 0));
        assert!(runs.summary().slowest.iter().all(Option::is_none));
        let mut run = run_of(0, 5_000, HookOutcome::Success, None);
        run.name = "my-script".to_owned();
        assert_eq!(run.label(), "SessionStart:my-script");
        run.finished_at_ms = None;
        assert_eq!(run.notice(HookNoticeMode::All, SLOW), None);
    }

    #[test]
    fn durations_read_as_milliseconds_seconds_and_minutes() {
        let format = |ms: u64| format_elapsed(Duration::from_millis(ms));
        assert_eq!(format(620), "620 ms");
        assert_eq!(format(999), "999 ms");
        assert_eq!(format(1_000), "1.0 s");
        assert_eq!(format(2_450), "2.5 s");
        assert_eq!(format(65_000), "1 min 5 s");
    }

    #[test]
    fn notice_modes_parse_from_their_stored_names() {
        assert_eq!(
            HookNoticeMode::from_stored("slow_and_failed"),
            Some(HookNoticeMode::SlowAndFailed)
        );
        assert_eq!(HookNoticeMode::from_stored("all"), Some(HookNoticeMode::All));
        assert_eq!(HookNoticeMode::from_stored("off"), Some(HookNoticeMode::Off));
        assert_eq!(HookNoticeMode::from_stored("loud"), None);
        assert_eq!(HookNoticeMode::default(), HookNoticeMode::SlowAndFailed);
    }
}
