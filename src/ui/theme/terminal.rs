//! Terminal foreground/background detection for block fills and dimmed text.
//!
//! Preference order: a `COLORFGBG` mode announcement (the ncurses/rxvt
//! convention) styles rho through the terminal's base-16 palette, which
//! theme switchers keep correct; otherwise an OSC 10/11 query derives the
//! container fill and dimmed-foreground blend from the reported colors.
//! Falls back to the default theme's ANSI black fill and SGR 2 dim when the
//! terminal is not a TTY or does not answer.

use super::Theme;
use anstyle::Style;
use std::io::IsTerminal;
use terminal_colorsaurus::{QueryOptions, color_palette};

/// Fraction of the foreground mixed into the background for the block fill.
const FG_TINT_PERCENT: u32 = 12;
/// Fraction of the way the foreground is washed toward the background for
/// dimmed text. A given wash reads heavier on light backgrounds, so light
/// palettes get a stronger wash.
const DIM_TINT_PERCENT_DARK: u32 = 40;
const DIM_TINT_PERCENT_LIGHT: u32 = 50;

pub fn detect() -> Theme {
    if let Ok(value) = std::env::var("COLORFGBG")
        && let Some(theme) = theme_from_colorfbg(&value)
    {
        return theme;
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Theme::default();
    }
    let Ok(palette) = color_palette(QueryOptions::default()) else {
        return Theme::default();
    };
    let is_light = palette.theme_mode() == terminal_colorsaurus::ThemeMode::Light;
    let fg = anstyle::RgbColor::from(palette.foreground);
    let bg = anstyle::RgbColor::from(palette.background);
    let dim = dimmed_foreground(fg, bg, is_light);
    Theme {
        is_light,
        dimmed: dim,
        thinking: dim,
        heading_h3: dim,
        block_fill: blend_fill(fg, bg),
        ..Theme::default()
    }
}

/// When the shell announces the palette mode, style through the terminal's
/// base-16 palette: bright-black (slot 8) for dim text and black (slot 0)
/// for the block fill -- the same slots vim and tmux use. The colors resolve
/// live from the active palette, sidestepping OSC queries whose answers
/// terminal multiplexers replace with their own tracked state.
pub(crate) fn theme_from_colorfbg(value: &str) -> Option<Theme> {
    let bg = value.rsplit(';').next()?.trim().parse::<u8>().ok()?;
    let dim = Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack)));
    Some(Theme {
        is_light: bg >= 7,
        dimmed: dim,
        thinking: dim,
        heading_h3: dim,
        block_fill: Style::new().bg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Black))),
        ..Theme::default()
    })
}

fn mix(a: u8, b: u8, percent: u32) -> u8 {
    (i32::from(a) + (i32::from(b) - i32::from(a)) * percent as i32 / 100) as u8
}

pub(crate) fn blend_fill(fg: anstyle::RgbColor, bg: anstyle::RgbColor) -> anstyle::Style {
    let blended = anstyle::RgbColor(
        mix(bg.0, fg.0, FG_TINT_PERCENT),
        mix(bg.1, fg.1, FG_TINT_PERCENT),
        mix(bg.2, fg.2, FG_TINT_PERCENT),
    );
    anstyle::Style::new().bg_color(Some(anstyle::Color::Rgb(blended)))
}

/// Secondary-text color: the foreground washed toward the background, so
/// dimmed text is muted on dark palettes and lightened (never darkened) on
/// light ones.
pub(crate) fn dimmed_foreground(fg: anstyle::RgbColor, bg: anstyle::RgbColor, is_light: bool) -> anstyle::Style {
    let tint = if is_light {
        DIM_TINT_PERCENT_LIGHT
    } else {
        DIM_TINT_PERCENT_DARK
    };
    let dimmed = anstyle::RgbColor(mix(fg.0, bg.0, tint), mix(fg.1, bg.1, tint), mix(fg.2, bg.2, tint));
    anstyle::Style::new().fg_color(Some(anstyle::Color::Rgb(dimmed)))
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

    fn dim_rgb(style: anstyle::Style) -> RgbColor {
        match style.get_fg_color() {
            Some(Color::Rgb(rgb)) => rgb,
            other => panic!("expected rgb foreground, got {other:?}"),
        }
    }

    #[test]
    fn dark_terminal_fill_is_lightened_toward_foreground() {
        // Catppuccin Mocha: dark background, light foreground.
        let fg = RgbColor(0xc0, 0xca, 0xf5);
        let bg = RgbColor(0x1e, 0x1e, 0x2e);
        let fill = fill_rgb(blend_fill(fg, bg));
        assert_eq!(
            fill,
            RgbColor(mix(bg.0, fg.0, 12), mix(bg.1, fg.1, 12), mix(bg.2, fg.2, 12))
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
        assert_eq!(dim_rgb(dimmed_foreground(same, same, false)), same);
        assert_eq!(dim_rgb(dimmed_foreground(same, same, true)), same);
    }

    #[test]
    fn dark_terminal_dim_is_washed_out_toward_background() {
        // Catppuccin Mocha fg/bg: dim lands at ~4.3:1 contrast.
        let fg = RgbColor(0xcd, 0xd6, 0xf4);
        let bg = RgbColor(0x1e, 0x1e, 0x2e);
        let dim = dim_rgb(dimmed_foreground(fg, bg, false));
        assert_eq!(dim, RgbColor(0x87, 0x8d, 0xa5));
        assert!(
            dim.0 < fg.0 && dim.1 < fg.1 && dim.2 < fg.2,
            "dim must be muted on dark palettes"
        );
    }

    #[test]
    fn light_terminal_dim_is_lighter_than_the_foreground() {
        // walh-shell monokai-light: near-white background, dark charcoal text;
        // the stronger 50% light-mode wash lands at ~2.6:1 (matching walh's
        // historical dim gray) instead of SGR 2's near-black.
        let fg = RgbColor(0x40, 0x3e, 0x41);
        let bg = RgbColor(0xf9, 0xf8, 0xf5);
        let dim = dim_rgb(dimmed_foreground(fg, bg, true));
        assert_eq!(dim, RgbColor(0x9c, 0x9b, 0x9b));
        assert!(
            dim.0 > fg.0 && dim.1 > fg.1 && dim.2 > fg.2,
            "dim must wash out, not darken, on light palettes"
        );
        assert!(dim.0 < bg.0, "dim must stay distinguishable from the background");
    }
}
