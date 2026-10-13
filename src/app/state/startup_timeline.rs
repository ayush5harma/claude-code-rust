// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// `SessionStart` hooks that finished before the session connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupHooks {
    pub count: usize,
    pub last_finished: Duration,
}

/// One measured point of the launch, as an offset from app start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupMilestone {
    pub label: &'static str,
    pub at: Duration,
    pub note: Option<String>,
}

/// What launch looked like, from moments the app itself observed.
///
/// Offsets are measured from the app's own start with a monotonic clock. The
/// hook times come from the bridge as epoch milliseconds, so they are placed
/// on the same line through the wall-clock reading taken at app start; the
/// two clocks read the same machine.
#[derive(Debug, Clone)]
pub struct StartupTimeline {
    origin: Instant,
    origin_wall_ms: u64,
    bridge_launched: Option<Duration>,
    connected: Option<Duration>,
    ready: Option<Duration>,
    session_start_hooks: Option<StartupHooks>,
}

impl Default for StartupTimeline {
    fn default() -> Self {
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Self::at(Instant::now(), u64::try_from(wall.as_millis()).unwrap_or(u64::MAX))
    }
}

impl StartupTimeline {
    #[must_use]
    pub fn at(origin: Instant, origin_wall_ms: u64) -> Self {
        Self {
            origin,
            origin_wall_ms,
            bridge_launched: None,
            connected: None,
            ready: None,
            session_start_hooks: None,
        }
    }

    /// The app started the bridge task.
    pub fn mark_bridge_launched(&mut self, now: Instant) {
        self.bridge_launched.get_or_insert_with(|| now.saturating_duration_since(self.origin));
    }

    /// The first session connected.
    pub fn mark_connected(&mut self, now: Instant) {
        self.connected.get_or_insert_with(|| now.saturating_duration_since(self.origin));
    }

    /// The instant the app started; the welcome crab's connecting loop is
    /// timed from it.
    #[must_use]
    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// The instant the first session of the process connected, if it has.
    #[must_use]
    pub fn connected_at(&self) -> Option<Instant> {
        self.connected.and_then(|connected| self.origin.checked_add(connected))
    }

    /// Whether the first connected session has not yet been shown ready.
    #[must_use]
    pub fn awaiting_ready(&self) -> bool {
        self.connected.is_some() && self.ready.is_none()
    }

    /// The first frame after connecting in which the composer accepts a prompt.
    pub fn mark_ready(&mut self, now: Instant) {
        if self.connected.is_some() {
            self.ready.get_or_insert_with(|| now.saturating_duration_since(self.origin));
        }
    }

    /// A hook finished at `finished_wall_ms` (the bridge's clock, epoch
    /// milliseconds). Only `SessionStart` hooks that finished before the
    /// session connected belong to launch; later ones are ordinary hook runs.
    pub fn note_hook_finished(&mut self, event: &str, finished_wall_ms: u64) {
        let Some(connected) = self.connected else { return };
        if event != "SessionStart" {
            return;
        }
        let connected_wall_ms = self
            .origin_wall_ms
            .saturating_add(u64::try_from(connected.as_millis()).unwrap_or(u64::MAX));
        if finished_wall_ms > connected_wall_ms {
            return;
        }
        let finished = Duration::from_millis(finished_wall_ms.saturating_sub(self.origin_wall_ms));
        let hooks = self
            .session_start_hooks
            .get_or_insert(StartupHooks { count: 0, last_finished: Duration::ZERO });
        hooks.count += 1;
        hooks.last_finished = hooks.last_finished.max(finished);
    }

    /// Measured milestones in the order they happened.
    #[must_use]
    pub fn milestones(&self) -> Vec<StartupMilestone> {
        let mut milestones = Vec::new();
        let mut push = |label, at: Option<Duration>, note: Option<String>| {
            if let Some(at) = at {
                milestones.push(StartupMilestone { label, at, note });
            }
        };
        push("Bridge launched", self.bridge_launched, None);
        push(
            "SessionStart hooks finished",
            self.session_start_hooks.map(|hooks| hooks.last_finished),
            self.session_start_hooks.map(|hooks| {
                format!("{} hook{}", hooks.count, if hooks.count == 1 { "" } else { "s" })
            }),
        );
        push("SDK connected", self.connected, None);
        push("Ready for input", self.ready, None);
        milestones.sort_by_key(|milestone| milestone.at);
        milestones
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN_WALL_MS: u64 = 1_760_000_000_000;

    fn timeline() -> (StartupTimeline, Instant) {
        let origin = Instant::now();
        (StartupTimeline::at(origin, ORIGIN_WALL_MS), origin)
    }

    fn labels(timeline: &StartupTimeline) -> Vec<(&'static str, u128)> {
        timeline
            .milestones()
            .iter()
            .map(|milestone| (milestone.label, milestone.at.as_millis()))
            .collect()
    }

    #[test]
    fn milestones_are_offsets_from_the_app_start_in_the_order_they_happened() {
        let (mut timeline, origin) = timeline();
        timeline.mark_bridge_launched(origin + Duration::from_millis(12));
        timeline.mark_connected(origin + Duration::from_millis(900));
        assert!(timeline.awaiting_ready());
        timeline.mark_ready(origin + Duration::from_millis(930));
        assert!(!timeline.awaiting_ready());
        assert_eq!(
            labels(&timeline),
            [("Bridge launched", 12), ("SDK connected", 900), ("Ready for input", 930)]
        );
    }

    #[test]
    fn only_the_first_mark_of_each_milestone_counts() {
        let (mut timeline, origin) = timeline();
        timeline.mark_bridge_launched(origin + Duration::from_millis(10));
        timeline.mark_bridge_launched(origin + Duration::from_millis(5_000));
        timeline.mark_connected(origin + Duration::from_millis(700));
        timeline.mark_connected(origin + Duration::from_millis(9_000));
        timeline.mark_ready(origin + Duration::from_millis(710));
        timeline.mark_ready(origin + Duration::from_millis(9_100));
        assert_eq!(
            labels(&timeline),
            [("Bridge launched", 10), ("SDK connected", 700), ("Ready for input", 710)]
        );
    }

    #[test]
    fn ready_before_connected_is_not_recorded() {
        let (mut timeline, origin) = timeline();
        timeline.mark_ready(origin + Duration::from_millis(50));
        assert!(!timeline.awaiting_ready());
        assert_eq!(labels(&timeline), []);
        timeline.mark_connected(origin + Duration::from_millis(60));
        assert!(timeline.awaiting_ready());
    }

    #[test]
    fn session_start_hooks_that_finished_before_connecting_are_placed_by_their_wall_time() {
        let (mut timeline, origin) = timeline();
        timeline.mark_bridge_launched(origin + Duration::from_millis(20));
        timeline.mark_connected(origin + Duration::from_millis(2_000));
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS + 400);
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS + 1_650);
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS + 1_000);
        let hooks = timeline
            .milestones()
            .into_iter()
            .find(|milestone| milestone.label == "SessionStart hooks finished")
            .expect("hook milestone");
        assert_eq!(hooks.at, Duration::from_millis(1_650));
        assert_eq!(hooks.note.as_deref(), Some("3 hooks"));
        assert_eq!(
            labels(&timeline).iter().map(|(label, _)| *label).collect::<Vec<_>>(),
            ["Bridge launched", "SessionStart hooks finished", "SDK connected"]
        );
    }

    #[test]
    fn hooks_after_the_connect_and_other_events_are_not_launch() {
        let (mut timeline, origin) = timeline();
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS + 100);
        timeline.mark_connected(origin + Duration::from_millis(1_000));
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS + 1_001);
        timeline.note_hook_finished("PreToolUse", ORIGIN_WALL_MS + 500);
        assert_eq!(labels(&timeline), [("SDK connected", 1_000)]);
    }

    #[test]
    fn a_hook_stamped_before_the_app_started_counts_from_zero() {
        let (mut timeline, origin) = timeline();
        timeline.mark_connected(origin + Duration::from_millis(1_000));
        timeline.note_hook_finished("SessionStart", ORIGIN_WALL_MS - 30);
        let at = timeline.milestones().iter().find(|m| m.note.is_some()).map(|m| m.at);
        assert_eq!(at, Some(Duration::ZERO));
        assert_eq!(timeline.milestones().first().and_then(|m| m.note.as_deref()), Some("1 hook"));
    }
}
