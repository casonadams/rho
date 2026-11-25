//! The site's look: breakpoints, widths, colors beyond the theme's, and shared pieces.

use nsh_base::{link, theme};
use nsh_core::prelude::*;

/// How much room the window gives the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Width {
    /// Phones: one column, navigation in a menu.
    Narrow,
    /// Tablets and small windows: navigation inline, one content column.
    Medium,
    /// Desktops: sidebars beside the content.
    Wide,
}

impl Width {
    pub fn of(points: f32) -> Width {
        if points < 720.0 {
            Width::Narrow
        } else if points < 1080.0 {
            Width::Medium
        } else {
            Width::Wide
        }
    }
}

/// The window's [`Width`]; reading it subscribes to resizes.
pub fn width() -> Width {
    Width::of(window().width.0)
}

pub const CONTENT_MAX: f32 = 1160.0;
pub const PROSE_MAX: f32 = 780.0;

pub fn gutter() -> Points {
    match width() {
        Width::Narrow => px(18.0),
        Width::Medium => px(28.0),
        Width::Wide => px(40.0),
    }
}

fn token(name: &str, light: u32, dark: u32) -> AdaptiveColor {
    theme().color(name).unwrap_or_else(|| AdaptiveColor::rgb(light, dark))
}

pub fn terminal_bg() -> AdaptiveColor {
    token("terminal", 0x0c0a09, 0x000000)
}

pub fn terminal_fg() -> AdaptiveColor {
    token("terminal-foreground", 0xe7e5e4, 0xe7e5e4)
}

pub fn terminal_prompt() -> AdaptiveColor {
    token("terminal-prompt", 0xfb923c, 0xfb923c)
}

pub fn ok() -> AdaptiveColor {
    token("ok", 0x15803d, 0x4ade80)
}

pub fn mono() -> SharedString {
    theme()
        .font_mono
        .clone()
        .unwrap_or_else(|| SharedString::from("ui-monospace, Menlo, monospace"))
}

/// A horizontally centered column no wider than `max`, with the page's side gutters.
pub fn container(max: f32) -> View {
    view().w_full().max_w(px(max)).self_center().px(gutter)
}

/// A page heading, a heading for screen readers and search engines too.
pub fn heading(label: impl Into<SharedString>, level: u32) -> Text {
    let size = match level {
        1 => 40.0,
        2 => 28.0,
        _ => 19.0,
    };
    text(label.into())
        .role(AccessibilityRole::Header)
        .accessibility_level(level)
        .font_weight(FontWeight::BOLD)
        .text_size(move || match (level, width()) {
            (1, Width::Narrow) => px(31.0),
            (2, Width::Narrow) => px(23.0),
            _ => px(size),
        })
        .letter_spacing(px(if level == 1 { -0.8 } else { -0.3 }))
        .text_color(move || theme().foreground)
}

pub fn muted(label: impl IntoReactive<SharedString>) -> Text {
    text(label).text_color(move || theme().muted_foreground)
}

/// A small uppercase label above a section's heading.
pub fn eyebrow(label: &'static str) -> Text {
    text(label)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .letter_spacing(px(1.4))
        .text_color(move || theme().primary)
}

/// A bordered surface.
pub fn card() -> View {
    view()
        .p_5()
        .gap_2()
        .rounded_xl()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
}

/// A link styled as a button, opening another site.
pub fn external_button(label: &'static str, href: &'static str, primary: bool) -> impl IntoElement {
    link()
        .href(href)
        .px_5()
        .py_3()
        .rounded_lg()
        .border_1()
        .border_color(move || if primary { theme().primary } else { theme().border })
        .bg(move || if primary { theme().primary } else { theme().card })
        .hover(|s| s.opacity(0.88))
        .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
        .child(text(label).font_weight(FontWeight::SEMIBOLD).text_color(move || {
            if primary {
                theme().primary_foreground
            } else {
                theme().foreground
            }
        }))
}

/// A line of shell input in the terminal's colors, with a copy button.
pub fn command_line(command: &'static str) -> impl IntoElement {
    let copied = signal(false);
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .pl_4()
        .pr_2()
        .py_2()
        .rounded_lg()
        .bg(terminal_bg)
        .border_1()
        .border_color(move || theme().border)
        .child(text("$").font_family(mono()).text_color(terminal_prompt))
        .child(
            text(command)
                .flex_1()
                .font_family(mono())
                .text_color(terminal_fg)
                .selectable(true),
        )
        .child(
            nsh_base::button()
                .accessibility_label(move || {
                    Some(SharedString::from(if copied.get() { "Copied" } else { "Copy command" }))
                })
                .px_3()
                .py_1()
                .rounded_md()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
                .on_press(move || {
                    set_clipboard(command);
                    copied.set(true);
                    spawn(async move {
                        timer(std::time::Duration::from_millis(1600)).await;
                        copied.set(false);
                    });
                })
                .child(
                    text(move || if copied.get() { "Copied" } else { "Copy" })
                        .text_sm()
                        .text_color(move || if copied.get() { ok() } else { terminal_fg() }),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::Width;

    #[test]
    fn widths_split_at_phone_and_desktop_sizes() {
        let cases = [
            (320.0, Width::Narrow),
            (719.0, Width::Narrow),
            (720.0, Width::Medium),
            (1079.0, Width::Medium),
            (1080.0, Width::Wide),
        ];
        for (points, want) in cases {
            assert_eq!(Width::of(points), want, "{points}");
        }
    }
}
