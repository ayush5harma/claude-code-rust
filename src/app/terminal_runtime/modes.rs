// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use crossterm::cursor::{Hide, SetCursorStyle, Show};
use crossterm::event::{DisableFocusChange, EnableFocusChange};
#[cfg(target_os = "macos")]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    DisableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::io::Stdout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalModeAction {
    EnableRawMode,
    DisableRawMode,
    EnterAlternateScreen,
    LeaveAlternateScreen,
    EnableFocusChange,
    DisableFocusChange,
    HideCursor,
    ShowCursor,
    SetBlinkingBarCursor,
    RestoreDefaultCursorShape,
    DisableLineWrap,
    EnableLineWrap,
    #[cfg(target_os = "macos")]
    PushKeyboardEnhancement,
    #[cfg(target_os = "macos")]
    PopKeyboardEnhancement,
}

const CHAT_STARTUP_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::EnableRawMode,
    #[cfg(target_os = "macos")]
    TerminalModeAction::PushKeyboardEnhancement,
    TerminalModeAction::EnableFocusChange,
    TerminalModeAction::DisableLineWrap,
    TerminalModeAction::SetBlinkingBarCursor,
    TerminalModeAction::ShowCursor,
];

const ENTER_FULLSCREEN_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::RestoreDefaultCursorShape,
    TerminalModeAction::HideCursor,
    TerminalModeAction::EnableLineWrap,
    TerminalModeAction::EnterAlternateScreen,
];

const EXIT_FULLSCREEN_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::LeaveAlternateScreen,
    TerminalModeAction::DisableLineWrap,
    TerminalModeAction::SetBlinkingBarCursor,
    TerminalModeAction::ShowCursor,
];

const SHUTDOWN_RESTORE_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::RestoreDefaultCursorShape,
    TerminalModeAction::ShowCursor,
    TerminalModeAction::EnableLineWrap,
    TerminalModeAction::DisableFocusChange,
    #[cfg(target_os = "macos")]
    TerminalModeAction::PopKeyboardEnhancement,
    TerminalModeAction::DisableRawMode,
];

const RELEASE_TO_CHILD_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::RestoreDefaultCursorShape,
    TerminalModeAction::ShowCursor,
    TerminalModeAction::EnableLineWrap,
    TerminalModeAction::DisableFocusChange,
    #[cfg(target_os = "macos")]
    TerminalModeAction::PopKeyboardEnhancement,
    TerminalModeAction::DisableRawMode,
];

const RETURN_FROM_CHILD_ACTIONS: &[TerminalModeAction] = CHAT_STARTUP_ACTIONS;

pub(super) fn chat_startup_actions() -> &'static [TerminalModeAction] {
    CHAT_STARTUP_ACTIONS
}

pub(super) fn enter_fullscreen_actions() -> &'static [TerminalModeAction] {
    ENTER_FULLSCREEN_ACTIONS
}

pub(super) fn exit_fullscreen_actions() -> &'static [TerminalModeAction] {
    EXIT_FULLSCREEN_ACTIONS
}

pub(super) fn shutdown_restore_actions() -> &'static [TerminalModeAction] {
    SHUTDOWN_RESTORE_ACTIONS
}

pub(super) fn release_to_child_actions() -> &'static [TerminalModeAction] {
    RELEASE_TO_CHILD_ACTIONS
}

pub(super) fn return_from_child_actions() -> &'static [TerminalModeAction] {
    RETURN_FROM_CHILD_ACTIONS
}

pub(super) fn apply_actions(
    stdout: &mut Stdout,
    actions: &[TerminalModeAction],
) -> std::io::Result<()> {
    apply_actions_with(actions, |action| apply_action(stdout, action))
}

/// Runs every action and reports the first failure, so a restore sequence still undoes
/// as much as it can. A failed `EnableRawMode` stops the sequence instead: without raw
/// mode there is no terminal to configure, and the remaining actions would only write
/// escape sequences into whatever stdout is (measured: 58 bytes into a pipe).
fn apply_actions_with(
    actions: &[TerminalModeAction],
    mut apply: impl FnMut(TerminalModeAction) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut first_error = None;

    for &action in actions {
        if let Err(err) = apply(action) {
            let err = std::io::Error::new(
                err.kind(),
                format!("terminal action {action:?} failed: {err}"),
            );
            if action == TerminalModeAction::EnableRawMode {
                return Err(err);
            }
            first_error.get_or_insert(err);
        }
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn apply_action(stdout: &mut Stdout, action: TerminalModeAction) -> std::io::Result<()> {
    match action {
        TerminalModeAction::EnableRawMode => enable_raw_mode(),
        TerminalModeAction::DisableRawMode => disable_raw_mode(),
        TerminalModeAction::EnterAlternateScreen => execute!(stdout, EnterAlternateScreen),
        TerminalModeAction::LeaveAlternateScreen => execute!(stdout, LeaveAlternateScreen),
        TerminalModeAction::EnableFocusChange => execute!(stdout, EnableFocusChange),
        TerminalModeAction::DisableFocusChange => execute!(stdout, DisableFocusChange),
        TerminalModeAction::HideCursor => execute!(stdout, Hide),
        TerminalModeAction::ShowCursor => execute!(stdout, Show),
        TerminalModeAction::SetBlinkingBarCursor => {
            execute!(stdout, SetCursorStyle::BlinkingBar)
        }
        TerminalModeAction::RestoreDefaultCursorShape => {
            execute!(stdout, SetCursorStyle::DefaultUserShape)
        }
        TerminalModeAction::DisableLineWrap => execute!(stdout, DisableLineWrap),
        TerminalModeAction::EnableLineWrap => {
            execute!(stdout, crossterm::terminal::EnableLineWrap)
        }
        #[cfg(target_os = "macos")]
        TerminalModeAction::PushKeyboardEnhancement => execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        ),
        #[cfg(target_os = "macos")]
        TerminalModeAction::PopKeyboardEnhancement => {
            execute!(stdout, PopKeyboardEnhancementFlags)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_startup_actions_match_expected_order() {
        assert_eq!(
            chat_startup_actions(),
            &[
                TerminalModeAction::EnableRawMode,
                #[cfg(target_os = "macos")]
                TerminalModeAction::PushKeyboardEnhancement,
                TerminalModeAction::EnableFocusChange,
                TerminalModeAction::DisableLineWrap,
                TerminalModeAction::SetBlinkingBarCursor,
                TerminalModeAction::ShowCursor,
            ]
        );
    }

    #[test]
    fn enter_fullscreen_actions_match_expected_order() {
        assert_eq!(
            enter_fullscreen_actions(),
            &[
                TerminalModeAction::RestoreDefaultCursorShape,
                TerminalModeAction::HideCursor,
                TerminalModeAction::EnableLineWrap,
                TerminalModeAction::EnterAlternateScreen,
            ]
        );
    }

    #[test]
    fn exit_fullscreen_actions_match_expected_order() {
        assert_eq!(
            exit_fullscreen_actions(),
            &[
                TerminalModeAction::LeaveAlternateScreen,
                TerminalModeAction::DisableLineWrap,
                TerminalModeAction::SetBlinkingBarCursor,
                TerminalModeAction::ShowCursor,
            ]
        );
    }

    #[test]
    fn shutdown_restore_actions_match_expected_order() {
        assert_eq!(
            shutdown_restore_actions(),
            &[
                TerminalModeAction::RestoreDefaultCursorShape,
                TerminalModeAction::ShowCursor,
                TerminalModeAction::EnableLineWrap,
                TerminalModeAction::DisableFocusChange,
                #[cfg(target_os = "macos")]
                TerminalModeAction::PopKeyboardEnhancement,
                TerminalModeAction::DisableRawMode,
            ]
        );
    }

    #[test]
    fn release_to_child_actions_match_expected_order() {
        assert_eq!(
            release_to_child_actions(),
            &[
                TerminalModeAction::RestoreDefaultCursorShape,
                TerminalModeAction::ShowCursor,
                TerminalModeAction::EnableLineWrap,
                TerminalModeAction::DisableFocusChange,
                #[cfg(target_os = "macos")]
                TerminalModeAction::PopKeyboardEnhancement,
                TerminalModeAction::DisableRawMode,
            ]
        );
    }

    #[test]
    fn return_from_child_actions_match_chat_startup() {
        assert_eq!(return_from_child_actions(), chat_startup_actions());
    }

    #[test]
    fn chat_startup_actions_disable_line_wrap_for_inline_chat() {
        assert!(
            chat_startup_actions().contains(&TerminalModeAction::DisableLineWrap),
            "inline chat must explicitly disable terminal line wrapping",
        );
    }

    #[test]
    fn chat_startup_actions_do_not_enter_alternate_screen() {
        assert!(!chat_startup_actions().contains(&TerminalModeAction::EnterAlternateScreen));
    }

    #[test]
    fn shutdown_restore_actions_restore_shell_friendly_defaults() {
        assert!(
            shutdown_restore_actions().contains(&TerminalModeAction::RestoreDefaultCursorShape)
        );
        assert!(shutdown_restore_actions().contains(&TerminalModeAction::ShowCursor));
        assert!(shutdown_restore_actions().contains(&TerminalModeAction::EnableLineWrap));
        #[cfg(target_os = "macos")]
        assert!(shutdown_restore_actions().contains(&TerminalModeAction::PopKeyboardEnhancement));
    }

    #[test]
    fn chat_cursor_shape_actions_are_restored_on_surface_changes() {
        assert!(chat_startup_actions().contains(&TerminalModeAction::SetBlinkingBarCursor));
        assert!(return_from_child_actions().contains(&TerminalModeAction::SetBlinkingBarCursor));
        assert!(exit_fullscreen_actions().contains(&TerminalModeAction::SetBlinkingBarCursor));
        assert!(
            enter_fullscreen_actions().contains(&TerminalModeAction::RestoreDefaultCursorShape)
        );
        assert!(
            release_to_child_actions().contains(&TerminalModeAction::RestoreDefaultCursorShape)
        );
        assert!(
            shutdown_restore_actions().contains(&TerminalModeAction::RestoreDefaultCursorShape)
        );
    }

    fn failing_on(
        failing: TerminalModeAction,
        attempted: &mut Vec<TerminalModeAction>,
    ) -> impl FnMut(TerminalModeAction) -> std::io::Result<()> + '_ {
        move |action| {
            attempted.push(action);
            if action == failing {
                Err(std::io::Error::other("Device not configured"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn failed_raw_mode_stops_startup_before_any_escape_sequence() {
        let mut attempted = Vec::new();

        let error = apply_actions_with(
            chat_startup_actions(),
            failing_on(TerminalModeAction::EnableRawMode, &mut attempted),
        )
        .expect_err("raw mode failure");

        assert_eq!(attempted, [TerminalModeAction::EnableRawMode]);
        assert_eq!(
            error.to_string(),
            "terminal action EnableRawMode failed: Device not configured"
        );
    }

    #[test]
    fn restore_continues_past_a_failed_action_and_reports_the_first() {
        let mut attempted = Vec::new();

        let error = apply_actions_with(
            shutdown_restore_actions(),
            failing_on(TerminalModeAction::ShowCursor, &mut attempted),
        )
        .expect_err("show cursor failure");

        assert_eq!(attempted, shutdown_restore_actions());
        assert_eq!(error.to_string(), "terminal action ShowCursor failed: Device not configured");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn chat_startup_actions_enable_keyboard_enhancement() {
        assert!(chat_startup_actions().contains(&TerminalModeAction::PushKeyboardEnhancement));
    }
}
