// SPDX-License-Identifier: Apache-2.0

use super::chat_terminal::NativeTerminalIo;
use super::panic_hook::restore_once;
use crossterm::cursor::{Hide, SetCursorStyle, Show};
use crossterm::event::{DisableFocusChange, EnableFocusChange};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    DisableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// External operations only; command selection stays in the production dispatcher.
pub(crate) trait ModeIo: Clone + Send + Sync + 'static {
    fn raw_mode(&self, enabled: bool) -> std::io::Result<()>;
    fn command(&self, command: impl crossterm::Command) -> std::io::Result<()>;
}

impl ModeIo for NativeTerminalIo {
    fn raw_mode(&self, enabled: bool) -> std::io::Result<()> {
        if enabled { enable_raw_mode() } else { disable_raw_mode() }
    }
    fn command(&self, command: impl crossterm::Command) -> std::io::Result<()> {
        execute!(std::io::stdout(), command)
    }
}

pub(super) struct TerminalModes<I: ModeIo = NativeTerminalIo> {
    pub(super) io: I,
    alternate_screen: AtomicBool,
    restored: AtomicBool,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    keyboard_enhancement_active: AtomicBool,
}

impl<I: ModeIo> TerminalModes<I> {
    pub(super) fn new(io: I) -> Self {
        Self {
            io,
            alternate_screen: AtomicBool::new(false),
            restored: AtomicBool::new(false),
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            keyboard_enhancement_active: AtomicBool::new(false),
        }
    }

    pub(super) fn is_restored(&self) -> bool {
        self.restored.load(Ordering::SeqCst)
    }

    pub(super) fn exit_fullscreen(&self) -> std::io::Result<()> {
        if self.alternate_screen.load(Ordering::SeqCst) {
            self.apply(exit_fullscreen_actions())
        } else {
            Ok(())
        }
    }

    /// Only successful cleanup closes the ownership gate. Failed I/O remains retryable.
    pub(super) fn restore(&self) -> std::io::Result<bool> {
        let mut outcome = Ok(false);
        restore_once(&self.restored, || {
            let exit = self.exit_fullscreen();
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            let remaining = shutdown_restore_actions()
                .iter()
                .copied()
                .filter(|action| {
                    *action != TerminalModeAction::PopKeyboardEnhancement
                        || self.keyboard_enhancement_active.load(Ordering::SeqCst)
                })
                .collect::<Vec<_>>();
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            let actions = remaining.as_slice();
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            let actions = shutdown_restore_actions();
            let shell = self.apply(actions);
            outcome = exit.and(shell).map(|()| true);
            if outcome.is_err() {
                self.restored.store(false, Ordering::SeqCst);
            }
        });
        outcome
    }

    pub(super) fn apply(&self, actions: &[TerminalModeAction]) -> std::io::Result<()> {
        let mut first_error = None;

        for action in actions {
            let result = match action {
                TerminalModeAction::EnableRawMode => self.io.raw_mode(true),
                TerminalModeAction::DisableRawMode => self.io.raw_mode(false),
                TerminalModeAction::EnterAlternateScreen => {
                    // A failed write may have delivered a prefix; retain cleanup ownership.
                    self.alternate_screen.store(true, Ordering::SeqCst);
                    self.io.command(EnterAlternateScreen)
                }
                TerminalModeAction::LeaveAlternateScreen => {
                    let result = self.io.command(LeaveAlternateScreen);
                    if result.is_ok() {
                        self.alternate_screen.store(false, Ordering::SeqCst);
                    }
                    result
                }
                TerminalModeAction::EnableFocusChange => self.io.command(EnableFocusChange),
                TerminalModeAction::DisableFocusChange => self.io.command(DisableFocusChange),
                TerminalModeAction::HideCursor => self.io.command(Hide),
                TerminalModeAction::ShowCursor => self.io.command(Show),
                TerminalModeAction::SetBlinkingBarCursor => {
                    self.io.command(SetCursorStyle::BlinkingBar)
                }
                TerminalModeAction::RestoreDefaultCursorShape => {
                    self.io.command(SetCursorStyle::DefaultUserShape)
                }
                TerminalModeAction::DisableLineWrap => self.io.command(DisableLineWrap),
                TerminalModeAction::EnableLineWrap => {
                    self.io.command(crossterm::terminal::EnableLineWrap)
                }
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                TerminalModeAction::PushKeyboardEnhancement => {
                    self.keyboard_enhancement_active.store(true, Ordering::SeqCst);
                    self.io.command(PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
                    ))
                }
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                TerminalModeAction::PopKeyboardEnhancement => {
                    let result = self.io.command(PopKeyboardEnhancementFlags);
                    if result.is_ok() {
                        self.keyboard_enhancement_active.store(false, Ordering::SeqCst);
                    }
                    result
                }
            };

            if let Err(err) = result {
                let err = std::io::Error::new(
                    err.kind(),
                    format!("terminal action {action:?} failed: {err}"),
                );
                // Without raw mode there is no terminal to configure, and the remaining
                // actions would only write escape sequences into whatever stdout is
                // (measured: 58 bytes into a pipe).
                if *action == TerminalModeAction::EnableRawMode {
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
}

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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    PushKeyboardEnhancement,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    PopKeyboardEnhancement,
}

const CHAT_STARTUP_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::EnableRawMode,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    TerminalModeAction::PopKeyboardEnhancement,
    TerminalModeAction::DisableRawMode,
];

const RELEASE_TO_CHILD_ACTIONS: &[TerminalModeAction] = &[
    TerminalModeAction::RestoreDefaultCursorShape,
    TerminalModeAction::ShowCursor,
    TerminalModeAction::EnableLineWrap,
    TerminalModeAction::DisableFocusChange,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FailingIo {
        failing: &'static str,
        attempted: Arc<Mutex<Vec<String>>>,
    }

    impl FailingIo {
        fn new(failing: &'static str) -> Self {
            Self { failing, attempted: Arc::default() }
        }

        fn record(&self, operation: &str) -> std::io::Result<()> {
            let mut attempted = self.attempted.lock().expect("attempt log");
            attempted.push(operation.to_owned());
            if operation == self.failing {
                Err(std::io::Error::other("Device not configured"))
            } else {
                Ok(())
            }
        }
    }

    impl ModeIo for FailingIo {
        fn raw_mode(&self, enabled: bool) -> std::io::Result<()> {
            self.record(if enabled { "raw:on" } else { "raw:off" })
        }

        fn command(&self, command: impl crossterm::Command) -> std::io::Result<()> {
            let mut bytes = String::new();
            command.write_ansi(&mut bytes).expect("ANSI form");
            self.record(&bytes)
        }
    }

    #[test]
    fn failed_raw_mode_stops_startup_before_any_escape_sequence() {
        let io = FailingIo::new("raw:on");
        let modes = TerminalModes::new(io.clone());

        let error = modes.apply(chat_startup_actions()).expect_err("raw mode failure");

        assert_eq!(*io.attempted.lock().expect("attempt log"), ["raw:on"]);
        assert_eq!(
            error.to_string(),
            "terminal action EnableRawMode failed: Device not configured"
        );
    }

    #[test]
    fn restore_continues_past_a_failed_action_and_reports_the_first() {
        let io = FailingIo::new("\x1b[?25h");
        let modes = TerminalModes::new(io.clone());

        let error = modes.apply(shutdown_restore_actions()).expect_err("show cursor failure");

        assert_eq!(
            io.attempted.lock().expect("attempt log").len(),
            shutdown_restore_actions().len()
        );
        assert_eq!(error.to_string(), "terminal action ShowCursor failed: Device not configured");
    }
}
