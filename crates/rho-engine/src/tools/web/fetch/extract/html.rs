use std::sync::LazyLock;
use url::Url;

static BOILERPLATE_TAGS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?is)<(?:script|style|svg|noscript|nav|footer|header|aside)[^>]*>.*?</(?:script|style|svg|noscript|nav|footer|header|aside)>")
        .expect("valid boilerplate tag pattern")
});

static MARKDOWN_LINK: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").expect("valid markdown link pattern"));

pub fn extract_html(html: &str, base_url: &str, mode: &str) -> String {
    let mode_lower = mode.to_lowercase();
    let is_main = mode_lower != "full";

    let clean_html = if is_main {
        strip_boilerplate_tags(html)
    } else {
        html.to_string()
    };

    let text = html2text::from_read(clean_html.as_bytes(), 100).unwrap_or(clean_html);
    resolve_markdown_links(&text, base_url)
}

fn strip_boilerplate_tags(html: &str) -> String {
    if !BOILERPLATE_TAGS.is_match(html) {
        return html.to_string();
    }
    BOILERPLATE_TAGS.replace_all(html, "").into_owned()
}

pub fn resolve_markdown_links(text: &str, base_url: &str) -> String {
    let Ok(base) = Url::parse(base_url) else {
        return text.to_string();
    };

    let re_link = &*MARKDOWN_LINK;
    re_link
        .replace_all(text, |caps: &regex::Captures| {
            let label = &caps[1];
            let href = &caps[2];
            if href.starts_with("http://") || href.starts_with("https://") || href.starts_with('#') {
                caps[0].to_string()
            } else if let Ok(resolved) = base.join(href) {
                format!("[{label}]({resolved})")
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
}
