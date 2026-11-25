//! The home page: what rho is, how to install it, and where to go next.

use nsh_base::{button, theme};
use nsh_core::prelude::*;

use super::Nav;
use super::style::{self, Width, width};
use crate::docs::{self, Doc};
use crate::route::Route;

const SCREENS: [(&str, ImageAsset, &str); 4] = [
    (
        "Start",
        asset!("screens/welcome.svg"),
        "Instructions, skills and MCP servers load on start, with live context and quota gauges.",
    ),
    (
        "Chat",
        asset!("screens/chat.svg"),
        "Native tools edit, search and run your code, with every change shown inline.",
    ),
    (
        "Models",
        asset!("screens/modal-model.svg"),
        "Pick any model from 13+ providers; live model lists need no rho update.",
    ),
    (
        "Login",
        asset!("screens/modal-login.svg"),
        "Sign in with a subscription (Claude, ChatGPT, Gemini) or an API key.",
    ),
];

struct Feature {
    title: &'static str,
    body: &'static str,
    doc: Doc,
}

const FEATURES: [Feature; 6] = [
    Feature {
        title: "Guarded shell",
        body: "A local guard model clears safe commands at once and stops destructive ones for you to allow, edit or deny.",
        doc: Doc::Permissions,
    },
    Feature {
        title: "Subagents in worktrees",
        body: "Scouts, critics, planners and your own agents work in parallel, each in its own git worktree, merged when you say so.",
        doc: Doc::Configuration,
    },
    Feature {
        title: "MCP and hooks",
        body: "stdio and HTTP MCP servers with OAuth, plus one-shot lifecycle hooks that gate, rewrite or stop tool calls.",
        doc: Doc::McpAndHooks,
    },
    Feature {
        title: "Workflows",
        body: "Multi-agent pipelines in a few lines of XML: loops, reviews and verification runs, watched live.",
        doc: Doc::Workflows,
    },
    Feature {
        title: "Any provider",
        body: "Claude, ChatGPT, Gemini, OpenRouter, Ollama and OpenAI-compatible endpoints, several accounts each.",
        doc: Doc::Configuration,
    },
    Feature {
        title: "A terminal UI that keeps up",
        body: "Streaming output, stable scrollback, live cost and context gauges, and your terminal's own colors.",
        doc: Doc::Ui,
    },
];

pub fn page(nav: Nav) -> impl IntoElement {
    view()
        .w_full()
        .child(hero(nav))
        .child(stats())
        .child(features(nav))
        .child(tools(nav))
        .child(privacy(nav))
        .child(pirho())
}

fn hero(nav: Nav) -> impl IntoElement {
    let side_by_side = move || width() == Width::Wide;
    view()
        .w_full()
        .bg(move || theme().accent.opacity(0.35))
        .border_b_1()
        .border_color(move || theme().border)
        .child(
            style::container(style::CONTENT_MAX)
                .py(move || if width() == Width::Narrow { px(40.0) } else { px(72.0) })
                .gap_10()
                .style_when(side_by_side, |s| s.flex_row())
                .items_center()
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_5()
                        .child(style::eyebrow("OPEN SOURCE · RUST · ZERO TELEMETRY"))
                        .child(style::heading("The coding agent that stays on your machine.", 1))
                        .child(
                            style::muted(
                                "rho reads, edits and runs your code with 15 native tools, guards every shell command, \
                                 and sends nothing anywhere but the model provider you choose.",
                            )
                            .text_lg()
                            .line_height(px(28.0)),
                        )
                        .child(style::command_line("cargo install rho"))
                        .child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .gap_3()
                                .child(
                                    nav.link(Route::Docs(Doc::Overview))
                                        .px_5()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(move || theme().primary)
                                        .hover(|s| s.opacity(0.88))
                                        .child(
                                            text("Read the docs")
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(move || theme().primary_foreground),
                                        ),
                                )
                                .child(style::external_button("View on GitHub", super::GITHUB, false)),
                        ),
                )
                .child(
                    view()
                        .w_full()
                        .style_when(side_by_side, |s| s.w(px(560.0)))
                        .child(screens()),
                ),
        )
}

fn screens() -> impl IntoElement {
    let shown = signal(0_usize);
    let tabs = SCREENS.iter().enumerate().map(move |(index, (label, _, _))| {
        let selected = move || shown.get() == index;
        button()
            .accessibility_label(Some(SharedString::from(format!("Show the {label} screen"))))
            .selected(selected)
            .px_3()
            .py_1()
            .rounded_full()
            .border_1()
            .border_color(move || if selected() { theme().primary } else { theme().border })
            .bg(move || if selected() { theme().primary } else { theme().card })
            .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
            .on_press(move || shown.set(index))
            .child(
                text(*label)
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
    });
    view()
        .gap_3()
        .child(
            view()
                .w_full()
                .aspect_ratio(900.0 / 655.0)
                .rounded_xl()
                .overflow_hidden()
                .bg(style::terminal_bg)
                .border_1()
                .border_color(move || theme().border)
                .shadow_xl()
                .justify_center()
                .child(dynamic(move || {
                    let (label, screen, _) = SCREENS[shown.get()];
                    image(screen)
                        .w_full()
                        .aspect_ratio(screen.size.width.0 / screen.size.height.0)
                        .label(format!("rho {label} screen"))
                })),
        )
        .child(view().flex_row().flex_wrap().gap_2().children(tabs))
        .child(style::muted(move || SCREENS[shown.get()].2).text_sm())
}

fn stats() -> impl IntoElement {
    let stat = |value: &'static str, label: &'static str| {
        view()
            .flex_1()
            .min_w(px(140.0))
            .gap_1()
            .child(
                text(value)
                    .text_3xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(move || theme().primary),
            )
            .child(style::muted(label).text_sm())
    };
    style::container(style::CONTENT_MAX)
        .py_10()
        .flex_row()
        .flex_wrap()
        .gap_6()
        .child(stat("15", "native tools, no plugins needed"))
        .child(stat("13+", "model providers"))
        .child(stat("0", "telemetry, analytics or phone-home"))
        .child(stat("1", "binary: cargo install rho"))
}

fn section_intro(eyebrow: &'static str, title: &'static str, lead: &'static str) -> View {
    view()
        .gap_3()
        .max_w(px(style::PROSE_MAX))
        .child(style::eyebrow(eyebrow))
        .child(style::heading(title, 2))
        .child(style::muted(lead).text_base().line_height(px(26.0)))
}

fn features(nav: Nav) -> impl IntoElement {
    let cards = FEATURES.iter().map(move |feature| {
        nav.link(Route::Docs(feature.doc))
            .flex_1()
            .min_w(px(260.0))
            .rounded_xl()
            .hover(|s| s.border_color(move || theme().primary))
            .child(
                style::card()
                    .h_full()
                    .child(text(feature.title).text_lg().font_weight(FontWeight::SEMIBOLD))
                    .child(style::muted(feature.body).text_sm().line_height(px(21.0)))
                    .child(
                        text(format!("{} →", feature.doc.title()))
                            .text_sm()
                            .mt_2()
                            .text_color(move || theme().primary),
                    ),
            )
    });
    style::container(style::CONTENT_MAX)
        .py_12()
        .gap_8()
        .child(section_intro(
            "WHAT IT DOES",
            "Everything an agent needs, nothing it does not.",
            "One Rust binary with the tools, safety and orchestration built in. Extend it with MCP servers, \
             hooks, skills and agents of your own.",
        ))
        .child(view().flex_row().flex_wrap().gap_4().children(cards))
}

fn tools(nav: Nav) -> impl IntoElement {
    let cards = docs::tools().into_iter().map(|tool| {
        view()
            .flex_1()
            .min_w(px(240.0))
            .p_4()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(move || theme().border)
            .bg(move || theme().card)
            .child(
                text(tool.name)
                    .font_family(style::mono())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().primary),
            )
            .child(style::muted(tool.summary).text_sm().line_height(px(20.0)))
    });
    style::container(style::CONTENT_MAX)
        .py_12()
        .gap_8()
        .child(section_intro(
            "BUILT-IN TOOLS",
            "Fifteen native tools, tuned for agents.",
            "Paged, gitignore-aware search, undoable edits, language-server queries, subagents and \
             background work, all in-process.",
        ))
        .child(view().flex_row().flex_wrap().gap_3().children(cards))
        .child(
            nav.link(Route::Docs(Doc::Overview)).self_start().child(
                text("Every tool in detail →")
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().primary),
            ),
        )
}

fn privacy(nav: Nav) -> impl IntoElement {
    let point = |title: &'static str, body: &'static str| {
        view()
            .flex_1()
            .min_w(px(220.0))
            .gap_2()
            .child(text(title).font_weight(FontWeight::SEMIBOLD))
            .child(style::muted(body).text_sm().line_height(px(21.0)))
    };
    view().w_full().my_8().bg(style::terminal_bg).child(
        style::container(style::CONTENT_MAX)
            .py_16()
            .gap_8()
            .text_color(style::terminal_fg)
            .child(style::eyebrow("PRIVACY"))
            .child(
                text("Your code goes to your model provider. Nowhere else.")
                    .role(AccessibilityRole::Header)
                    .accessibility_level(2)
                    .text_3xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(style::terminal_fg),
            )
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_8()
                    .child(point(
                        "Zero telemetry",
                        "No analytics, crash reporters or tracking. rho collects nothing.",
                    ))
                    .child(point(
                        "Direct to provider",
                        "Requests go straight from your machine to the provider you configure.",
                    ))
                    .child(point(
                        "Local storage",
                        "Credentials, sessions and caches stay in ~/.config/rho and your project.",
                    ))
                    .child(point(
                        "Auditable",
                        "Open source under MIT or Apache-2.0, with no tracking dependencies.",
                    )),
            )
            .child(
                nav.link(Route::Docs(Doc::Permissions)).self_start().child(
                    text("How permissions and the guard work →")
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(style::terminal_prompt),
                ),
            ),
    )
}

fn pirho() -> impl IntoElement {
    style::container(style::CONTENT_MAX).py_12().child(
        style::card()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_6()
            .p_8()
            .child(image(asset!("icon.svg")).w(px(72.0)).h(px(72.0)).rounded_xl())
            .child(
                view()
                    .flex_1()
                    .min_w(px(240.0))
                    .gap_2()
                    .child(text("π + ρ = pirho 🔥").text_2xl().font_weight(FontWeight::BOLD))
                    .child(
                        style::muted(
                            "rho grew out of pi, the minimal agent harness, and kept its taste for small, sharp tools. \
                             For developers who like to play with fire, with a guard model watching the matches.",
                        )
                        .line_height(px(24.0)),
                    ),
            )
            .child(style::external_button("Meet pi", "https://pi.dev", false)),
    )
}
