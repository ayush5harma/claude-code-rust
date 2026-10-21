// SPDX-License-Identifier: Apache-2.0

use super::super::{App, SystemSeverity};
use super::notices::emit_system_notice;
use crate::agent::model::HookActivity;
use crate::app::state::{HOOK_NOTICE_SLOW_THRESHOLD, HookRun};

/// Record one hook report and, once the saved `presentation.hookNotices`
/// preference is known, say what a finished hook earned.
pub(super) fn handle_hook_activity(app: &mut App, activity: &HookActivity) {
    if let Some(run) = app.session_runtime.hook_runs.record(activity)
        && let Some(finished_at_ms) = run.finished_at_ms
    {
        app.startup.timeline.note_hook_finished(&run.event, finished_at_ms);
    }
    flush_pending_hook_notices(app);
}

/// Decide the notice of every finished run still waiting for one. `SessionStart`
/// hooks finish before the app has read its settings, so they wait here for
/// the first snapshot instead of being judged by a guessed preference.
pub(crate) fn flush_pending_hook_notices(app: &mut App) {
    if app.config.snapshot.is_none() {
        return;
    }
    let mode = app.config.hook_notice_mode_effective();
    for run in app.session_runtime.hook_runs.take_pending_notices() {
        let Some(text) = run.notice(mode, HOOK_NOTICE_SLOW_THRESHOLD) else { continue };
        emit_hook_notice(app, &run, &text);
    }
}

fn emit_hook_notice(app: &mut App, run: &HookRun, text: &str) {
    tracing::debug!(
        target: crate::logging::targets::APP_SESSION,
        event_name = "hook_notice_emitted",
        message = "hook notice emitted",
        outcome = "slow",
        hook_event = %run.event,
        hook_id = %run.id,
    );
    emit_system_notice(app, SystemSeverity::Info, text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::{HookOutcome, HookPhase};
    use crate::agent::settings::SettingsSnapshot;
    use crate::app::{MessageBlock, MessageRole};
    use serde_json::json;

    fn report(id: &str, event: &str, phase: HookPhase, at: u64) -> HookActivity {
        HookActivity {
            hook_id: id.to_owned(),
            hook_name: format!("{event}:startup"),
            hook_event: event.to_owned(),
            phase,
            outcome: (phase == HookPhase::Finished).then_some(HookOutcome::Success),
            exit_code: None,
            output: None,
            observed_at_ms: at,
        }
    }

    fn run_hook(app: &mut App, id: &str, event: &str, took_ms: u64) {
        handle_hook_activity(app, &report(id, event, HookPhase::Started, 10_000));
        handle_hook_activity(app, &report(id, event, HookPhase::Finished, 10_000 + took_ms));
    }

    fn set_mode(app: &mut App, mode: &str) {
        let snapshot = app.config.snapshot.get_or_insert_with(SettingsSnapshot::default);
        snapshot
            .values
            .extend(SettingsSnapshot::test_value("presentation.hookNotices", json!(mode)).values);
    }

    fn app_with_settings() -> App {
        let mut app = App::test_default();
        app.config.snapshot = Some(SettingsSnapshot::default());
        app
    }

    /// Every notice in the transcript, with the severity it was shown at.
    fn notices(app: &App) -> Vec<(SystemSeverity, String)> {
        app.transcript
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter_map(|block| match block {
                MessageBlock::Notice(notice) => Some((notice.severity, notice.text.text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_slow_hook_is_noticed_dimly_and_a_fast_one_is_not() {
        let mut app = app_with_settings();
        run_hook(&mut app, "fast", "SessionStart", 40);
        assert_eq!(notices(&app), []);
        run_hook(&mut app, "slow", "SessionStart", 2_500);
        assert_eq!(
            notices(&app),
            [(SystemSeverity::Info, "Hook SessionStart:startup took 2.5 s".to_owned())]
        );
        assert!(matches!(
            app.transcript.messages.last().map(|message| message.role.clone()),
            Some(MessageRole::System(Some(SystemSeverity::Info)))
        ));
    }

    #[test]
    fn a_failed_hook_is_recorded_but_left_to_the_bridge_to_notice() {
        let mut app = app_with_settings();
        let named = |activity: HookActivity| HookActivity {
            hook_name: "PreToolUse:Bash".to_owned(),
            hook_event: "PreToolUse".to_owned(),
            ..activity
        };
        handle_hook_activity(&mut app, &named(report("deny", "PreToolUse", HookPhase::Started, 1)));
        handle_hook_activity(
            &mut app,
            &named(HookActivity {
                outcome: Some(HookOutcome::Error),
                exit_code: Some(2),
                output: Some("rm -rf is not allowed".to_owned()),
                ..report("deny", "PreToolUse", HookPhase::Finished, 30)
            }),
        );
        // The bridge decides whether a failure or a blocking decision needs a notice.
        assert_eq!(notices(&app), []);
        assert_eq!(app.session_runtime.hook_runs.summary().failed, 1);
    }

    #[test]
    fn the_saved_mode_all_notices_a_fast_hook_and_off_notices_nothing() {
        let mut app = app_with_settings();
        set_mode(&mut app, "all");
        run_hook(&mut app, "fast", "Stop", 12);
        assert_eq!(notices(&app).len(), 1);

        let mut quiet = app_with_settings();
        set_mode(&mut quiet, "off");
        run_hook(&mut quiet, "slow", "SessionStart", 9_000);
        handle_hook_activity(
            &mut quiet,
            &HookActivity {
                outcome: Some(HookOutcome::Error),
                ..report("bad", "Stop", HookPhase::Finished, 5)
            },
        );
        assert_eq!(notices(&quiet), []);
    }

    #[test]
    fn hooks_that_finish_before_the_settings_arrive_are_judged_by_the_saved_mode_not_a_guess() {
        let mut app = App::test_default();
        run_hook(&mut app, "startup-slow", "SessionStart", 2_000);
        assert_eq!(notices(&app), [], "no preference is known yet");

        set_mode(&mut app, "off");
        flush_pending_hook_notices(&mut app);
        assert_eq!(notices(&app), [], "off was saved, so the waiting notice never appears");

        let mut app = App::test_default();
        run_hook(&mut app, "startup-slow", "SessionStart", 2_000);
        app.config.snapshot = Some(SettingsSnapshot::default());
        flush_pending_hook_notices(&mut app);
        flush_pending_hook_notices(&mut app);
        assert_eq!(notices(&app).len(), 1, "a waiting notice is shown once");
    }
}
