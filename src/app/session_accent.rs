// SPDX-License-Identifier: Apache-2.0

use crate::ui::theme::{self, Palette};
use ratatui::style::Color;

/// The eight session colors, in the order `named` lists them: Apple's system
/// hues as the active palette draws them, so each reads as text on the terminal
/// background and on the composer fill.
fn session_colors(palette: &Palette) -> [Color; 8] {
    [
        palette.danger,
        palette.accent,
        palette.warning,
        palette.success,
        palette.cyan,
        palette.info,
        palette.purple,
        palette.pink,
    ]
}

pub(crate) fn for_session_id(session_id: &str) -> Color {
    let hash = session_id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let index = usize::try_from(hash % 8).unwrap_or_default();
    session_colors(theme::palette())[index]
}

pub(crate) fn named(name: &str, session_id: &str) -> Option<Color> {
    let colors = session_colors(theme::palette());
    match name {
        "red" => Some(colors[0]),
        "orange" => Some(colors[1]),
        "yellow" => Some(colors[2]),
        "green" => Some(colors[3]),
        "cyan" => Some(colors[4]),
        "blue" => Some(colors[5]),
        "purple" => Some(colors[6]),
        "pink" => Some(colors[7]),
        "default" => Some(for_session_id(session_id)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{DARK, LIGHT};

    fn luminance(color: Color) -> f64 {
        let Color::Rgb(red, green, blue) = color else {
            panic!("palette contains a non-RGB color");
        };
        let channel = |value: u8| {
            let normalized = f64::from(value) / 255.0;
            if normalized <= 0.04045 {
                normalized / 12.92
            } else {
                ((normalized + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn named_colors_and_default_are_readable_on_the_composer_surface() {
        assert_eq!(named("blue", "session-1"), Some(DARK.info));
        assert_eq!(named("default", "session-1"), Some(for_session_id("session-1")));
        assert_eq!(named("BLUE", "session-1"), None);

        for palette in [&DARK, &LIGHT] {
            for color in session_colors(palette) {
                let ratio = contrast(color, palette.fill);
                assert!(ratio >= 4.5, "{color:?} has contrast ratio {ratio:.2}");
            }
        }
    }

    #[test]
    fn session_ids_have_a_stable_palette_choice() {
        assert_eq!(for_session_id("session-1"), for_session_id("session-1"));
        assert_ne!(for_session_id("session-1"), for_session_id("session-2"));
    }
}
