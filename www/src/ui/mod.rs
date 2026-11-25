//! The site's shell: navigation state, header, menu, footer and the page the address names.

mod config_page;
mod docs_page;
mod ecosystem_page;
mod home;
pub mod style;

use nsh_base::{LinkOpenWith, link, theme};
use nsh_core::WebElement;
use nsh_core::desktop::WindowAttributes;
use nsh_core::element::desktop::main_window;
use nsh_core::platform::Appearance;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;
use nsh_core::url_scheme::HistoryBehavior;

use crate::route::{NavSection, Route};
use style::{Width, width};

const GITHUB: &str = "https://github.com/casonadams/rho";
const APPEARANCE_KEY: &str = "rho.appearance";

/// Which page shows, and the heading the address points at.
#[derive(Clone, Copy)]
pub struct Nav {
    pub route: Signal<Route>,
    pub anchor: Signal<Option<String>>,
    menu_open: Signal<bool>,
}

impl Nav {
    fn start() -> Nav {
        let nav = Nav {
            route: signal(Route::Home),
            anchor: signal(None),
            menu_open: signal(false),
        };
        spawn(async move {
            if let Some(url) = app_url().await {
                nav.show(&url);
            }
            loop {
                let change = next_app_url_change().await;
                nav.show(&change.url);
            }
        });
        nav
    }

    fn show(self, url: &str) {
        let (route, anchor) = Route::parse(url);
        self.route.set(route);
        self.anchor.set(anchor);
    }

    /// Shows `route`, at `anchor`, as a new entry of the browser's history.
    pub fn go(self, route: Route, anchor: Option<String>) {
        let href = route.href_at(anchor.as_deref());
        if let Err(error) = set_app_url_with(href, HistoryBehavior::Push) {
            log::warn!("address not updated: {error}");
        }
        self.menu_open.set(false);
        self.route.set(route);
        self.anchor.set(anchor);
    }

    /// A link to a page of the site: a real `<a href>` that the app opens itself.
    pub fn link(self, route: Route) -> nsh_base::LinkElement {
        link()
            .href(route.href())
            .open_with(LinkOpenWith::App)
            .on_open(move |_: &SharedString| self.go(route, None))
            .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
    }
}

pub fn root() -> impl IntoElement {
    restore_appearance();
    let nav = Nav::start();
    effect(move || {
        let mut attributes = WindowAttributes::default();
        attributes.title = Some(page_title(nav.route.get()));
        main_window().set(attributes);
    });
    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(header(nav))
        .child(when(
            move || nav.menu_open.get() && width() == Width::Narrow,
            move || menu(nav),
        ))
        .child(dynamic(move || match nav.route.get() {
            Route::Home => page(nav, home::page(nav)),
            Route::Docs(doc) => docs_page::page(nav, doc).into_element(),
            Route::Config => page(nav, config_page::page()),
            Route::Ecosystem => page(nav, ecosystem_page::page()),
            Route::NotFound => page(nav, not_found(nav)),
        }))
}

fn page_title(route: Route) -> String {
    let page = match route {
        Route::Home => return "rho - a fast, private coding agent CLI".to_owned(),
        Route::Docs(doc) => doc.title(),
        Route::Config => "Configuration reference",
        Route::Ecosystem => "Ecosystem",
        Route::NotFound => "Page not found",
    };
    format!("{page} · rho")
}

/// A page that scrolls as a whole, footer included.
fn page(nav: Nav, content: impl IntoElement) -> AnyElement {
    scroll()
        .flex_1()
        .child(view().w_full().child(content).child(footer(nav)))
        .into_element()
}

fn header(nav: Nav) -> impl IntoElement {
    view()
        .web_element(WebElement::Header)
        .w_full()
        .border_b_1()
        .border_color(move || theme().border)
        .bg(move || theme().background)
        .child(
            style::container(style::CONTENT_MAX)
                .h(px(64.0))
                .flex_row()
                .items_center()
                .gap_2()
                .child(brand(nav))
                .child(view().flex_1())
                .child(when(move || width() != Width::Narrow, move || nav_links(nav)))
                .child(theme_toggle())
                .child(when(move || width() == Width::Narrow, move || menu_button(nav))),
        )
}

fn brand(nav: Nav) -> impl IntoElement {
    nav.link(Route::Home)
        .accessibility_label("rho home")
        .flex_row()
        .items_center()
        .gap_2()
        .rounded_md()
        .child(image(asset!("icon.svg")).w(px(30.0)).h(px(30.0)).rounded_md())
        .child(
            text("rho")
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .letter_spacing(px(-0.5)),
        )
}

fn nav_links(nav: Nav) -> impl IntoElement {
    let links = NavSection::ALL
        .into_iter()
        .filter(|section| *section != NavSection::Home)
        .map(move |section| {
            let current = move || nav.route.get().section() == Some(section);
            nav.link(section.route())
                .current(current)
                .px_3()
                .py_2()
                .rounded_md()
                .hover(|s| s.bg(move || theme().muted))
                .child(
                    text(section.label())
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || {
                            if current() {
                                theme().primary
                            } else {
                                theme().muted_foreground
                            }
                        }),
                )
        });
    view()
        .web_element(WebElement::Nav)
        .flex_row()
        .items_center()
        .gap_1()
        .children(links)
        .child(
            link()
                .href(GITHUB)
                .px_3()
                .py_2()
                .rounded_md()
                .hover(|s| s.bg(move || theme().muted))
                .child(
                    text("GitHub")
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || theme().muted_foreground),
                ),
        )
}

fn icon_button(
    label: impl Fn() -> &'static str + 'static,
    glyph: impl Fn() -> &'static str + 'static,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .accessibility_label(move || Some(SharedString::from(label())))
        .w(px(40.0))
        .h(px(40.0))
        .items_center()
        .justify_center()
        .rounded_md()
        .hover(|s| s.bg(move || theme().muted))
        .focus_visible(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
        .child(text(glyph).text_lg().text_color(move || theme().muted_foreground))
}

fn theme_toggle() -> impl IntoElement {
    icon_button(
        || {
            if dark() {
                "Switch to light theme"
            } else {
                "Switch to dark theme"
            }
        },
        || if dark() { "☀" } else { "☾" },
    )
    .on_press(|| {
        let next = if dark() { Appearance::Light } else { Appearance::Dark };
        set_color_scheme(next);
        let stored = if next == Appearance::Dark { "dark" } else { "light" };
        if Storage::set_item(APPEARANCE_KEY, stored).is_err() {
            log::warn!("theme choice not saved: storage is full");
        }
    })
}

fn restore_appearance() {
    match Storage::get_item(APPEARANCE_KEY).as_deref() {
        Some("dark") => set_color_scheme(Appearance::Dark),
        Some("light") => set_color_scheme(Appearance::Light),
        _ => {}
    }
}

fn menu_button(nav: Nav) -> impl IntoElement {
    icon_button(
        move || if nav.menu_open.get() { "Close menu" } else { "Open menu" },
        move || if nav.menu_open.get() { "✕" } else { "☰" },
    )
    .on_press(move || nav.menu_open.update(|open| *open = !*open))
}

fn menu(nav: Nav) -> impl IntoElement {
    let entries = NavSection::ALL.into_iter().map(move |section| {
        let current = move || nav.route.get().section() == Some(section);
        nav.link(section.route())
            .current(current)
            .px(style::gutter)
            .py_3()
            .hover(|s| s.bg(move || theme().muted))
            .child(
                text(section.label())
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || if current() { theme().primary } else { theme().foreground }),
            )
    });
    view()
        .web_element(WebElement::Nav)
        .w_full()
        .py_2()
        .border_b_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .children(entries)
        .child(
            link()
                .href(GITHUB)
                .px(style::gutter)
                .py_3()
                .child(text("GitHub ↗").text_base().font_weight(FontWeight::MEDIUM)),
        )
}

fn footer(nav: Nav) -> impl IntoElement {
    let column = |title: &'static str| view().gap_2().min_w(px(150.0)).child(style::eyebrow(title));
    let page_link = move |route: Route, label: &'static str| {
        nav.link(route)
            .child(text(label).text_sm().text_color(move || theme().muted_foreground))
    };
    let out = |href: &'static str, label: &'static str| {
        link()
            .href(href)
            .child(text(label).text_sm().text_color(move || theme().muted_foreground))
    };
    view()
        .web_element(WebElement::Footer)
        .w_full()
        .mt_16()
        .py_10()
        .border_t_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .child(
            style::container(style::CONTENT_MAX)
                .gap_8()
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .gap_10()
                        .child(view().gap_2().max_w(px(320.0)).child(brand(nav)).child(
                            style::muted("A fast, private coding agent CLI in Rust. No telemetry, ever.").text_sm(),
                        ))
                        .child(
                            column("PROJECT")
                                .child(page_link(Route::Docs(crate::docs::Doc::Overview), "Documentation"))
                                .child(page_link(Route::Config, "Configuration"))
                                .child(page_link(Route::Ecosystem, "Ecosystem")),
                        )
                        .child(
                            column("ELSEWHERE")
                                .child(out(GITHUB, "GitHub"))
                                .child(out("https://github.com/casonadams/rho/releases", "Releases"))
                                .child(out("https://crates.io/crates/rho", "crates.io"))
                                .child(out("https://modelcontextprotocol.io", "Model Context Protocol")),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .justify_between()
                        .gap_3()
                        .child(style::muted("© 2026 Cason Adams · MIT or Apache-2.0").text_xs())
                        .child(style::muted("π + ρ = pirho 🔥  Built in Rust with Native Shell").text_xs()),
                ),
        )
}

fn not_found(nav: Nav) -> impl IntoElement {
    style::container(style::PROSE_MAX)
        .py_24()
        .gap_4()
        .items_start()
        .child(style::eyebrow("404"))
        .child(style::heading("This page went up in smoke.", 1))
        .child(style::muted(
            "The address names no page of this site. It may have moved when the docs were reorganized.",
        ))
        .child(
            nav.link(Route::Home)
                .mt_2()
                .px_5()
                .py_3()
                .rounded_lg()
                .bg(move || theme().primary)
                .child(
                    text("Back to the home page")
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().primary_foreground),
                ),
        )
}
