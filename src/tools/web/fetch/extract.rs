use crate::error::{AppError, Result};
use url::Url;

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
    let tags = ["script", "style", "svg", "noscript", "nav", "footer", "header", "aside"];
    let mut out = html.to_string();
    for tag in tags {
        let pattern = format!(r"(?is)<{tag}[^>]*>.*?</{tag}>");
        if let Ok(re) = regex::Regex::new(&pattern) {
            out = re.replace_all(&out, "").to_string();
        }
    }
    out
}

pub fn extract_json(raw: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

pub fn extract_feed_or_xml(raw: &str, _base_url: &str) -> String {
    // Try feed-rs first for RSS / Atom
    if let Ok(feed) = feed_rs::parser::parse(raw.as_bytes()) {
        let mut out = String::new();
        if let Some(title) = feed.title {
            out.push_str(&format!("# {}\n", title.content));
        }
        if let Some(desc) = feed.description {
            out.push_str(&format!("{}\n\n", desc.content));
        }
        for entry in feed.entries.iter().take(30) {
            let title = entry.title.as_ref().map(|t| t.content.as_str()).unwrap_or("Untitled");
            let link = entry.links.first().map(|l| l.href.as_str()).unwrap_or("");
            let summary = entry.summary.as_ref().map(|s| s.content.as_str()).unwrap_or("");

            out.push_str(&format!("## {title}\n"));
            if !link.is_empty() {
                out.push_str(&format!("Link: {link}\n"));
            }
            if !summary.is_empty() {
                out.push_str(&format!("{summary}\n"));
            }
            out.push('\n');
        }
        return out.trim().to_string();
    }

    // Try XML sitemap extraction
    if raw.contains("<urlset") || raw.contains("<sitemapindex") {
        return extract_sitemap_urls(raw);
    }

    // Fallback: strip tags or return raw
    html2text::from_read(raw.as_bytes(), 100).unwrap_or_else(|_| raw.to_string())
}

fn extract_sitemap_urls(xml_str: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut urls = Vec::new();
    let mut in_loc = false;

    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            Event::Start(e) if e.name().as_ref() == b"loc" => {
                in_loc = true;
            }
            Event::End(e) if e.name().as_ref() == b"loc" => {
                in_loc = false;
            }
            Event::Text(e) if in_loc => {
                if let Ok(txt) = e.unescape() {
                    urls.push(txt.to_string());
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    if !urls.is_empty() {
        let mut out = format!("Sitemap containing {} URLs:\n", urls.len());
        for u in urls.iter().take(100) {
            out.push_str(&format!("- {u}\n"));
        }
        if urls.len() > 100 {
            out.push_str(&format!("[... and {} more URLs]", urls.len() - 100));
        }
        return out;
    }

    xml_str.to_string()
}

pub fn extract_csv(raw: &str, delimiter: u8) -> String {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(raw.as_bytes());

    let mut out = String::new();
    if let Ok(headers) = rdr.headers() {
        let header_row = headers.iter().collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {header_row} |\n"));
        let sep = headers.iter().map(|_| "---").collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {sep} |\n"));
    }

    for record in rdr.records().take(100).flatten() {
        let row = record.iter().collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("| {row} |\n"));
    }
    out
}

pub async fn extract_pdf_bytes(bytes: Vec<u8>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| AppError::Tool(format!("PDF extraction error: {e}")))
    })
    .await
    .map_err(|e| AppError::Tool(format!("Tokio spawn error during PDF extraction: {e}")))?
}

pub fn resolve_markdown_links(text: &str, base_url: &str) -> String {
    let Ok(base) = Url::parse(base_url) else {
        return text.to_string();
    };

    let re_link = regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_html() {
        let html = "<html><body><h1>Hello World</h1><nav>Skip me</nav><p>Content paragraph</p></body></html>";
        let res = extract_html(html, "https://example.com", "main");
        assert!(res.contains("Hello World"));
        assert!(res.contains("Content paragraph"));
        assert!(!res.contains("Skip me"));
    }

    #[test]
    fn test_extract_json() {
        let json = r#"{"name":"test","count":42}"#;
        let res = extract_json(json);
        assert!(res.contains("\"name\": \"test\""));
        assert!(res.contains("\"count\": 42"));
    }

    #[test]
    fn test_extract_csv() {
        let csv_data = "name,age,city\nAlice,30,NYC\nBob,25,SF\n";
        let res = extract_csv(csv_data, b',');
        assert!(res.contains("| name | age | city |"));
        assert!(res.contains("| Alice | 30 | NYC |"));
    }
}
