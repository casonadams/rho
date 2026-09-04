use std::sync::LazyLock;
use url::Url;

static BOILERPLATE_TAGS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    ["script", "style", "svg", "noscript", "nav", "footer", "header", "aside"]
        .iter()
        .filter_map(|tag| regex::Regex::new(&format!(r"(?is)<{tag}[^>]*>.*?</{tag}>")).ok())
        .collect()
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
    let mut out = html.to_string();
    for re in BOILERPLATE_TAGS.iter() {
        if re.is_match(&out) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    out
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
                format!("[{label}]({href})")
            } else if let Ok(resolved) = base.join(href) {
                format!("[{label}]({resolved})")
            } else {
                format!("[{label}]({href})")
            }
        })
        .to_string()
}
