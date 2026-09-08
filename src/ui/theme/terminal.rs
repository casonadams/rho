//! Terminal foreground/background detection for the block fill.
//!
//! Queries OSC 10/11 once at interactive startup and computes the container
//! fill as the foreground tinted over the reported background, so the fill is
//! visible and self-consistent on any palette. Falls back to the default
//! fill when the terminal is not a TTY or does not answer.

use super::Theme;
use std::io::IsTerminal;
use terminal_colorsaurus::{QueryOptions, color_palette};

/// Fraction of the foreground mixed into the background for the block fill.
const FG_TINT_PERCENT: u32 = 12;

pub fn detect() -> Theme {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Theme::default();
    }
    let Ok(palette) = color_palette(QueryOptions::default()) else {
        return Theme::default();
    };
    let is_light = palette.theme_mode() == terminal_colorsaurus::ThemeMode::Light;
    let fg = anstyle::RgbColor::from(palette.foreground);
    let bg = anstyle::RgbColor::from(palette.background);
    Theme {
        is_light,
        block_fill: blend_fill(fg, bg),
        ..Theme::default()
    }
}

pub(crate) fn blend_fill(fg: anstyle::RgbColor, bg: anstyle::RgbColor) -> anstyle::Style {
    let mix = |fg: u8, bg: u8| -> u8 {
        (i32::from(bg) + (i32::from(fg) - i32::from(bg)) * FG_TINT_PERCENT as i32 / 100) as u8
    };
    let blended = anstyle::RgbColor(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2));
    anstyle::Style::new().bg_color(Some(anstyle::Color::Rgb(blended)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{Color, RgbColor};

    fn fill_rgb(style: anstyle::Style) -> RgbColor {
        match style.get_bg_color() {
            Some(Color::Rgb(rgb)) => rgb,
            other => panic!("expected rgb background, got {other:?}"),
        }
    }

    #[test]
    fn dark_terminal_fill_is_lightened_toward_foreground() {
        // Catppuccin Mocha: dark background, light foreground.
        let fg = RgbColor(0xc0, 0xca, 0xf5);
        let bg = RgbColor(0x1e, 0x1e, 0x2e);
        let fill = fill_rgb(blend_fill(fg, bg));
        let expected = |fg: u8, bg: u8| (i32::from(bg) + (i32::from(fg) - i32::from(bg)) * 12 / 100) as u8;
        assert_eq!(
            fill,
            RgbColor(expected(fg.0, bg.0), expected(fg.1, bg.1), expected(fg.2, bg.2))
        );
        assert!(fill.0 > bg.0 && fill.1 > bg.1 && fill.2 > bg.2);
    }

    #[test]
    fn light_terminal_fill_is_darkened_toward_foreground() {
        let fg = RgbColor(0x33, 0x33, 0x33);
        let bg = RgbColor(0xfd, 0xfd, 0xfd);
        let fill = fill_rgb(blend_fill(fg, bg));
        assert!(fill.0 < bg.0 && fill.1 < bg.1 && fill.2 < bg.2);
    }

    #[test]
    fn fill_never_matches_or_exceeds_the_foreground() {
        let fg = RgbColor(0xff, 0xff, 0xff);
        let bg = RgbColor(0x00, 0x00, 0x00);
        let fill = fill_rgb(blend_fill(fg, bg));
        assert_ne!(fill, fg);
        assert_ne!(fill, bg);
        assert!(fill.0 < fg.0);
    }

    #[test]
    fn equal_colors_stay_put() {
        let same = RgbColor(0x40, 0x40, 0x40);
        assert_eq!(fill_rgb(blend_fill(same, same)), same);
    }
}
