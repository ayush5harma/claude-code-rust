// SPDX-License-Identifier: Apache-2.0

//! Decides, once and before the first draw, whether the terminal is dark text
//! on light or light text on dark, and hands the answer to the palette.

use crate::claude_paths::ClaudePaths;
use crate::ui::theme::{self, Appearance};
use std::path::Path;
use std::time::Duration;

/// `light` or `dark`; overrides every other signal.
pub const APPEARANCE_ENV: &str = "CLAUDE_RS_APPEARANCE";

/// How long to wait for the terminal to report its background: the library's
/// own default, chosen for SSH. A local terminal answers within a millisecond,
/// and one without the query answers the DA1 sent with it at once, so the wait
/// only runs out when nothing answers at all. A shorter timeout would let a
/// slow link's reply (an SSH round trip can pass 250 ms) arrive after the
/// query gave up, where crossterm would read it as keystrokes.
const QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Where the appearance came from, for the startup log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Environment,
    ClaudeTheme,
    Terminal,
    Default,
}

/// Resolves the appearance and installs it as the palette for this process.
pub fn init() -> (Appearance, Source) {
    let resolved = resolve(
        std::env::var(APPEARANCE_ENV).ok().as_deref(),
        query_terminal,
        claude_theme_setting,
    );
    theme::set_appearance(resolved.0);
    tracing::info!(
        target: crate::logging::targets::APP_LIFECYCLE,
        event_name = "appearance_resolved",
        message = "resolved terminal appearance",
        appearance = ?resolved.0,
        source = ?resolved.1,
    );
    resolved
}

/// The explicit override first, then the background the terminal reports,
/// which is what the text is actually drawn on, then Claude Code's own light
/// or dark theme for terminals that do not answer, then dark. The theme comes
/// second because `dark` is Claude Code's onboarding default, so it often
/// says nothing about the terminal, and the settings are only read when the
/// terminal stays silent.
fn resolve(
    env_value: Option<&str>,
    query: impl FnOnce() -> Option<Appearance>,
    claude_theme: impl FnOnce() -> Option<String>,
) -> (Appearance, Source) {
    if let Some(appearance) = env_value.and_then(parse_override) {
        return (appearance, Source::Environment);
    }
    if let Some(appearance) = query() {
        return (appearance, Source::Terminal);
    }
    if let Some(appearance) = claude_theme().as_deref().and_then(from_claude_theme) {
        return (appearance, Source::ClaudeTheme);
    }
    (Appearance::Dark, Source::Default)
}

fn parse_override(value: &str) -> Option<Appearance> {
    match value.trim().to_ascii_lowercase().as_str() {
        "light" => Some(Appearance::Light),
        "dark" => Some(Appearance::Dark),
        _ => None,
    }
}

/// Claude Code's `theme` values: `dark`, `light`, their `-daltonized` and
/// `-ansi` forms, `auto`, and `custom:<name>`. Only the first two families say
/// which appearance the terminal has.
fn from_claude_theme(value: &str) -> Option<Appearance> {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("light") {
        Some(Appearance::Light)
    } else if value.starts_with("dark") {
        Some(Appearance::Dark)
    } else {
        None
    }
}

/// The `theme` from Claude Code's user settings, where current versions keep
/// it, or from the older global preferences file.
fn claude_theme_setting() -> Option<String> {
    let paths = ClaudePaths::resolve(None)?;
    theme_in(&paths.config_dir.join("settings.json")).or_else(|| theme_in(&paths.preferences))
}

fn theme_in(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let document: serde_json::Value = serde_json::from_str(&raw).ok()?;
    document.get("theme")?.as_str().map(str::to_owned)
}

fn query_terminal() -> Option<Appearance> {
    let mut options = terminal_colorsaurus::QueryOptions::default();
    options.timeout = QUERY_TIMEOUT;
    match terminal_colorsaurus::theme_mode(options) {
        Ok(terminal_colorsaurus::ThemeMode::Light) => Some(Appearance::Light),
        Ok(terminal_colorsaurus::ThemeMode::Dark) => Some(Appearance::Dark),
        Err(error) => {
            tracing::debug!(
                target: crate::logging::targets::APP_LIFECYCLE,
                event_name = "appearance_query_failed",
                message = "terminal did not report its background",
                error_message = %error,
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unanswered() -> Option<Appearance> {
        None
    }

    fn theme(value: &'static str) -> impl FnOnce() -> Option<String> {
        move || Some(value.to_owned())
    }

    fn unread_theme() -> Option<String> {
        panic!("the settings must not be read when an earlier signal decides")
    }

    #[test]
    fn override_wins_over_every_other_signal() {
        let resolved = resolve(Some(" Light "), || Some(Appearance::Dark), unread_theme);
        assert_eq!(resolved, (Appearance::Light, Source::Environment));
    }

    #[test]
    fn terminal_background_wins_over_the_default_claude_theme() {
        // A light terminal with Claude Code still on its onboarding default.
        let resolved = resolve(None, || Some(Appearance::Light), unread_theme);
        assert_eq!(resolved, (Appearance::Light, Source::Terminal));
    }

    #[test]
    fn claude_theme_decides_when_the_terminal_stays_silent() {
        let resolved = resolve(None, unanswered, theme("light-daltonized"));
        assert_eq!(resolved, (Appearance::Light, Source::ClaudeTheme));
        let resolved = resolve(None, unanswered, theme("dark-ansi"));
        assert_eq!(resolved, (Appearance::Dark, Source::ClaudeTheme));
    }

    #[test]
    fn auto_and_custom_themes_say_nothing() {
        for value in ["auto", "custom:ghostty"] {
            let resolved = resolve(None, unanswered, theme(value));
            assert_eq!(resolved, (Appearance::Dark, Source::Default), "{value}");
        }
    }

    #[test]
    fn unknown_override_is_ignored_and_silence_means_dark() {
        let resolved = resolve(Some("sepia"), unanswered, || None);
        assert_eq!(resolved, (Appearance::Dark, Source::Default));
    }

    #[test]
    fn theme_is_read_from_a_settings_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model":"opus","theme":"light"}"#).unwrap();
        assert_eq!(theme_in(&path).as_deref(), Some("light"));
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        assert_eq!(theme_in(&path), None);
        assert_eq!(theme_in(&dir.path().join("missing.json")), None);
    }
}
