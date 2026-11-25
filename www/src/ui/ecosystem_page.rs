//! The ecosystem: MCP servers and hook recipes, with the config each needs.

use nsh_base::{button, link, text_view, theme};
use nsh_core::prelude::*;

use super::style;
use crate::ecosystem::{EXTENSIONS, Extension, Kind};

pub fn page() -> impl IntoElement {
    let filter = signal(None::<Kind>);
    let shown = move || {
        filter.with(|kind| {
            EXTENSIONS
                .iter()
                .filter(|e| kind.is_none_or(|k| e.kind == k))
                .copied()
                .collect::<Vec<_>>()
        })
    };
    style::container(style::CONTENT_MAX)
        .py_12()
        .gap_8()
        .child(
            view()
                .gap_3()
                .max_w(px(style::PROSE_MAX))
                .child(style::eyebrow("EXTEND RHO"))
                .child(style::heading("Ecosystem", 1))
                .child(
                    style::muted(
                        "Add tools with any Model Context Protocol server, and shape every tool call with one-shot \
                         hooks: scripts in .agents/hooks/ that read an event on stdin and answer with an action.",
                    )
                    .text_lg()
                    .line_height(px(28.0)),
                ),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(chip("All", None, filter))
                .child(chip("MCP servers", Some(Kind::Mcp), filter))
                .child(chip("Hooks", Some(Kind::Hook), filter)),
        )
        .child(view().flex_row().flex_wrap().gap_4().child(each(
            shown,
            |extension: &Extension| extension.name,
            |extension, _| extension_card(extension.get_untracked()),
        )))
}

fn chip(label: &'static str, kind: Option<Kind>, filter: Signal<Option<Kind>>) -> impl IntoElement {
    let selected = move || filter.get() == kind;
    button()
        .selected(selected)
        .px_4()
        .py_2()
        .rounded_full()
        .border_1()
        .border_color(move || if selected() { theme().primary } else { theme().border })
        .bg(move || if selected() { theme().primary } else { theme().card })
        .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
        .on_press(move || filter.set(kind))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if selected() {
                        theme().primary_foreground
                    } else {
                        theme().foreground
                    }
                }),
        )
}

fn language(extension: &Extension) -> &'static str {
    match extension.kind {
        Kind::Mcp => "json",
        Kind::Hook if extension.snippet.starts_with("#!/usr/bin/env python") => "python",
        Kind::Hook => "sh",
    }
}

fn extension_card(extension: Extension) -> impl IntoElement {
    let snippet = format!("```{}\n{}\n```", language(&extension), extension.snippet.trim_end());
    style::card()
        .flex_1()
        .min_w(px(320.0))
        .gap_3()
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(text(extension.name).text_lg().font_weight(FontWeight::SEMIBOLD))
                .child(view().flex_1())
                .child(
                    view().px_2().py_1().rounded_full().bg(move || theme().accent).child(
                        text(extension.kind.label())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || theme().accent_foreground),
                    ),
                ),
        )
        .child(style::muted(format!("{} · {}", extension.category, extension.runtime)).text_xs())
        .child(text(extension.about).text_sm().line_height(px(21.0)))
        .child(
            style::muted(format!("Goes in {}", extension.install_at))
                .text_xs()
                .font_family(style::mono()),
        )
        .child(text_view().markdown(snippet))
        .child(
            link().href(extension.source).self_start().child(
                text("Source ↗")
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || theme().primary),
            ),
        )
}
