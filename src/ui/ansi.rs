// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

//! The terminal-output filter: one linear pass over untrusted bytes.
//!
//! Visible text and SGR styling (`ESC [ ... m`) survive. Every other control sequence is
//! dropped: other CSI sequences (cursor movement, erase, mode changes), OSC (titles, OSC 8
//! hyperlinks), DCS/SOS/PM/APC strings, two-byte escapes, and stray C0/C1 control
//! characters, so program output can never move the cursor, retitle the window or plant a
//! link. [`strip`] and [`styled_lines`] share the scanner, so the two cannot disagree about
//! what is a control sequence.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
/// An SGR sequence longer than this is not interpreted. Real sequences are far shorter; the
/// cap keeps a hostile one from costing more than one pass over its own bytes.
const MAX_SGR_BYTES: usize = 128;
/// Parameters read from one SGR sequence; the rest are ignored.
const MAX_SGR_PARAMS: usize = 32;

enum Event<'a> {
    Text(&'a str),
    /// The parameter bytes of a well-formed `ESC [ <digits ; :> m` sequence.
    Sgr(&'a str),
}

#[derive(Clone, Copy)]
enum State {
    Text,
    Escape,
    /// `ESC` followed by intermediate bytes (`ESC ( B`): the next final byte ends it.
    EscapeIntermediate,
    Csi {
        params_start: usize,
        clean: bool,
    },
    /// OSC, DCS, SOS, PM or APC string, ended by BEL or `ESC \`.
    Str,
    StrEscape,
}

/// Control characters that never reach the screen. Newline and tab are layout, not control.
fn is_dropped_control(ch: char) -> bool {
    ch.is_control() && ch != '\n' && ch != '\t'
}

fn scan(text: &str, mut on_event: impl FnMut(Event<'_>)) {
    let mut run_start = 0;
    let mut state = State::Text;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        state = match state {
            State::Text => {
                if ch == ESC || is_dropped_control(ch) {
                    if run_start < idx {
                        on_event(Event::Text(&text[run_start..idx]));
                    }
                    run_start = next;
                    match ch {
                        ESC => State::Escape,
                        // Measured 2026-10-13: C1 `CSI 2 J` left `2J` visible in the
                        // transcript. Consume its payload, but keep `clean` false because
                        // only the ESC-[ SGR form is allowed to set terminal styles.
                        '\u{9b}' => State::Csi { params_start: next, clean: false },
                        '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => State::Str,
                        _ => State::Text,
                    }
                } else {
                    State::Text
                }
            }
            State::Escape => match ch {
                ESC => State::Escape,
                '[' => State::Csi { params_start: next, clean: true },
                ']' | 'P' | 'X' | '^' | '_' => State::Str,
                ' '..='/' => State::EscapeIntermediate,
                _ => {
                    run_start = next;
                    State::Text
                }
            },
            State::EscapeIntermediate => {
                if matches!(ch, ' '..='/') {
                    State::EscapeIntermediate
                } else {
                    run_start = next;
                    State::Text
                }
            }
            State::Csi { params_start, clean } => match ch {
                '\u{40}'..='\u{7e}' => {
                    if ch == 'm' && clean && idx - params_start <= MAX_SGR_BYTES {
                        on_event(Event::Sgr(&text[params_start..idx]));
                    }
                    run_start = next;
                    State::Text
                }
                '0'..='9' | ';' | ':' => State::Csi { params_start, clean },
                _ => State::Csi { params_start, clean: false },
            },
            State::Str => match ch {
                BEL | '\u{9c}' => {
                    run_start = next;
                    State::Text
                }
                ESC => State::StrEscape,
                _ => State::Str,
            },
            State::StrEscape => {
                if ch == '\\' || ch == '\u{9c}' {
                    run_start = next;
                    State::Text
                } else {
                    State::Str
                }
            }
        };
    }
    if matches!(state, State::Text) && run_start < text.len() {
        on_event(Event::Text(&text[run_start..]));
    }
}

/// `text` without any escape sequence or control character (newline and tab stay).
pub(crate) fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    scan(text, |event| {
        if let Event::Text(run) = event {
            out.push_str(run);
        }
    });
    out
}

/// One line per `\n`, with the SGR styling in effect applied to each run. The style carries
/// across lines, as on a terminal. Cells with no SGR attribute keep `Style::default()`, so a
/// caller can patch its own base style underneath.
pub(crate) fn styled_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut sgr = SgrState::default();
    scan(text, |event| match event {
        Event::Sgr(params) => sgr.apply(params),
        Event::Text(run) => {
            for (piece_index, piece) in run.split('\n').enumerate() {
                if piece_index > 0 {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                push_run(&mut spans, piece, sgr.style());
            }
        }
    });
    lines.push(Line::from(spans));
    lines
}

fn push_run(spans: &mut Vec<Span<'static>>, run: &str, style: Style) {
    if run.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(run);
        return;
    }
    spans.push(Span::styled(run.to_owned(), style));
}

#[derive(Default)]
struct SgrState {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

impl SgrState {
    fn style(&self) -> Style {
        Style { fg: self.fg, bg: self.bg, add_modifier: self.modifiers, ..Style::default() }
    }

    /// Apply one SGR sequence's parameters. Only reset, bold, dim, italic, underline, their
    /// "off" codes, the 16 standard and bright colours, 256-colour and truecolour are
    /// understood; any other parameter is skipped and a malformed extended colour ends the
    /// sequence, because the parameters after it can no longer be told apart.
    fn apply(&mut self, params: &str) {
        if params.is_empty() {
            *self = Self::default();
            return;
        }
        let mut params = params.split(';').take(MAX_SGR_PARAMS);
        while let Some(param) = params.next() {
            let Some(code) = parse_param(param) else { continue };
            match code {
                0 => *self = Self::default(),
                1 => self.modifiers.insert(Modifier::BOLD),
                2 => self.modifiers.insert(Modifier::DIM),
                3 => self.modifiers.insert(Modifier::ITALIC),
                4 => self.modifiers.insert(Modifier::UNDERLINED),
                22 => self.modifiers.remove(Modifier::BOLD | Modifier::DIM),
                23 => self.modifiers.remove(Modifier::ITALIC),
                24 => self.modifiers.remove(Modifier::UNDERLINED),
                30..=37 => self.fg = Some(palette_color(code - 30)),
                90..=97 => self.fg = Some(palette_color(code - 90 + 8)),
                39 => self.fg = None,
                40..=47 => self.bg = Some(palette_color(code - 40)),
                100..=107 => self.bg = Some(palette_color(code - 100 + 8)),
                49 => self.bg = None,
                38 | 48 => {
                    let Some(color) = extended_color(&mut params) else { return };
                    if code == 38 {
                        self.fg = Some(color);
                    } else {
                        self.bg = Some(color);
                    }
                }
                _ => {}
            }
        }
    }
}

/// An empty parameter means 0. A sub-parameter form (`4:3`) or a number that does not fit is
/// not one of the supported codes.
fn parse_param(param: &str) -> Option<u16> {
    if param.is_empty() { Some(0) } else { param.parse().ok() }
}

/// The 16 ANSI colours by index: 0-7 standard, 8-15 bright.
fn palette_color(index: u16) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        _ => Color::White,
    }
}

/// The colour after `38` or `48`: `5;n` or `2;r;g;b`, every number 0-255.
fn extended_color<'a>(params: &mut impl Iterator<Item = &'a str>) -> Option<Color> {
    let mut byte = || params.next().and_then(parse_param).and_then(|n| u8::try_from(n).ok());
    match byte()? {
        5 => Some(Color::Indexed(byte()?)),
        2 => Some(Color::Rgb(byte()?, byte()?, byte()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Cell = (String, Option<Color>, Option<Color>, Modifier);

    /// (text, fg, bg, modifiers) of every span.
    fn cells(text: &str) -> Vec<Vec<Cell>> {
        styled_lines(text)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| {
                        (
                            span.content.into_owned(),
                            span.style.fg,
                            span.style.bg,
                            span.style.add_modifier,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn run(text: &str, fg: Option<Color>, bg: Option<Color>, modifiers: Modifier) -> Cell {
        (text.to_owned(), fg, bg, modifiers)
    }

    #[test]
    fn sgr_attributes_and_colours_map_to_styles() {
        let input = "\u{1b}[1mb\u{1b}[22m \u{1b}[2md\u{1b}[22m \u{1b}[3mi\u{1b}[23m \u{1b}[4mu\u{1b}[24m \
                     \u{1b}[31mr\u{1b}[92mg\u{1b}[39m \u{1b}[44mB\u{1b}[105mM\u{1b}[49m \
                     \u{1b}[38;5;208mo\u{1b}[48;2;1;2;3mT\u{1b}[0m plain";
        let none = Modifier::empty();
        assert_eq!(
            cells(input),
            vec![vec![
                run("b", None, None, Modifier::BOLD),
                run(" ", None, None, none),
                run("d", None, None, Modifier::DIM),
                run(" ", None, None, none),
                run("i", None, None, Modifier::ITALIC),
                run(" ", None, None, none),
                run("u", None, None, Modifier::UNDERLINED),
                run(" ", None, None, none),
                run("r", Some(Color::Red), None, none),
                run("g", Some(Color::LightGreen), None, none),
                run(" ", None, None, none),
                run("B", None, Some(Color::Blue), none),
                run("M", None, Some(Color::LightMagenta), none),
                run(" ", None, None, none),
                run("o", Some(Color::Indexed(208)), None, none),
                run("T", Some(Color::Indexed(208)), Some(Color::Rgb(1, 2, 3)), none),
                run(" plain", None, None, none),
            ]]
        );
    }

    #[test]
    fn style_carries_across_lines_and_an_empty_sgr_resets() {
        let input = "\u{1b}[31mone\ntwo\u{1b}[mthree\n";
        assert_eq!(
            cells(input),
            vec![
                vec![run("one", Some(Color::Red), None, Modifier::empty())],
                vec![
                    run("two", Some(Color::Red), None, Modifier::empty()),
                    run("three", None, None, Modifier::empty()),
                ],
                vec![],
            ]
        );
    }

    #[test]
    fn unknown_and_malformed_sgr_parameters_are_dropped() {
        let none = Modifier::empty();
        // 7 (reverse), 9 (strike), 5 (blink), 53 and a sub-parameter form are not supported;
        // the supported parameter next to them still applies.
        assert_eq!(
            cells("\u{1b}[7;31;9;5;53;4:3mX"),
            vec![vec![run("X", Some(Color::Red), None, none)]]
        );
        // Malformed extended colours (missing or out-of-range numbers) change nothing and end
        // the sequence, so the 1 after them is not applied either.
        for sequence in ["38;5m", "38;5;256m", "38;2;1;2m", "38;2;1;2;300m", "48;9;1m", "38m"] {
            let input = format!("\u{1b}[{sequence}X\u{1b}[38;5;999;1mY");
            assert_eq!(cells(&input), vec![vec![run("XY", None, None, none)]], "{sequence}");
        }
        // Numbers that do not fit are not codes.
        assert_eq!(cells("\u{1b}[99999999;1mX"), vec![vec![run("X", None, None, Modifier::BOLD)]]);
    }

    #[test]
    fn sequences_that_are_not_plain_sgr_never_style_or_print() {
        // Private markers, intermediate bytes and other final bytes make a CSI sequence
        // something other than SGR, whatever numbers it carries.
        for sequence in [
            "\u{1b}[?25m",
            "\u{1b}[>1m",
            "\u{1b}[31 m",
            "\u{1b}[1;31H",
            "\u{1b}[2J",
            "\u{1b}[3A",
            "\u{1b}[?1049h",
            "\u{9b}31m",
        ] {
            let rendered = cells(&format!("a{sequence}b"));
            assert_eq!(
                rendered,
                vec![vec![run("ab", None, None, Modifier::empty())]],
                "{sequence:?}"
            );
        }
    }

    #[test]
    fn overlong_sgr_is_not_interpreted() {
        let long = format!("\u{1b}[{}mX", "1;".repeat(MAX_SGR_BYTES));
        assert_eq!(cells(&long), vec![vec![run("X", None, None, Modifier::empty())]]);
        // Parameters past the cap are ignored, so a colour placed after them is too.
        let many = format!("\u{1b}[{}31mX", "0;".repeat(MAX_SGR_PARAMS));
        assert_eq!(cells(&many), vec![vec![run("X", None, None, Modifier::empty())]]);
    }

    #[test]
    fn strip_removes_every_control_sequence_and_stray_control_character() {
        // (input, what stays visible between the brackets). An OSC 8 hyperlink loses its URL
        // but not its label. C1 control strings use the same scanner states as their ESC forms.
        let hostile = [
            ("\u{1b}]8;;https://evil.example\u{1b}\\link\u{1b}]8;;\u{1b}\\", "link"),
            ("\u{1b}]0;retitled\u{7}", ""),
            ("\u{1b}]52;c;Zm9v\u{7}", ""),
            ("\u{1b}Pq#0;2;0;0;0\u{1b}\\", ""),
            ("\u{1b}_Gi=1;AAAA\u{1b}\\", ""),
            ("\u{1b}[2J\u{1b}[H\u{1b}[3A\u{1b}[?1049h", ""),
            ("\u{1b}(B\u{1b}#8\u{1b}c\u{1b}7\u{1b}=", ""),
            ("\u{9b}31m\u{9d}0;t\u{9c}", ""),
            ("\u{7}\u{8}\r\u{c}\u{b}\u{0}\u{7f}", ""),
        ];
        for (input, visible) in hostile {
            assert_eq!(strip(&format!("[{input}]")), format!("[{visible}]"), "{input:?}");
        }
        assert_eq!(strip("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn an_unterminated_string_swallows_only_what_follows_it() {
        assert_eq!(strip("before\u{1b}]0;never closed"), "before");
        assert_eq!(strip("ESC at end\u{1b}"), "ESC at end");
    }

    #[test]
    fn strip_and_styled_lines_show_the_same_text() {
        let input = "\u{1b}[31mred\u{1b}[0m \u{1b}]0;t\u{7}x\u{1b}[2Ky\nz\u{1b}[1";
        let styled: Vec<String> = styled_lines(input)
            .into_iter()
            .map(|line| line.spans.into_iter().map(|span| span.content.into_owned()).collect())
            .collect();
        assert_eq!(styled.join("\n"), strip(input));
    }
}
