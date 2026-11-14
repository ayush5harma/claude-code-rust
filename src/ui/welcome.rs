// SPDX-License-Identifier: Apache-2.0

use crate::app::{
    App, AppStatus, ChatMessage, MessageBlock, SurfaceMode, TerminalLifecycleState, WelcomeBlock,
};
use crate::ui::host_tips::HOST_TIPS;
use crate::ui::theme;
use crate::ui::wrap::{
    StyledChunk, display_width, join_column_lines, wrap_styled_chunks,
    wrap_styled_chunks_with_hanging_prefix,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::time::{Duration, Instant};

/// One pose of the welcome crab: four rows, each as wide as the rest pose's,
/// so the overview text beside it never shifts. The art is drawn in block
/// elements, two by two sub-pixels a cell, on a 32x8 grid: a solid silhouette
/// with raised claws and square eyes cut out of the shell, which reads at a
/// glance where ASCII punctuation read as noise. Terminals draw these glyphs
/// themselves, so the pixels meet without gaps.
pub(crate) type FerrisFrame = [&'static str; 4];

const CLAWS_OPEN: &str = " █▄█ ▄▄▄▄▄▄ █▄█ ";
const CLAWS_SHUT: &str = " ▟█▙ ▄▄▄▄▄▄ ▟█▙ ";
const FACE: &str = "  ▜▖▟█▄██▄█▙▗▛  ";
const FACE_LEFT: &str = "  ▜▖▟▙▟█▙▟█▙▗▛  ";
const FACE_RIGHT: &str = "  ▜▖▟█▙▟█▙▟▙▗▛  ";
const FACE_BLINK: &str = "  ▜▖▟██████▙▗▛  ";
const SHELL: &str = "   ▀████████▀   ";
const LEGS_A: &str = "    ▞▐ ▌▐ ▌▚    ";
const LEGS_B: &str = "    ▐▞▗▘▝▖▚▌    ";

pub(crate) const FERRIS_REST: FerrisFrame = [CLAWS_OPEN, FACE, SHELL, LEGS_A];
const FERRIS_REST_BLINK: FerrisFrame = [CLAWS_OPEN, FACE_BLINK, SHELL, LEGS_A];
const FERRIS_SNAP: FerrisFrame = [CLAWS_SHUT, FACE, SHELL, LEGS_A];

/// Scuttles in place and looks around while the session connects.
const CONNECTING_FRAMES: [FerrisFrame; 4] = [
    [CLAWS_OPEN, FACE_LEFT, SHELL, LEGS_A],
    [CLAWS_OPEN, FACE, SHELL, LEGS_B],
    [CLAWS_OPEN, FACE_RIGHT, SHELL, LEGS_A],
    [CLAWS_OPEN, FACE, SHELL, LEGS_B],
];
const CONNECTING_FRAME_INTERVAL: Duration = Duration::from_millis(180);

/// Played once, after the first session of the process connects: two claw
/// snaps and a blink. It ends on the rest pose, so the last frame drawn inside
/// the window is already final.
const HELLO_FRAMES: [FerrisFrame; 5] =
    [FERRIS_SNAP, FERRIS_REST, FERRIS_SNAP, FERRIS_REST_BLINK, FERRIS_REST];
const HELLO_BEAT: Duration = Duration::from_millis(240);
const HELLO_WINDOW: Duration = Duration::from_millis(1_200);

/// What the welcome crab is doing at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FerrisPhase {
    Rest,
    Connecting { elapsed: Duration },
    Hello { elapsed: Duration },
}

/// The crab's phase, derived from app state and the clock; nothing is stored.
/// `width` is the width the welcome is rendered at.
pub(crate) fn ferris_phase(
    app: &App,
    welcome: &WelcomeBlock,
    width: u16,
    now: Instant,
) -> FerrisPhase {
    if !motion_allowed(app) {
        return FerrisPhase::Rest;
    }
    // A welcome whose values are loading is never commit-ready, so the loop
    // can never be frozen into scrollback mid-step.
    if app.status == AppStatus::Connecting && !welcome_values_ready(welcome) {
        let elapsed = now.saturating_duration_since(app.startup.timeline.origin());
        return FerrisPhase::Connecting { elapsed };
    }
    hello_beat(app, width, now).map_or(FerrisPhase::Rest, |elapsed| FerrisPhase::Hello { elapsed })
}

/// Whether the frame loop must keep redrawing for the welcome hello: while
/// its window holds the welcome back from scrollback and a chat surface shows
/// it. The draw after the window closes is the one that commits the welcome,
/// so the loop redraws through the window even before the motion preference
/// is known and the hello cannot start yet. The connecting loop needs nothing
/// here: the Connecting status already redraws.
pub(crate) fn ferris_hello_live(app: &App, now: Instant) -> bool {
    let chat_visible = matches!(
        app.terminal_lifecycle,
        TerminalLifecycleState::Running(SurfaceMode::Chat) | TerminalLifecycleState::Bootstrapping
    );
    chat_visible && hello_window(app, app.chat_render.terminal_size().width, now).is_some()
}

/// Whether the welcome rows may be committed to scrollback, where they can
/// never be redrawn. They are held back while the hello window is open, so a
/// committed welcome is always final and always at rest.
pub(crate) fn welcome_commit_ready(
    app: &App,
    welcome: &WelcomeBlock,
    width: u16,
    now: Instant,
) -> bool {
    welcome_values_ready(welcome) && hello_window(app, width, now).is_none()
}

/// The time into the first-connect hello while it holds the welcome back:
/// the crab is visible at `width`, the welcome is the whole transcript (a
/// prompt, a notice or resumed history snaps the crab to rest) and the window
/// since the first connection has not closed. Later connects of the process
/// never reopen it.
fn hello_window(app: &App, width: u16, now: Instant) -> Option<Duration> {
    if !motion_allowed(app)
        || !ferris_column_visible(usize::from(width))
        || !welcome_is_whole_transcript(app)
    {
        return None;
    }
    let elapsed = now.saturating_duration_since(app.startup.timeline.connected_at()?);
    (elapsed < HELLO_WINDOW).then_some(elapsed)
}

/// The time into the hello while the crab plays it. The preference arrives
/// with the settings snapshot, which is requested only after the first
/// connect, so the hello starts partway through its window if the snapshot is
/// late and is skipped if it comes after the window.
fn hello_beat(app: &App, width: u16, now: Instant) -> Option<Duration> {
    hello_window(app, width, now).filter(|_| app.config.snapshot.is_some())
}

/// Whether the transcript holds nothing but the welcome.
pub(crate) fn welcome_is_whole_transcript(app: &App) -> bool {
    matches!(app.transcript.messages.as_slice(), [message] if welcome_block(message).is_some())
}

fn motion_allowed(app: &App) -> bool {
    !app.config.prefers_reduced_motion_effective()
        && !app.shutdown_requested()
        && app.show_session_overview
}

/// The pose for a phase.
pub(crate) fn ferris_frame(phase: FerrisPhase) -> &'static FerrisFrame {
    match phase {
        FerrisPhase::Rest => &FERRIS_REST,
        FerrisPhase::Connecting { elapsed } => {
            let beat = beat_index(elapsed, CONNECTING_FRAME_INTERVAL);
            CONNECTING_FRAMES.get(beat % CONNECTING_FRAMES.len()).unwrap_or(&FERRIS_REST)
        }
        // Past the last beat the hello holds its final pose, which is rest.
        FerrisPhase::Hello { elapsed } => {
            HELLO_FRAMES.get(beat_index(elapsed, HELLO_BEAT)).unwrap_or(&FERRIS_REST)
        }
    }
}

fn beat_index(elapsed: Duration, interval: Duration) -> usize {
    let beats = elapsed.as_millis() / interval.as_millis().max(1);
    usize::try_from(beats).unwrap_or(usize::MAX)
}

pub(crate) fn welcome_block(message: &ChatMessage) -> Option<&WelcomeBlock> {
    message.blocks.iter().find_map(|block| match block {
        MessageBlock::Welcome(welcome) => Some(welcome),
        MessageBlock::Text(_)
        | MessageBlock::BtwExchange(_)
        | MessageBlock::Notice(_)
        | MessageBlock::ToolCall(_)
        | MessageBlock::ToolResult { .. }
        | MessageBlock::ImageAttachment(_)
        | MessageBlock::UserDialog(_) => None,
    })
}

fn welcome_values_ready(welcome: &WelcomeBlock) -> bool {
    !welcome_value_missing(&welcome.subscription) && !welcome_value_missing(&welcome.session_id)
}

const FERRIS_LEFT_PADDING: &str = "  ";
const FERRIS_TEXT_GAP: usize = 2;
const MIN_INLINE_FIELD_VALUE_WIDTH: usize = 8;
const WELCOME_FIELD_LABELS: &[&str] = &["Version", "Subscription", "Cwd", "Session ID", "Tips"];

pub(crate) fn overview_lines(
    block: &WelcomeBlock,
    loading_status: Option<&str>,
    ferris: &FerrisFrame,
    width: u16,
) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 {
        return vec![Line::default()];
    }

    let loading = loading_status.unwrap_or("Loading");
    let subscription_missing = welcome_value_missing(&block.subscription);
    let session_missing = welcome_value_missing(&block.session_id);
    let subscription_value =
        if subscription_missing { loading.to_owned() } else { block.subscription.clone() };
    let session_value = if session_missing { loading.to_owned() } else { block.session_id.clone() };
    let subscription_style = if subscription_missing {
        Style::default().fg(theme::tertiary())
    } else {
        Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD)
    };

    let ferris_width = ferris_column_width();
    let overview_width = width.saturating_sub(ferris_width.saturating_add(FERRIS_TEXT_GAP));
    let side_by_side = ferris_column_visible(width);
    let text_width = if side_by_side { overview_width } else { width };
    let text_rows = overview_text_rows(
        block,
        &subscription_value,
        subscription_style,
        &session_value,
        text_width,
    );
    let mut lines = if side_by_side {
        join_column_lines(ferris_rows(ferris), text_rows, ferris_width, FERRIS_TEXT_GAP)
    } else {
        text_rows
    };
    lines.push(Line::default());
    lines
}

fn overview_text_rows(
    block: &WelcomeBlock,
    subscription_value: &str,
    subscription_style: Style,
    session_value: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let dim = Style::default().fg(theme::tertiary());
    let mut rows = Vec::new();
    rows.extend(welcome_field_lines("Version", &block.version, dim, width));
    rows.extend(welcome_field_lines("Subscription", subscription_value, subscription_style, width));
    rows.extend(welcome_field_lines("Cwd", &block.cwd, dim, width));
    rows.extend(welcome_field_lines("Session ID", session_value, dim, width));
    rows.push(Line::default());
    rows.extend(welcome_field_lines("Tips", selected_tip(block), dim, width));
    rows
}

fn welcome_field_lines(
    label: &str,
    value: &str,
    value_style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let prefix = vec![StyledChunk {
        text: format!("{label}: "),
        style: Style::default().fg(theme::tertiary()),
    }];
    let body = vec![StyledChunk { text: value.to_owned(), style: value_style }];
    let prefix_width = prefix.iter().map(|chunk| display_width(&chunk.text)).sum::<usize>();
    if width.saturating_sub(prefix_width) < MIN_INLINE_FIELD_VALUE_WIDTH {
        let mut rows = wrap_styled_chunks(&prefix, width);
        rows.extend(wrap_styled_chunks(&body, width));
        return rows;
    }

    wrap_styled_chunks_with_hanging_prefix(&prefix, &body, width, Style::default())
}

fn ferris_column_width() -> usize {
    display_width(FERRIS_LEFT_PADDING)
        .saturating_add(FERRIS_REST.iter().map(|line| display_width(line)).max().unwrap_or(0))
}

/// Whether a row `width` columns wide shows the crab beside the overview; a
/// narrower one shows the overview text alone.
fn ferris_column_visible(width: usize) -> bool {
    let overview_offset = ferris_column_width().saturating_add(FERRIS_TEXT_GAP);
    width.saturating_sub(overview_offset) >= minimum_overview_width()
}

fn ferris_rows(ferris: &FerrisFrame) -> Vec<Line<'static>> {
    ferris
        .iter()
        .map(|art| {
            Line::from(Span::styled(
                format!("{FERRIS_LEFT_PADDING}{art}"),
                Style::default().fg(theme::accent()),
            ))
        })
        .collect()
}

fn minimum_overview_width() -> usize {
    WELCOME_FIELD_LABELS
        .iter()
        .map(|label| display_width(&format!("{label}: ")))
        .max()
        .unwrap_or(0)
        .saturating_add(MIN_INLINE_FIELD_VALUE_WIDTH)
}

fn welcome_value_missing(value: &str) -> bool {
    value.trim().is_empty() || value == "-"
}

pub(crate) fn selected_tip(block: &WelcomeBlock) -> &'static str {
    let Some(first_tip) = HOST_TIPS.first().copied() else {
        return "Enter sends, Shift+Enter inserts a newline, and Ctrl+C clears or quits";
    };
    let len_u64 = u64::try_from(HOST_TIPS.len()).unwrap_or(1);
    let idx_u64 = block.tip_seed % len_u64;
    let idx = usize::try_from(idx_u64).unwrap_or(0);
    HOST_TIPS.get(idx).copied().unwrap_or(first_tip)
}

#[cfg(test)]
mod tests {
    use super::{
        CONNECTING_FRAMES, FERRIS_REST, FerrisPhase, HELLO_FRAMES, HOST_TIPS, Line, ferris_frame,
        overview_lines,
    };
    use crate::app::{ChatMessage, MessageBlock};
    use crate::ui::wrap::{display_width, line_display_width};
    use std::time::Duration;

    /// The rest pose, written out so the frame table cannot drift from it.
    const STATIC_CRAB: [&str; 4] =
        [" █▄█ ▄▄▄▄▄▄ █▄█ ", "  ▜▖▟█▄██▄█▙▗▛  ", "   ▀████████▀   ", "    ▞▐ ▌▐ ▌▚    "];

    /// Block elements and spaces only: one column each in every terminal,
    /// and drawn by the terminal itself rather than taken from the font.
    fn is_block_art(row: &str) -> bool {
        row.chars().all(|ch| ch == ' ' || ('\u{2580}'..='\u{259f}').contains(&ch))
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    /// The part of `row` from terminal column `column` on; the crab's block
    /// glyphs are three bytes each, so a byte offset is not a column.
    fn from_column(row: &str, column: usize) -> String {
        let mut used = 0usize;
        for (index, ch) in row.char_indices() {
            if used >= column {
                return row[index..].to_owned();
            }
            used += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
        String::new()
    }

    fn field_region(rows: &[String], label: &str) -> (usize, Vec<String>) {
        let labels = ["Version:", "Subscription:", "Cwd:", "Session ID:", "Tips:"];
        let start = rows.iter().position(|row| row.contains(label)).expect("field label");
        let column = display_width(&rows[start][..rows[start].find(label).unwrap()]);
        let end = rows[start + 1..]
            .iter()
            .position(|row| labels.iter().any(|next| row.contains(next)))
            .map_or(rows.len(), |offset| start + 1 + offset);
        let mut region = vec![from_column(&rows[start], column + label.len())];
        region.extend(rows[start + 1..end].iter().map(|row| from_column(row, column)));
        (column, region)
    }

    fn assert_field_source(region: &[String], expected: &str) {
        let expected = expected.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut remaining = expected.as_str();
        for piece in region.iter().map(|row| row.trim()).filter(|row| !row.is_empty()) {
            remaining = remaining.strip_prefix(' ').unwrap_or(remaining);
            assert!(
                remaining.starts_with(piece),
                "field changed source text: {region:?}, expected {expected:?}"
            );
            remaining = &remaining[piece.len()..];
        }
        assert!(remaining.is_empty(), "field lost source text: {region:?}, missing {remaining:?}");
    }

    #[test]
    fn every_crab_frame_keeps_the_rest_pose_geometry() {
        assert_eq!(FERRIS_REST, STATIC_CRAB);
        for frame in CONNECTING_FRAMES.iter().chain(HELLO_FRAMES.iter()) {
            for (row, rest_row) in frame.iter().zip(STATIC_CRAB) {
                assert!(is_block_art(row), "{row:?}");
                assert_eq!(display_width(row), 16, "{row:?}");
                assert_eq!(display_width(row), display_width(rest_row), "{row:?}");
            }
        }
        assert_eq!(HELLO_FRAMES.last(), Some(&FERRIS_REST), "the hello ends at rest");
    }

    #[test]
    fn frames_follow_elapsed_time() {
        let at = |ms| Duration::from_millis(ms);
        let connecting = |ms| ferris_frame(FerrisPhase::Connecting { elapsed: at(ms) });
        let hello = |ms| ferris_frame(FerrisPhase::Hello { elapsed: at(ms) });

        assert_eq!(connecting(0)[1], "  ▜▖▟▙▟█▙▟█▙▗▛  ", "looking left");
        assert_eq!(connecting(179)[1], "  ▜▖▟▙▟█▙▟█▙▗▛  ");
        assert_eq!(connecting(180)[3], "    ▐▞▗▘▝▖▚▌    ", "legs mid-step");
        assert_eq!(connecting(360)[1], "  ▜▖▟█▙▟█▙▟▙▗▛  ", "looking right");
        assert_eq!(connecting(720), connecting(0));
        assert_eq!(hello(0)[0], " ▟█▙ ▄▄▄▄▄▄ ▟█▙ ", "claws snapped shut");
        assert_eq!(*hello(240), STATIC_CRAB);
        assert_eq!(hello(480)[0], " ▟█▙ ▄▄▄▄▄▄ ▟█▙ ");
        assert_eq!(hello(720)[1], "  ▜▖▟██████▙▗▛  ", "blinking");
        assert_eq!(*hello(960), STATIC_CRAB);
        assert_eq!(*hello(60_000), STATIC_CRAB);
        assert_eq!(*ferris_frame(FerrisPhase::Rest), STATIC_CRAB);
    }

    #[test]
    fn overview_lines_render_expected_fields() {
        let mut message = ChatMessage::welcome(env!("CARGO_PKG_VERSION"), "-", "/cwd", "-");
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };
        block.tip_seed = 16;
        let lines: Vec<String> = overview_lines(block, None, &FERRIS_REST, 120)
            .into_iter()
            .map(|line| line.spans.into_iter().map(|s| s.content).collect())
            .collect();
        assert!(lines.iter().any(|line| line.contains("▀████████▀")));
        assert!(!lines.iter().any(|line| line.contains("Welcome back to Claude, in Rust!")));
        assert!(lines.iter().any(|line| line.contains("Version:")));
        assert!(lines.iter().any(|line| line.contains("Subscription: Loading")));
        assert!(lines.iter().any(|line| line.contains("Cwd: /cwd")));
        assert!(lines.iter().any(|line| line.contains("Session ID: Loading")));
        assert!(lines.iter().any(|line| line.contains("Tips: ")));
        assert!(
            HOST_TIPS.iter().any(|tip| lines.iter().any(|line| line.contains(tip))),
            "expected one welcome tip to be rendered"
        );
    }

    #[test]
    fn narrow_overview_hides_ferris_and_keeps_hanging_indent() {
        let mut message = ChatMessage::welcome("1.2.3", "Pro", "/workspace/demo", "session-123");
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };
        block.tip_seed = 7;

        let lines = overview_lines(block, None, &FERRIS_REST, 36);
        let text = lines.iter().map(line_text).collect::<Vec<_>>();
        let tip_row =
            text.iter().position(|line| line.starts_with("Tips: Start")).expect("tip row");

        assert!(!text.iter().any(|line| line.contains("▀████████▀")));
        assert_eq!(text[tip_row + 1].find(|ch: char| !ch.is_whitespace()), Some(6));
        assert!(lines.iter().all(|line| line_display_width(line) <= 36));
    }

    #[test]
    fn overview_fields_keep_their_own_resolved_or_loading_values() {
        for (subscription, session, cause, expected_subscription, expected_session) in [
            (
                "Enterprise fixture",
                "session-saffron",
                Some("WAIT_PROFILE"),
                "Enterprise fixture",
                "session-saffron",
            ),
            ("-", "session-saffron", Some("WAIT_PROFILE"), "WAIT_PROFILE", "session-saffron"),
            (
                "Enterprise fixture",
                "  ",
                Some("WAIT_SESSION"),
                "Enterprise fixture",
                "WAIT_SESSION",
            ),
            ("", "-", None, "Loading", "Loading"),
        ] {
            let message = ChatMessage::welcome("8.7.6", subscription, "/workspace/cedar", session);
            let MessageBlock::Welcome(block) = &message.blocks[0] else { panic!("welcome block") };
            let rows = overview_lines(block, cause, &FERRIS_REST, 120)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>();
            for (label, expected) in [
                ("Version:", "8.7.6"),
                ("Subscription:", expected_subscription),
                ("Cwd:", "/workspace/cedar"),
                ("Session ID:", expected_session),
            ] {
                let (_, region) = field_region(&rows, label);
                if cause.is_none() && expected == "Loading" {
                    assert!(region.iter().any(|row| row.contains("Loading")), "{label}: {rows:?}");
                } else {
                    assert_field_source(&region, expected);
                }
            }
        }
    }

    #[test]
    fn welcome_fields_preserve_unicode_content_and_relative_wrapping() {
        let cwd =
            "/workspace/雪 cedar/e\u{301}clair wide directory/amber cobalt saffron/final-file";
        let session = "session-orchid-782";
        let mut message = ChatMessage::welcome("8.7.6", "Enterprise fixture", cwd, session);
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else { panic!("welcome block") };
        let (index, tip) = HOST_TIPS.iter().enumerate().max_by_key(|(_, tip)| tip.len()).unwrap();
        block.tip_seed = u64::try_from(index).unwrap();
        for width in [100, 46, 24, 14] {
            let lines = overview_lines(block, None, &FERRIS_REST, width);
            assert!(lines.iter().all(|line| line_display_width(line) <= usize::from(width)));
            let rows = lines.iter().map(line_text).collect::<Vec<_>>();
            for (label, expected) in [("Cwd:", cwd), ("Session ID:", session), ("Tips:", *tip)] {
                let (_, region) = field_region(&rows, label);
                assert_field_source(&region, expected);
                if !region[0].trim().is_empty() {
                    let value_start = label.len() + region[0].len() - region[0].trim_start().len();
                    for continuation in region.iter().skip(1).filter(|row| !row.trim().is_empty()) {
                        let indent = continuation.len() - continuation.trim_start().len();
                        assert_eq!(
                            indent, value_start,
                            "continuation escaped its field: {region:?}"
                        );
                    }
                }
            }
        }
    }
}
