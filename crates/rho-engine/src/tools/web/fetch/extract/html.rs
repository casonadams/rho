use super::markdown::resolve_markdown_links;
use rho_harness_core::error::{AppError, Result};
use scraper::{Html, Selector};
use std::sync::LazyLock;
use url::Url;

static BASE_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("base[href]").expect("valid selector"));
static TITLE_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("title").expect("valid selector"));
static AUTHOR_SEL: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse(r#"meta[name="author"], meta[property="article:author"]"#).expect("valid selector")
});
static SITE_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(r#"meta[property="og:site_name"]"#).expect("valid selector"));
static MAIN_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("main, article").expect("valid selector"));
static SCRIPT_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("script").expect("valid selector"));
static SPA_ROOT_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("#app, #root, #__next").expect("valid selector"));

fn effective_base_url(document: &Html, response_url: &str) -> String {
    if let Some(base_el) = document.select(&BASE_SEL).next()
        && let Some(href) = base_el.value().attr("href")
        && let Ok(base) = Url::parse(response_url)
        && let Ok(effective) = base.join(href)
    {
        return effective.to_string();
    }
    response_url.to_string()
}

fn check_spa_shell(document: &Html, full_text_len: usize) -> Result<()> {
    if full_text_len >= 100 {
        return Ok(());
    }
    let script_len: usize = document
        .select(&SCRIPT_SEL)
        .map(|s| s.text().map(|t| t.len()).sum::<usize>())
        .sum();
    if script_len > 500 || document.select(&SPA_ROOT_SEL).next().is_some() {
        return Err(AppError::Tool(
            "Page contains little static content and may require a JavaScript-capable browser".to_string(),
        ));
    }
    Ok(())
}

fn extract_meta(document: &Html) -> (Option<String>, Option<String>, Option<String>) {
    let title = document
        .select(&TITLE_SEL)
        .next()
        .map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .filter(|s| !s.is_empty());
    let author = document
        .select(&AUTHOR_SEL)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let site = document
        .select(&SITE_SEL)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (title, author, site)
}

pub fn html_to_text(html: &str, width: usize) -> String {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        html2text::from_read(html.as_bytes(), width).ok()
    }))
    .ok()
    .flatten()
    .or_else(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            html2text::from_read(html.as_bytes(), 10_000).ok()
        }))
        .ok()
        .flatten()
    });
    std::panic::set_hook(prev_hook);

    result.unwrap_or_else(|| {
        let doc = Html::parse_fragment(html);
        doc.root_element().text().collect::<Vec<_>>().join(" ")
    })
}

fn find_semantic_main(document: &Html, full_text_len: usize, base_url: &str) -> Option<(String, String)> {
    let threshold = 400.min(120.max(full_text_len * 15 / 100));
    let mut best_text = String::new();
    let mut best_tag = String::new();

    for candidate in document.select(&MAIN_SEL) {
        let tag_name = candidate.value().name().to_string();
        let html_content = candidate.html();
        let text = html_to_text(&html_content, 120);
        let trimmed = text.trim().to_string();
        if trimmed.len() >= threshold && trimmed.len() > best_text.len() {
            best_text = resolve_markdown_links(&trimmed, base_url);
            best_tag = format!("<{tag_name}>");
        }
    }

    (!best_text.is_empty()).then_some((best_tag, best_text))
}

fn format_main_output(tag: &str, text: &str, document: &Html) -> String {
    let (title, author, site) = extract_meta(document);
    let mut out = format!(
        "[HTML extraction: main content via {tag}. Use mode=\"full\" if important navigation or sidebars are missing.]\n\n"
    );
    if let Some(t) = title {
        out.push_str(&format!("# {t}\n\n"));
    }
    if let Some(a) = author {
        out.push_str(&format!("By: {a}\n\n"));
    }
    if let Some(s) = site {
        out.push_str(&format!("Site: {s}\n\n"));
    }
    out.push_str(text);
    out
}

fn try_main_mode(document: &Html, full_len: usize, base_url: &str, is_main: bool) -> Result<Option<String>> {
    if let Some((tag, main_text)) = find_semantic_main(document, full_len, base_url) {
        return Ok(Some(format_main_output(&tag, &main_text, document)));
    }
    if is_main {
        return Err(AppError::Tool(
            "Could not identify substantial main content; retry with mode=\"full\"".to_string(),
        ));
    }
    Ok(None)
}

pub fn extract_html(html: &str, response_url: &str, mode: &str) -> Result<String> {
    let document = Html::parse_document(html);
    let base_url = effective_base_url(&document, response_url);
    let full_raw = html_to_text(html, 120);
    let full_text = resolve_markdown_links(full_raw.trim(), &base_url);

    let mode_lower = mode.to_lowercase();
    if mode_lower != "full"
        && let Some(main_out) = try_main_mode(&document, full_text.len(), &base_url, mode_lower == "main")?
    {
        return Ok(main_out);
    }

    check_spa_shell(&document, full_text.len())?;
    Ok(format!("[HTML extraction: full page.]\n\n{full_text}"))
}
