//! A documentation page: the Markdown of one doc, with the list of docs beside it and, on wide
//! screens, an outline of its sections.

use std::rc::Rc;

use nsh_base::{TextViewLinkPressEvent, text_view, theme};
use nsh_core::WebElement;
use nsh_core::prelude::*;

use super::Nav;
use super::style::{self, Width, width};
use crate::docs::{self, Doc, LinkTarget, Section};
use crate::route::Route;

const SIDEBAR: f32 = 240.0;
const OUTLINE: f32 = 220.0;
/// Below this window width the outline gives its room to the text.
const OUTLINE_MIN_WINDOW: f32 = 1320.0;

pub fn page(nav: Nav, doc: Doc) -> impl IntoElement {
    let sections: Rc<[Section]> = docs::sections(doc.markdown()).into();
    let refs: Rc<[ElementRef]> = sections.iter().map(|_| element_ref()).collect();
    let content = element_ref();
    scroll_to_anchor(nav, content, sections.clone(), refs.clone());
    view()
        .flex_1()
        .flex_row()
        .child(when(move || width() == Width::Wide, move || sidebar(nav, doc)))
        .child(
            scroll().flex_1().element_ref(&content).child(
                view()
                    .w_full()
                    .child(when(move || width() != Width::Wide, move || doc_picker(nav, doc)))
                    .child(article(nav, doc, &sections, &refs))
                    .child(super::footer(nav)),
            ),
        )
        .child(when(move || window().width.0 >= OUTLINE_MIN_WINDOW, {
            let sections = sections.clone();
            move || outline(nav, doc, &sections)
        }))
}

/// Scrolls to the section holding the address's heading whenever it changes; to the top without one.
fn scroll_to_anchor(nav: Nav, content: ElementRef, sections: Rc<[Section]>, refs: Rc<[ElementRef]>) {
    effect(move || {
        let anchor = nav.anchor.get();
        let target = anchor
            .as_ref()
            .and_then(|anchor| sections.iter().position(|s| s.anchor.as_ref() == Some(anchor)))
            .map(|index| refs[index]);
        spawn(async move {
            next_frame().await;
            let y = match target {
                Some(section) => match section.measure().await {
                    Some(at) => at.frame.origin.y,
                    None => return,
                },
                None => px(0.0),
            };
            content.scroll_to(point(px(0.0), y), true);
        });
    });
}

fn sidebar(nav: Nav, current: Doc) -> impl IntoElement {
    scroll()
        .w(px(SIDEBAR))
        .border_r_1()
        .border_color(move || theme().border)
        .child(
            view()
                .web_element(WebElement::Nav)
                .label("Documentation")
                .py_8()
                .px_4()
                .gap_1()
                .child(style::eyebrow("DOCUMENTATION").mb_2().ml_3())
                .children(Doc::ALL.map(|doc| doc_link(nav, doc, doc == current))),
        )
}

fn doc_link(nav: Nav, doc: Doc, current: bool) -> impl IntoElement {
    nav.link(Route::Docs(doc))
        .current(current)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(move || {
            if current {
                theme().accent
            } else {
                rgba(0x00000000).into()
            }
        })
        .hover(|s| s.bg(move || theme().muted))
        .child(
            text(doc.title())
                .text_sm()
                .font_weight(if current {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(move || {
                    if current {
                        theme().accent_foreground
                    } else {
                        theme().muted_foreground
                    }
                }),
        )
}

/// The docs as a row of chips that scrolls sideways, for windows without a sidebar.
fn doc_picker(nav: Nav, current: Doc) -> impl IntoElement {
    let chips = Doc::ALL.map(|doc| {
        let selected = doc == current;
        nav.link(Route::Docs(doc))
            .current(selected)
            .px_3()
            .py_2()
            .rounded_full()
            .border_1()
            .border_color(move || if selected { theme().primary } else { theme().border })
            .bg(move || if selected { theme().primary } else { theme().card })
            .child(text(doc.title()).text_sm().number_of_lines(1).text_color(move || {
                if selected {
                    theme().primary_foreground
                } else {
                    theme().foreground
                }
            }))
    });
    scroll()
        .horizontal()
        .scrollbar(false)
        .w_full()
        .border_b_1()
        .border_color(move || theme().border)
        .child(
            view()
                .web_element(WebElement::Nav)
                .label("Documentation")
                .flex_row()
                .gap_2()
                .py_3()
                .px(style::gutter)
                .children(chips),
        )
}

fn article(nav: Nav, doc: Doc, sections: &[Section], refs: &[ElementRef]) -> impl IntoElement {
    let blocks = sections.iter().zip(refs).map(|(section, section_ref)| {
        view().element_ref(section_ref).child(
            text_view()
                .markdown(section.markdown)
                .selectable(true)
                .text_size(px(16.0))
                .line_height(px(27.0))
                .text_color(move || theme().foreground)
                .on_link_press(move |event: &TextViewLinkPressEvent| follow(nav, doc, &event.url)),
        )
    });
    style::container(style::PROSE_MAX)
        .web_element(WebElement::Article)
        .py_10()
        .children(blocks)
        .child(pager(nav, doc))
}

fn follow(nav: Nav, doc: Doc, href: &str) {
    match docs::resolve_link(doc, href) {
        LinkTarget::Page(route, anchor) => nav.go(route, anchor),
        LinkTarget::Anchor(anchor) => nav.go(Route::Docs(doc), Some(anchor)),
        LinkTarget::External(url) => {
            let opened = open_url(url.clone());
            spawn(async move {
                if !opened.await {
                    log::warn!("could not open {url}");
                }
            });
        }
    }
}

fn pager(nav: Nav, doc: Doc) -> impl IntoElement {
    let at = Doc::ALL.iter().position(|d| *d == doc).unwrap_or_default();
    let neighbor = |doc: Option<&Doc>, label: &'static str, end: bool| {
        doc.map(|&doc| {
            nav.link(Route::Docs(doc))
                .flex_1()
                .min_w(px(200.0))
                .p_4()
                .gap_1()
                .rounded_lg()
                .border_1()
                .border_color(move || theme().border)
                .hover(|s| s.border_color(move || theme().primary))
                .style_when(end, |s| s.items_end())
                .child(style::muted(label).text_xs())
                .child(
                    text(doc.title())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().primary),
                )
        })
    };
    let edit = format!("https://github.com/casonadams/rho/edit/main/{}", doc.source_path());
    view()
        .mt_12()
        .gap_6()
        .child(
            nsh_base::link()
                .href(edit)
                .self_start()
                .child(style::muted("Edit this page on GitHub ↗").text_sm()),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_3()
                .child(neighbor(
                    at.checked_sub(1).and_then(|i| Doc::ALL.get(i)),
                    "← Previous",
                    false,
                ))
                .child(neighbor(Doc::ALL.get(at + 1), "Next →", true)),
        )
}

fn outline(nav: Nav, doc: Doc, sections: &[Section]) -> impl IntoElement + use<> {
    let entries = sections
        .iter()
        .filter(|section| section.level == 2)
        .filter_map(|section| section.anchor.clone().map(|anchor| (section.title.clone(), anchor)))
        .map(move |(title, anchor)| {
            let current = {
                let anchor = anchor.clone();
                move || nav.anchor.with(|a| a.as_deref() == Some(anchor.as_str()))
            };
            nsh_base::link()
                .href(Route::Docs(doc).href_at(Some(&anchor)))
                .open_with(nsh_base::LinkOpenWith::App)
                .on_open(move |_: &SharedString| nav.go(Route::Docs(doc), Some(anchor.clone())))
                .py_1()
                .child(text(title).text_sm().text_color(move || {
                    if current() {
                        theme().primary
                    } else {
                        theme().muted_foreground
                    }
                }))
        })
        .collect::<Vec<_>>();
    scroll().w(px(OUTLINE)).child(
        view()
            .py_10()
            .pr_6()
            .gap_1()
            .child(style::eyebrow("ON THIS PAGE").mb_2())
            .children(entries),
    )
}
