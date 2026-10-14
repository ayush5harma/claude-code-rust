// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicU8, Ordering};

// The palette follows Apple's Human Interface Guidelines for color: semantic roles
// rather than raw hues, the system colors Apple published for the 26 releases
// (developer.apple.com/design/human-interface-guidelines/color, revised 2025-06-09),
// a dark and a light variant of each, and at least 4.5:1 contrast for every text
// role against the terminal background and against the app's own fills.
// `tests::every_text_role_meets_the_contrast_floor` enforces the last point.
//
// Values are explicit RGB on purpose. Named ANSI colors are remapped by the
// terminal theme, which is how the MCP badges ended up as white on salmon and
// orange on orange; only the primary label defers to the terminal (`Color::Reset`),
// because the terminal's own foreground is the one color known to suit its
// background. Where Apple's default variant fell short of 4.5:1, the
// increased-contrast variant from the same table is used (dark red, info,
// purple, indigo) or the hue is darkened until it passes (light roles).

/// Whether the terminal is dark text on light or light text on dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

/// The role a badge plays; each maps to a tinted fill and a readable label on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Info,
}

/// One appearance's semantic colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Primary text: the terminal's own foreground.
    pub label: Color,
    /// Values and supporting text one step below the label.
    pub secondary: Color,
    /// Hints, keys, metadata and chrome text.
    pub tertiary: Color,
    /// Rules and dividers; decorative, so not held to the text floor.
    pub separator: Color,
    /// The app's tint: the assistant, focus, the selected item, the mascot.
    pub accent: Color,
    /// Text drawn on an accent fill.
    pub on_accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub info: Color,
    pub cyan: Color,
    pub purple: Color,
    pub indigo: Color,
    pub pink: Color,
    /// Raised surfaces: the user's message, the composer, the selected row.
    pub fill: Color,
    /// Find-in-transcript match and the rewind picker's selection.
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    /// Badge label and fill per tone, in `Tone` declaration order.
    badges: [(Color, Color); 6],
}

const fn rgb(value: (u8, u8, u8)) -> Color {
    Color::Rgb(value.0, value.1, value.2)
}

/// `color` laid over `base` at `percent` opacity, rounded per channel.
const fn tint(color: (u8, u8, u8), base: (u8, u8, u8), percent: u16) -> (u8, u8, u8) {
    const fn channel(color: u8, base: u8, percent: u16) -> u8 {
        let mixed = (color as u16 * percent + base as u16 * (100 - percent) + 50) / 100;
        mixed as u8
    }
    (
        channel(color.0, base.0, percent),
        channel(color.1, base.1, percent),
        channel(color.2, base.2, percent),
    )
}

const BLACK: (u8, u8, u8) = (0, 0, 0);
const WHITE: (u8, u8, u8) = (255, 255, 255);

mod dark {
    pub(super) const SECONDARY: (u8, u8, u8) = (186, 186, 192);
    pub(super) const TERTIARY: (u8, u8, u8) = (148, 148, 153);
    pub(super) const ACCENT: (u8, u8, u8) = (255, 146, 48); // systemOrange, dark
    pub(super) const SUCCESS: (u8, u8, u8) = (48, 209, 88); // systemGreen, dark
    pub(super) const WARNING: (u8, u8, u8) = (255, 214, 0); // systemYellow, dark
    pub(super) const DANGER: (u8, u8, u8) = (255, 97, 101); // systemRed, dark, increased contrast
    pub(super) const INFO: (u8, u8, u8) = (92, 184, 255); // systemBlue, dark, increased contrast
    pub(super) const CYAN: (u8, u8, u8) = (60, 211, 254); // systemCyan, dark
    pub(super) const PURPLE: (u8, u8, u8) = (234, 141, 255); // systemPurple, dark, increased contrast
    pub(super) const INDIGO: (u8, u8, u8) = (167, 170, 255); // systemIndigo, dark, increased contrast
    pub(super) const PINK: (u8, u8, u8) = (255, 138, 196); // systemPink, dark, increased contrast
    pub(super) const FILL: (u8, u8, u8) = (44, 44, 46); // systemGray5, dark
    pub(super) const SEPARATOR: (u8, u8, u8) = (84, 84, 88);
    /// Badge fills are the tone at this opacity over black.
    pub(super) const BADGE_TINT: u16 = 22;
}

mod light {
    pub(super) const SECONDARY: (u8, u8, u8) = (60, 60, 67);
    pub(super) const TERTIARY: (u8, u8, u8) = (100, 100, 106);
    pub(super) const ACCENT: (u8, u8, u8) = (184, 74, 0);
    pub(super) const SUCCESS: (u8, u8, u8) = (0, 128, 45);
    pub(super) const WARNING: (u8, u8, u8) = (150, 98, 0);
    pub(super) const DANGER: (u8, u8, u8) = (215, 0, 40);
    pub(super) const INFO: (u8, u8, u8) = (0, 102, 214);
    pub(super) const CYAN: (u8, u8, u8) = (0, 118, 163);
    pub(super) const PURPLE: (u8, u8, u8) = (176, 47, 194); // systemPurple, light, increased contrast
    pub(super) const INDIGO: (u8, u8, u8) = (86, 74, 222); // systemIndigo, light, increased contrast
    pub(super) const PINK: (u8, u8, u8) = (215, 15, 72);
    pub(super) const FILL: (u8, u8, u8) = (242, 242, 247); // systemGray6, light
    pub(super) const SEPARATOR: (u8, u8, u8) = (198, 198, 204);
    /// Badge fills are the tone at this opacity over white; labels are the tone
    /// darkened to this share of itself, since a light tint costs contrast.
    pub(super) const BADGE_TINT: u16 = 12;
    pub(super) const BADGE_LABEL_SHADE: u16 = 82;
}

const fn dark_badge(color: (u8, u8, u8)) -> (Color, Color) {
    (rgb(color), rgb(tint(color, BLACK, dark::BADGE_TINT)))
}

const fn light_badge(color: (u8, u8, u8)) -> (Color, Color) {
    (rgb(tint(color, BLACK, light::BADGE_LABEL_SHADE)), rgb(tint(color, WHITE, light::BADGE_TINT)))
}

pub const DARK: Palette = Palette {
    label: Color::Reset,
    secondary: rgb(dark::SECONDARY),
    tertiary: rgb(dark::TERTIARY),
    separator: rgb(dark::SEPARATOR),
    accent: rgb(dark::ACCENT),
    on_accent: rgb(BLACK),
    success: rgb(dark::SUCCESS),
    warning: rgb(dark::WARNING),
    danger: rgb(dark::DANGER),
    info: rgb(dark::INFO),
    cyan: rgb(dark::CYAN),
    purple: rgb(dark::PURPLE),
    indigo: rgb(dark::INDIGO),
    pink: rgb(dark::PINK),
    fill: rgb(dark::FILL),
    highlight_fg: rgb(BLACK),
    highlight_bg: rgb(dark::WARNING),
    badges: [
        dark_badge(dark::SECONDARY),
        dark_badge(dark::ACCENT),
        dark_badge(dark::SUCCESS),
        dark_badge(dark::WARNING),
        dark_badge(dark::DANGER),
        dark_badge(dark::INFO),
    ],
};

pub const LIGHT: Palette = Palette {
    label: Color::Reset,
    secondary: rgb(light::SECONDARY),
    tertiary: rgb(light::TERTIARY),
    separator: rgb(light::SEPARATOR),
    accent: rgb(light::ACCENT),
    on_accent: rgb(WHITE),
    success: rgb(light::SUCCESS),
    warning: rgb(light::WARNING),
    danger: rgb(light::DANGER),
    info: rgb(light::INFO),
    cyan: rgb(light::CYAN),
    purple: rgb(light::PURPLE),
    indigo: rgb(light::INDIGO),
    pink: rgb(light::PINK),
    fill: rgb(light::FILL),
    highlight_fg: rgb(BLACK),
    highlight_bg: rgb((255, 204, 0)), // systemYellow, light
    badges: [
        light_badge(light::SECONDARY),
        light_badge(light::ACCENT),
        light_badge(light::SUCCESS),
        light_badge(light::WARNING),
        light_badge(light::DANGER),
        light_badge(light::INFO),
    ],
};

impl Palette {
    /// The (label, fill) pair for a badge of `tone`.
    pub const fn badge(&self, tone: Tone) -> (Color, Color) {
        self.badges[tone as usize]
    }
}

const APPEARANCE_DARK: u8 = 0;
const APPEARANCE_LIGHT: u8 = 1;
static APPEARANCE: AtomicU8 = AtomicU8::new(APPEARANCE_DARK);

/// Chooses the palette for the rest of the process. Set once at startup, before
/// the first draw; until then, and in tests, the dark palette applies.
pub fn set_appearance(appearance: Appearance) {
    let value = match appearance {
        Appearance::Dark => APPEARANCE_DARK,
        Appearance::Light => APPEARANCE_LIGHT,
    };
    APPEARANCE.store(value, Ordering::Relaxed);
}

pub fn appearance() -> Appearance {
    if APPEARANCE.load(Ordering::Relaxed) == APPEARANCE_LIGHT {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

pub fn palette() -> &'static Palette {
    match appearance() {
        Appearance::Dark => &DARK,
        Appearance::Light => &LIGHT,
    }
}

pub fn label() -> Color {
    palette().label
}
pub fn secondary() -> Color {
    palette().secondary
}
pub fn tertiary() -> Color {
    palette().tertiary
}
pub fn separator() -> Color {
    palette().separator
}
pub fn accent() -> Color {
    palette().accent
}
pub fn on_accent() -> Color {
    palette().on_accent
}
pub fn success() -> Color {
    palette().success
}
pub fn warning() -> Color {
    palette().warning
}
pub fn danger() -> Color {
    palette().danger
}
pub fn info() -> Color {
    palette().info
}
pub fn cyan() -> Color {
    palette().cyan
}
pub fn purple() -> Color {
    palette().purple
}
pub fn indigo() -> Color {
    palette().indigo
}
pub fn pink() -> Color {
    palette().pink
}
pub fn fill() -> Color {
    palette().fill
}

/// Role aliases kept so call sites read by purpose.
pub fn role_assistant() -> Color {
    accent()
}
pub fn btw_accent() -> Color {
    cyan()
}
pub fn ultracode_accent() -> Color {
    purple()
}
pub fn slash_command() -> Color {
    purple()
}
pub fn subagent_token() -> Color {
    indigo()
}

/// The text color that carries `tone` outside a badge.
pub fn tone_color(tone: Tone) -> Color {
    let palette = palette();
    match tone {
        Tone::Neutral => palette.tertiary,
        Tone::Accent => palette.accent,
        Tone::Success => palette.success,
        Tone::Warning => palette.warning,
        Tone::Danger => palette.danger,
        Tone::Info => palette.info,
    }
}

/// Text on an accent fill: the selected row of a list, the active secondary tab.
pub fn on_accent_style() -> Style {
    Style::default().fg(on_accent()).bg(accent()).add_modifier(Modifier::BOLD)
}

/// A find match or the rewind picker's selection: black on yellow, as in Apple's
/// own find highlight.
pub fn highlight_style() -> Style {
    let palette = palette();
    Style::default().fg(palette.highlight_fg).bg(palette.highlight_bg).add_modifier(Modifier::BOLD)
}

/// A capsule label for `tone`: tinted fill, readable label, bold.
pub fn badge_style(tone: Tone) -> Style {
    let (fg, bg) = palette().badge(tone);
    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
}

pub const PROMPT_CHAR: &str = "\u{276f}";
pub const SEPARATOR_CHAR: &str = "\u{2500}";

// Tool status icons
pub const ICON_COMPLETED: &str = "\u{2713}";
pub const ICON_DETACHED: &str = "\u{2197}";
pub const ICON_FAILED: &str = "\u{2717}";

/// SDK tool icon + label pair. Monochrome Unicode symbols.
/// Unknown tool names fall back to a generic Tool label.
pub fn tool_name_label(sdk_tool_name: &str) -> (&'static str, &'static str) {
    match sdk_tool_name {
        "Read" => ("\u{2b1a}", "Read"),
        "Write" => ("\u{25a3}", "Write"),
        "Edit" => ("\u{25a3}", "Edit"),
        "MultiEdit" => ("\u{25a3}", "MultiEdit"),
        "NotebookEdit" => ("\u{25a3}", "NotebookEdit"),
        "Delete" => ("\u{25a3}", "Delete"),
        "Move" => ("\u{21c4}", "Move"),
        "Glob" => ("\u{2315}", "Glob"),
        "Grep" => ("\u{2315}", "Grep"),
        "LS" => ("\u{2315}", "LS"),
        "Bash" => ("$ \u{27e9}", "Bash"),
        "PowerShell" => ("PS \u{27e9}", "PowerShell"),
        "Task" | "Agent" => ("\u{25c7}", "Subagent"),
        "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList" | "TaskOutput" | "TaskStop" => {
            ("\u{25a1}", "Task")
        }
        "WebFetch" => ("\u{2295}", "WebFetch"),
        "WebSearch" => ("\u{2295}", "WebSearch"),
        "EnterPlanMode" => ("\u{2299}", "EnterPlanMode"),
        "ExitPlanMode" => ("\u{2299}", "ExitPlanMode"),
        "Config" => ("\u{2299}", "Config"),
        "CronCreate" | "CronDelete" | "CronList" => ("\u{25f7}", "Cron"),
        "ScheduleWakeup" => ("\u{25f7}", "Wakeup"),
        "PushNotification" => ("!", "Notify"),
        "RemoteTrigger" => ("\u{21c4}", "Remote"),
        "REPL" => ("\u{03bb}", "REPL"),
        "Monitor" => ("\u{25c9}", "Monitor"),
        "Workflow" => ("\u{25c7}", "Workflow"),
        "Projects" => ("\u{25a6}", "Projects"),
        "Artifact" => ("\u{25c8}", "Artifact"),
        "ShowOnboardingRolePicker" => ("\u{2299}", "Role"),
        "Skill" => ("\u{2726}", "Skill"),
        "EnterWorktree" => ("\u{21c4}", "EnterWorktree"),
        "ExitWorktree" => ("\u{21c4}", "ExitWorktree"),
        _ => ("\u{25cb}", "Tool"),
    }
}

#[cfg(test)]
mod tests {
    use super::{DARK, LIGHT, Palette, Tone, tool_name_label};
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    /// WCAG 2.x relative luminance of an sRGB color.
    pub(crate) fn luminance(color: Color) -> f64 {
        let Color::Rgb(red, green, blue) = color else {
            return f64::NAN;
        };
        let channel = |value: u8| {
            let normalized = f64::from(value) / 255.0;
            if normalized <= 0.040_45 {
                normalized / 12.92
            } else {
                ((normalized + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    pub(crate) fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// The text floor from Apple's accessibility guidance (WCAG AA, body text).
    const TEXT_FLOOR: f64 = 4.5;

    /// Backgrounds a dark palette must read on: pure black, a typical dark theme
    /// (#1e1e1e), and the app's own raised fill. Light likewise.
    fn surfaces(palette: &Palette) -> Vec<(&'static str, Color)> {
        if *palette == DARK {
            vec![
                ("black", Color::Rgb(0, 0, 0)),
                ("#1e1e1e", Color::Rgb(30, 30, 30)),
                ("fill", palette.fill),
            ]
        } else {
            vec![
                ("white", Color::Rgb(255, 255, 255)),
                ("#fafafa", Color::Rgb(250, 250, 250)),
                ("fill", palette.fill),
            ]
        }
    }

    fn text_roles(palette: &Palette) -> Vec<(&'static str, Color)> {
        vec![
            ("secondary", palette.secondary),
            ("tertiary", palette.tertiary),
            ("accent", palette.accent),
            ("success", palette.success),
            ("warning", palette.warning),
            ("danger", palette.danger),
            ("info", palette.info),
            ("cyan", palette.cyan),
            ("purple", palette.purple),
            ("indigo", palette.indigo),
            ("pink", palette.pink),
        ]
    }

    #[test]
    fn every_text_role_meets_the_contrast_floor() {
        for palette in [&DARK, &LIGHT] {
            for (surface_name, surface) in surfaces(palette) {
                for (role, color) in text_roles(palette) {
                    let ratio = contrast(color, surface);
                    assert!(
                        ratio >= TEXT_FLOOR,
                        "{:?} {role} on {surface_name}: {ratio:.2}",
                        palette.accent
                    );
                }
            }
        }
    }

    #[test]
    fn fills_carry_readable_labels() {
        for palette in [&DARK, &LIGHT] {
            let pairs = [
                ("on_accent", palette.on_accent, palette.accent),
                ("highlight", palette.highlight_fg, palette.highlight_bg),
            ];
            let badges = [
                Tone::Neutral,
                Tone::Accent,
                Tone::Success,
                Tone::Warning,
                Tone::Danger,
                Tone::Info,
            ]
            .map(|tone| {
                let (fg, bg) = palette.badge(tone);
                ("badge", fg, bg)
            });
            for (name, fg, bg) in pairs.into_iter().chain(badges) {
                let ratio = contrast(fg, bg);
                assert!(ratio >= TEXT_FLOOR, "{name} {fg:?} on {bg:?}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn badge_fills_follow_the_tone() {
        assert_eq!(DARK.badge(Tone::Success), (Color::Rgb(48, 209, 88), Color::Rgb(11, 46, 19)));
        assert_eq!(LIGHT.badge(Tone::Danger), (Color::Rgb(176, 0, 33), Color::Rgb(250, 224, 229)));
    }

    #[test]
    fn primary_label_defers_to_the_terminal() {
        assert_eq!(DARK.label, Color::Reset);
        assert_eq!(LIGHT.label, Color::Reset);
    }

    #[test]
    fn task_and_agent_share_subagent_label_and_icon() {
        assert_eq!(tool_name_label("Task"), ("\u{25c7}", "Subagent"));
        assert_eq!(tool_name_label("Agent"), ("\u{25c7}", "Subagent"));
    }

    #[test]
    fn sdk_task_state_tools_use_neutral_task_label() {
        for sdk_tool_name in
            ["TaskCreate", "TaskUpdate", "TaskGet", "TaskList", "TaskOutput", "TaskStop"]
        {
            assert_eq!(tool_name_label(sdk_tool_name), ("\u{25a1}", "Task"));
        }
    }

    #[test]
    fn worktree_tools_use_worktree_labels_and_icon() {
        assert_eq!(tool_name_label("EnterWorktree"), ("\u{21c4}", "EnterWorktree"));
        assert_eq!(tool_name_label("ExitWorktree"), ("\u{21c4}", "ExitWorktree"));
    }

    #[test]
    fn cron_tools_use_cron_labels_and_icon() {
        for sdk_tool_name in ["CronCreate", "CronDelete", "CronList"] {
            assert_eq!(tool_name_label(sdk_tool_name), ("\u{25f7}", "Cron"));
        }
    }

    #[test]
    fn schedule_wakeup_tool_uses_wakeup_label_and_icon() {
        assert_eq!(tool_name_label("ScheduleWakeup"), ("\u{25f7}", "Wakeup"));
    }

    #[test]
    fn push_notification_tool_uses_notify_label_and_icon() {
        assert_eq!(tool_name_label("PushNotification"), ("!", "Notify"));
    }

    #[test]
    fn remote_trigger_tool_uses_remote_label_and_icon() {
        assert_eq!(tool_name_label("RemoteTrigger"), ("\u{21c4}", "Remote"));
    }

    #[test]
    fn repl_tool_uses_lambda_label_and_icon() {
        assert_eq!(tool_name_label("REPL"), ("\u{03bb}", "REPL"));
    }

    #[test]
    fn monitor_and_workflow_tools_use_background_task_labels_and_icons() {
        assert_eq!(tool_name_label("Monitor"), ("\u{25c9}", "Monitor"));
        assert_eq!(tool_name_label("Workflow"), ("\u{25c7}", "Workflow"));
    }

    #[test]
    fn project_artifact_and_role_tools_use_specific_labels_and_icons() {
        assert_eq!(tool_name_label("Projects"), ("\u{25a6}", "Projects"));
        assert_eq!(tool_name_label("Artifact"), ("\u{25c8}", "Artifact"));
        assert_eq!(tool_name_label("ShowOnboardingRolePicker"), ("\u{2299}", "Role"));
    }

    #[test]
    fn skill_tool_uses_spark_icon() {
        assert_eq!(tool_name_label("Skill"), ("\u{2726}", "Skill"));
        assert_eq!(UnicodeWidthStr::width("\u{2726}"), 1);
    }

    #[test]
    fn plan_mode_tools_use_plan_mode_labels_and_icon() {
        assert_eq!(tool_name_label("EnterPlanMode"), ("\u{2299}", "EnterPlanMode"));
        assert_eq!(tool_name_label("ExitPlanMode"), ("\u{2299}", "ExitPlanMode"));
    }

    #[test]
    fn unknown_tools_use_generic_fallback() {
        assert_eq!(tool_name_label("UnknownFutureTool"), ("\u{25cb}", "Tool"));
    }

    #[test]
    fn shell_tools_use_distinct_shell_labels_and_icons() {
        assert_eq!(tool_name_label("Bash"), ("$ \u{27e9}", "Bash"));
        assert_eq!(tool_name_label("PowerShell"), ("PS \u{27e9}", "PowerShell"));
    }
}
