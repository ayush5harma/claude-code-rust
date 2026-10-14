// SPDX-License-Identifier: Apache-2.0

use ratatui::style::Color;

const PALETTE: [Color; 8] = [
    Color::Rgb(245, 115, 120),
    Color::Rgb(246, 160, 90),
    Color::Rgb(220, 195, 85),
    Color::Rgb(105, 205, 145),
    Color::Rgb(90, 200, 215),
    Color::Rgb(120, 175, 245),
    Color::Rgb(190, 155, 240),
    Color::Rgb(235, 145, 190),
];

pub(crate) fn for_session_id(session_id: &str) -> Color {
    let hash = session_id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let index = usize::try_from(hash % 8).unwrap_or_default();
    PALETTE[index]
}

pub(crate) fn named(name: &str, session_id: &str) -> Option<Color> {
    match name {
        "red" => Some(PALETTE[0]),
        "orange" => Some(PALETTE[1]),
        "yellow" => Some(PALETTE[2]),
        "green" => Some(PALETTE[3]),
        "cyan" => Some(PALETTE[4]),
        "blue" => Some(PALETTE[5]),
        "purple" => Some(PALETTE[6]),
        "pink" => Some(PALETTE[7]),
        "default" => Some(for_session_id(session_id)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn named_colors_and_default_are_readable_on_the_composer_surface() {
        assert_eq!(named("blue", "session-1"), Some(Color::Rgb(120, 175, 245)));
        assert_eq!(named("default", "session-1"), Some(for_session_id("session-1")));
        assert_eq!(named("BLUE", "session-1"), None);

        let background = luminance(crate::ui::theme::fill());
        for color in PALETTE {
            let ratio = (luminance(color) + 0.05) / (background + 0.05);
            assert!(ratio >= 4.5, "{color:?} has contrast ratio {ratio:.2}");
        }
    }

    #[test]
    fn session_ids_have_a_stable_palette_choice() {
        assert_eq!(for_session_id("session-1"), for_session_id("session-1"));
        assert_ne!(for_session_id("session-1"), for_session_id("session-2"));
    }
}
