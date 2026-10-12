// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use super::common::{detail_kv, section_heading};
use crate::app::App;
use crate::app::format_elapsed;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let lines = status_lines(app);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

pub(crate) fn status_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // ---- Session ----
    lines.push(section_heading("Session"));
    lines.push(detail_kv("Version", env!("CARGO_PKG_VERSION"), Color::White));
    lines.push(detail_kv("Session name", &derive_session_name(app), Color::White));

    let session_id_str = app
        .session_runtime
        .session_id
        .as_ref()
        .map_or_else(|| "(none)".to_owned(), std::string::ToString::to_string);
    lines.push(detail_kv("Session ID", &session_id_str, Color::White));

    lines.push(detail_kv("cwd", &app.cwd, Color::White));

    if let Some(branch) = app.git_branch() {
        lines.push(detail_kv("Git branch", branch, Color::White));
    }

    lines.push(Line::default());

    // ---- Account ----
    if let Some(ref account) = app.session_runtime.account_info {
        lines.push(section_heading("Account"));
        lines.push(detail_kv("Login method", &login_method_label(account), Color::White));
        if let Some(ref provider) = account.api_provider {
            lines.push(detail_kv("API provider", provider.label(), Color::White));
        }
        if let Some(ref org) = account.organization
            && !org.is_empty()
        {
            lines.push(detail_kv("Organization", org, Color::White));
        }
        if let Some(ref email) = account.email
            && !email.is_empty()
        {
            lines.push(detail_kv("Email", email, Color::White));
        }
        if let Some(ref sub) = account.subscription_type
            && !sub.is_empty()
        {
            lines.push(detail_kv("Subscription", sub, Color::White));
        }
        lines.push(Line::default());
    }

    // ---- Model ----
    lines.push(section_heading("Model"));
    lines.push(detail_kv("Model", &model_display(app), Color::White));
    if let Some(current_model) = app.session_runtime.current_model.as_ref() {
        lines.push(detail_kv("Resolved model ID", &current_model.resolved_id, Color::White));
        if let Some(requested_id) = current_model.requested_id.as_deref()
            && requested_id != current_model.resolved_id
        {
            lines.push(detail_kv("Requested model", requested_id, Color::White));
        }
    }

    if let Some(ref mode) = app.session_runtime.mode {
        lines.push(detail_kv("Mode", &mode.current_mode_name, Color::White));
    }

    lines.push(Line::default());

    // ---- Settings ----
    lines.push(section_heading("Settings"));

    let memory_path = resolve_memory_path(app);
    lines.push(detail_kv("Memory", &memory_path, Color::White));

    let sources = setting_sources(app);
    lines.push(detail_kv("Setting sources", &sources, Color::White));

    lines.push(Line::default());
    lines.extend(startup_lines(app));
    lines.push(Line::default());
    lines.extend(hook_lines(app));

    lines
}

/// Launch milestones the app measured itself, as offsets from app start.
fn startup_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![section_heading("Startup")];
    let milestones = app.startup.timeline.milestones();
    if milestones.is_empty() {
        lines.push(detail_kv("Measured", "nothing yet", Color::White));
    }
    for milestone in milestones {
        let note = milestone.note.map_or_else(String::new, |note| format!(" ({note})"));
        let value = format!("+{}{note}", format_elapsed(milestone.at));
        lines.push(detail_kv(milestone.label, &value, Color::White));
    }
    lines
}

/// Count, total time and slowest runs of the active session's hooks.
fn hook_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![section_heading("Hooks this session")];
    let summary = app.session_runtime.hook_runs.summary();
    if summary.runs == 0 {
        lines.push(detail_kv("Runs", "none reported", Color::White));
        return lines;
    }
    let mut count = if summary.dropped == 0 {
        summary.runs.to_string()
    } else {
        format!("{} kept, {} older not kept", summary.runs, summary.dropped)
    };
    if summary.failed > 0 {
        count = format!("{count} ({} failed)", summary.failed);
    }
    let mut parts = vec![count];
    if summary.runs > summary.finished {
        parts.push(format!("{} still running", summary.runs - summary.finished));
    }
    parts.push(format!("{} in total", format_elapsed(summary.total)));
    lines.push(detail_kv("Runs", &parts.join(", "), Color::White));
    for (rank, run) in summary.slowest.iter().flatten().enumerate() {
        let took = run.duration().map_or_else(String::new, format_elapsed);
        lines.push(detail_kv(
            &format!("Slowest {}", rank + 1),
            &format!("{} {took}", run.label()),
            Color::White,
        ));
    }
    lines
}

fn derive_session_name(app: &App) -> String {
    if let Some(title) = app.session_runtime.session_title.as_ref() {
        return title.clone();
    }
    if let Some(ref sid) = app.session_runtime.session_id {
        let sid_str = sid.to_string();
        if let Some(session) = app.recent_sessions.iter().find(|s| s.session_id == sid_str) {
            if !session.summary.trim().is_empty() {
                let summary = &session.summary;
                return if summary.len() > 60 {
                    format!("{}...", &summary[..57])
                } else {
                    summary.clone()
                };
            }
            if let Some(ref prompt) = session.first_prompt
                && !prompt.trim().is_empty()
            {
                return if prompt.len() > 60 {
                    format!("{}...", &prompt[..57])
                } else {
                    prompt.clone()
                };
            }
        }
    }
    "(unnamed session)".to_owned()
}

fn model_display(app: &App) -> String {
    let Some(current_model) = app.session_runtime.current_model.as_ref() else {
        return "(not set)".to_owned();
    };
    current_model.display_name_long.clone()
}

pub(crate) fn login_method_label(account: &crate::agent::model::AccountInfo) -> String {
    account.login_method_label()
}

fn resolve_memory_path(app: &App) -> String {
    let Some(paths) =
        crate::claude_paths::ClaudePaths::resolve(app.settings_home_override.as_deref())
    else {
        return "(unable to resolve Claude configuration directory)".to_owned();
    };
    let memory_md = paths.default_memory_file(&app.cwd_raw);

    if memory_md.exists() {
        format!("auto memory ({})", memory_md.display())
    } else {
        "(no memory file found)".to_owned()
    }
}

fn setting_sources(app: &App) -> String {
    app.config.snapshot.as_ref().map_or_else(
        || "(not inspected)".to_owned(),
        |snapshot| {
            snapshot
                .sources
                .iter()
                .map(|source| format!("{} ({})", source.scope.label(), source.status))
                .collect::<Vec<_>>()
                .join(", ")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_contains_version() {
        let app = App::test_default();
        let text = lines_to_string(&status_lines(&app));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn status_lines_shows_cwd() {
        let mut app = App::test_default();
        app.cwd = "/test/project".to_owned();
        let text = lines_to_string(&status_lines(&app));
        assert!(text.contains("/test/project"));
    }

    #[test]
    fn status_lines_shows_model() {
        let mut app = App::test_default();
        app.session_runtime.current_model = Some(
            crate::agent::model::CurrentModel::new("claude-sonnet-4-7", "Sonnet", "Sonnet 4.7")
                .authoritative(true),
        );
        let text = lines_to_string(&status_lines(&app));
        assert!(text.contains("Sonnet 4.7"));
    }

    #[test]
    fn status_lines_unnamed_session_fallback() {
        let app = App::test_default();
        let text = lines_to_string(&status_lines(&app));
        assert!(text.contains("(unnamed session)"));
    }

    #[test]
    fn status_lines_name_the_session_by_its_live_title() {
        let mut app = App::test_default();
        app.session_runtime.session_id = Some(crate::agent::model::SessionId::new("test-sess-1"));
        app.recent_sessions = vec![crate::app::RecentSessionInfo {
            session_id: "test-sess-1".to_owned(),
            summary: "Listed summary".to_owned(),
            last_modified_ms: 0,
            file_size_bytes: 0,
            cwd: None,
            git_branch: None,
            custom_title: Some("Stale listed title".to_owned()),
            first_prompt: None,
        }];
        let name_row = |app: &App| {
            status_lines(app)
                .iter()
                .map(std::string::ToString::to_string)
                .find(|line| line.contains("Session name"))
                .expect("session name row")
        };
        assert!(name_row(&app).contains("Listed summary"));
        app.session_runtime.session_title = Some("probe-e2e".to_owned());
        assert!(name_row(&app).contains("probe-e2e"));
    }

    #[test]
    fn section_headers_present() {
        let app = App::test_default();
        let text = lines_to_string(&status_lines(&app));
        assert!(text.contains("Session"));
        assert!(text.contains("Model"));
        assert!(text.contains("Settings"));
    }

    fn kv_rows(app: &App) -> Vec<String> {
        lines_to_string(&status_lines(app)).lines().map(str::to_owned).collect()
    }

    #[test]
    fn startup_section_lists_only_the_milestones_measured_in_order() {
        use crate::app::StartupTimeline;
        use std::time::{Duration, Instant};
        let mut app = App::test_default();
        let rows = kv_rows(&app);
        assert!(rows.iter().any(|row| row == "Startup"));
        assert!(rows.iter().any(|row| row == "Measured: nothing yet"));

        let origin = Instant::now();
        app.startup.timeline = StartupTimeline::at(origin, 1_760_000_000_000);
        app.startup.timeline.mark_bridge_launched(origin + Duration::from_millis(12));
        app.startup.timeline.mark_connected(origin + Duration::from_millis(2_450));
        app.startup.timeline.note_hook_finished("SessionStart", 1_760_000_002_400);
        let rows = kv_rows(&app);
        let at = |label: &str| rows.iter().position(|row| row.starts_with(label));
        assert_eq!(
            rows.iter().find(|row| row.starts_with("Bridge launched")).map(String::as_str),
            Some("Bridge launched: +12 ms")
        );
        assert_eq!(
            rows.iter().find(|row| row.starts_with("SessionStart hooks")).map(String::as_str),
            Some("SessionStart hooks finished: +2.4 s (1 hook)")
        );
        assert_eq!(
            rows.iter().find(|row| row.starts_with("SDK connected")).map(String::as_str),
            Some("SDK connected: +2.5 s")
        );
        assert!(at("Bridge launched") < at("SessionStart hooks"));
        assert!(at("SessionStart hooks") < at("SDK connected"));
        assert_eq!(at("Ready for input"), None, "nothing is shown for a moment not yet measured");
    }

    #[test]
    fn hooks_section_summarises_count_total_and_the_slowest_three() {
        use crate::agent::model::{HookActivity, HookOutcome, HookPhase};
        let mut app = App::test_default();
        assert!(kv_rows(&app).iter().any(|row| row == "Runs: none reported"));

        let mut report = |id: &str, name: &str, phase, at, outcome| {
            app.session_runtime.hook_runs.record(&HookActivity {
                hook_id: id.to_owned(),
                hook_name: name.to_owned(),
                hook_event: name.split(':').next().unwrap_or_default().to_owned(),
                phase,
                outcome,
                exit_code: None,
                output: None,
                observed_at_ms: at,
            });
        };
        for (id, name, took, outcome) in [
            ("a", "SessionStart:startup", 1_200, HookOutcome::Success),
            ("b", "SessionStart:compact", 40, HookOutcome::Success),
            ("c", "PreToolUse:Bash", 300, HookOutcome::Error),
            ("d", "Stop:*", 900, HookOutcome::Success),
        ] {
            report(id, name, HookPhase::Started, 5_000, None);
            report(id, name, HookPhase::Finished, 5_000 + took, Some(outcome));
        }
        report("e", "PostToolUse:Edit", HookPhase::Started, 9_000, None);
        let rows = kv_rows(&app);
        let row = |label: &str| rows.iter().find(|row| row.starts_with(label)).map(String::as_str);
        assert_eq!(row("Runs"), Some("Runs: 5 (1 failed), 1 still running, 2.4 s in total"));
        assert_eq!(row("Slowest 1"), Some("Slowest 1: SessionStart:startup 1.2 s"));
        assert_eq!(row("Slowest 2"), Some("Slowest 2: Stop:* 900 ms"));
        assert_eq!(row("Slowest 3"), Some("Slowest 3: PreToolUse:Bash 300 ms"));
        assert_eq!(row("Slowest 4"), None);
    }

    #[test]
    fn login_method_maps_oauth() {
        let account = crate::agent::model::AccountInfo {
            api_key_source: Some("oauth".to_owned()),
            ..Default::default()
        };
        assert_eq!(login_method_label(&account), "Claude Max Account");
    }

    #[test]
    fn login_method_maps_user_key() {
        let account = crate::agent::model::AccountInfo {
            api_key_source: Some("user".to_owned()),
            ..Default::default()
        };
        assert_eq!(login_method_label(&account), "User API key");
    }

    #[test]
    fn login_method_maps_external_provider() {
        let account = crate::agent::model::AccountInfo {
            api_provider: Some(crate::agent::model::AccountApiProvider::Bedrock),
            ..Default::default()
        };
        assert_eq!(login_method_label(&account), "External provider");
    }

    #[test]
    fn status_lines_render_api_provider() {
        let mut app = App::test_default();
        app.session_runtime.account_info = Some(crate::agent::model::AccountInfo {
            api_provider: Some(crate::agent::model::AccountApiProvider::Gateway),
            ..Default::default()
        });

        let text = lines_to_string(&status_lines(&app));

        assert!(text.contains("API provider"));
        assert!(text.contains("Gateway"));
    }

    #[test]
    fn login_method_falls_back_to_unknown() {
        let account = crate::agent::model::AccountInfo::default();
        assert_eq!(login_method_label(&account), "Unknown");
    }

    fn lines_to_string(lines: &[Line<'_>]) -> String {
        lines.iter().map(std::string::ToString::to_string).collect::<Vec<_>>().join("\n")
    }
}
