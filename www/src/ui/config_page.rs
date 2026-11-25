//! The configuration reference: every setting, searchable, with its default and environment.

use nsh_base::{text_view, theme};
use nsh_core::prelude::*;

use super::style::{self, Width, width};
use crate::config_ref::{self, GROUPS, Group, Setting};

const EXAMPLE: &str = r#"```toml
# ~/.config/rho/config.toml
thinking_level = "medium"
max_turns = 1000

[models]
default = "anthropic/claude-sonnet-4-5"
smol = "gemini/gemini-2.5-flash"
guard = "ollama/qwen2.5-coder:7b"

[tools.web.search]
default = "brave"
fallback = ["duckduckgo", "yahoo"]

[ui]
block_style = "border"
auto_title = true
```"#;

pub fn page() -> impl IntoElement {
    let query = signal(String::new());
    let shown = computed(move || {
        query.with(|q| {
            GROUPS
                .iter()
                .map(|g| g.settings.iter().filter(|s| s.matches(q)).count())
                .sum::<usize>()
        })
    });
    style::container(style::CONTENT_MAX)
        .py_12()
        .gap_8()
        .child(intro())
        .child(search(query, shown))
        .child(when(
            move || shown.get() == 0,
            || {
                style::card()
                    .items_center()
                    .py_10()
                    .child(text("No settings match.").font_weight(FontWeight::SEMIBOLD))
                    .child(style::muted(
                        "Try a key, a value, or an environment variable such as AI_MAX_TURNS.",
                    ))
            },
        ))
        .children(GROUPS.iter().map(move |group| group_section(group, query)))
}

fn intro() -> impl IntoElement {
    let step = |n: &'static str, title: &'static str, body: &'static str| {
        view()
            .flex_row()
            .gap_3()
            .child(
                view()
                    .w(px(26.0))
                    .h(px(26.0))
                    .rounded_full()
                    .items_center()
                    .justify_center()
                    .bg(move || theme().accent)
                    .child(
                        text(n)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(move || theme().accent_foreground),
                    ),
            )
            .child(
                view()
                    .flex_1()
                    .gap_1()
                    .child(text(title).font_weight(FontWeight::SEMIBOLD))
                    .child(style::muted(body).text_sm()),
            )
    };
    view()
        .gap_6()
        .child(
            view()
                .gap_3()
                .max_w(px(style::PROSE_MAX))
                .child(style::eyebrow("REFERENCE"))
                .child(style::heading("Configuration", 1))
                .child(
                    style::muted(
                        "Set any of these in config.toml, with rho config <key> <value>, or in the /settings modal. \
                         /reload applies file changes without losing the session.",
                    )
                    .text_lg()
                    .line_height(px(28.0)),
                ),
        )
        .child(
            view()
                .gap_6()
                .style_when(move || width() == Width::Wide, |s| s.flex_row())
                .child(
                    style::card()
                        .flex_1()
                        .gap_4()
                        .child(text("Precedence, highest first").font_weight(FontWeight::BOLD))
                        .child(step("1", "Command-line flags", "--model, --thinking, --no-permission and the rest."))
                        .child(step("2", "Environment variables", "AI_*, RHO_* and WEB_*, listed with each setting."))
                        .child(step("3", "Project file", ".rho/config.toml"))
                        .child(step("4", "Global file", "~/.config/rho/config.toml ($RHO_HOME replaces ~/.config/rho)."))
                        .child(
                            style::muted(
                                "permission, providers, allow_private_network, [worktree] and MCP servers are read from the \
                                 global file only, so a cloned repository cannot change them.",
                            )
                            .text_sm()
                            .mt_2(),
                        ),
                )
                .child(view().flex_1().min_w(px(0.0)).child(text_view().markdown(EXAMPLE))),
        )
}

fn search(query: Signal<String>, shown: Memo<usize>) -> impl IntoElement {
    view()
        .gap_2()
        .child(
            nsh_base::input()
                .test_id("settings-search")
                .label("Search settings")
                .placeholder("Search settings: guard, timeout, cursor, AI_MAX_TURNS")
                .value(move || query.get())
                .on_change(move |value: &str| query.set(value.to_owned()))
                .w_full()
                .px_4()
                .py_3()
                .text_base()
                .rounded_lg()
                .border_1()
                .border_color(move || theme().input)
                .bg(move || theme().card)
                .focus(|s| s.border_color(move || theme().ring)),
        )
        .child(style::muted(move || format!("{} of {} settings", shown.get(), config_ref::count())).text_sm())
}

fn group_section(group: &'static Group, query: Signal<String>) -> impl IntoElement {
    let visible = move || query.with(|q| group.settings.iter().any(|s| s.matches(q)));
    when(visible, move || {
        view()
            .gap_4()
            .child(style::heading(group.title, 2))
            .child(group.note.map(|note| style::muted(note).text_sm()))
            .child(view().flex_row().flex_wrap().gap_3().child(each(
                move || {
                    query.with(|q| {
                        group
                            .settings
                            .iter()
                            .filter(|s| s.matches(q))
                            .copied()
                            .collect::<Vec<_>>()
                    })
                },
                |setting: &Setting| setting.key,
                |setting, _| setting_card(setting.get_untracked()),
            )))
    })
}

fn setting_card(setting: Setting) -> impl IntoElement {
    style::card()
        .flex_1()
        .min_w(px(300.0))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    text(setting.key)
                        .font_family(style::mono())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().primary)
                        .selectable(true),
                )
                .child(setting.default.map(|default| {
                    view()
                        .px_2()
                        .rounded_full()
                        .bg(move || theme().muted)
                        .child(style::muted(format!("default {default}")).text_xs())
                })),
        )
        .child(text(setting.about).text_sm().line_height(px(21.0)))
        .child(setting.env.map(|env| {
            style::muted(format!("Also: {env}"))
                .text_xs()
                .font_family(style::mono())
        }))
}
