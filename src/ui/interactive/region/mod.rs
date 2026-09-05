#[cfg(test)]
mod tests;

use crate::ui::block::ANSI_PATTERN;
use crate::ui::block::visible_width;
use crate::ui::block::wrap::sgr_resets_background;
use crate::ui::theme::Theme;
use anstyle::Style;

/// SGR style that paints rho's own region with the theme's colors, or `None`
/// for ANSI themes, which leave the terminal's native colors untouched.
pub(crate) fn region_style(theme: &Theme) -> Option<Style> {
    let mut style = Style::new();
    if let Some(fg) = theme.foreground {
        style = style.fg_color(Some(fg));
    }
    if let Some(bg) = theme.background {
        style = style.bg_color(Some(bg));
    }
    (theme.foreground.is_some() || theme.background.is_some()).then_some(style)
}

/// SGR that erases cells with the theme background (Background Color Erase),
/// or empty for ANSI themes.
pub(crate) fn bg_code(theme: &Theme) -> String {
    theme
        .background
        .map(|color| Style::new().bg_color(Some(color)).render().to_string())
        .unwrap_or_default()
}

pub(crate) fn paint_lines(lines: &[String], theme: &Theme, width: usize) -> Vec<String> {
    let Some(style) = region_style(theme) else {
        return lines.to_vec();
    };
    let width = width.max(1);
    lines.iter().map(|line| paint_line(line, style, width)).collect()
}

pub(crate) fn paint_region(text: &str, theme: &Theme, width: usize) -> String {
    let Some(style) = region_style(theme) else {
        return text.to_string();
    };
    let width = width.max(1);
    match text.rfind('\n') {
        Some(pos) => {
            let (complete, open) = (&text[..pos], &text[pos + 1..]);
            let mut painted = String::new();
            if !complete.is_empty() {
                painted.push_str(
                    &complete
                        .split('\n')
                        .map(|line| paint_line(line, style, width))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            painted.push('\n');
            painted.push_str(&paint_open_line(open, style));
            painted
        }
        None => paint_open_line(text, style),
    }
}

/// Colors an in-progress streamed line without padding or a final reset, so
/// subsequent tokens continue on the same row inside the active colors.
fn paint_open_line(line: &str, style: Style) -> String {
    if line.is_empty() || line.starts_with("\x1b]") {
        return line.to_string();
    }
    let style_code = style.render().to_string();
    let mut painted = String::with_capacity(line.len() + style_code.len() * 2);
    painted.push_str(&style_code);
    let mut last = 0;
    for sequence in ANSI_PATTERN.find_iter(line) {
        painted.push_str(&line[last..sequence.end()]);
        if sgr_resets_background(sequence.as_str()) {
            painted.push_str(&style_code);
        }
        last = sequence.end();
    }
    painted.push_str(&line[last..]);
    if style.get_bg_color().is_none() {
        painted.push_str("\x1b[0m");
    }
    painted
}

fn paint_line(line: &str, style: Style, width: usize) -> String {
    if line.starts_with("\x1b]") {
        return line.to_string();
    }
    let style_code = style.render().to_string();
    let mut painted = String::with_capacity(line.len() + style_code.len() * 2 + 8);
    painted.push_str(&style_code);
    let mut last = 0;
    for sequence in ANSI_PATTERN.find_iter(line) {
        painted.push_str(&line[last..sequence.end()]);
        if sgr_resets_background(sequence.as_str()) {
            painted.push_str(&style_code);
        }
        last = sequence.end();
    }
    painted.push_str(&line[last..]);
    if style.get_bg_color().is_some() {
        let trailing = width.saturating_sub(visible_width(line));
        painted.push_str(&style_code);
        painted.push_str(&" ".repeat(trailing));
    }
    painted.push_str("\x1b[0m");
    painted
}
