use super::markdown::resolve_relative_url;
use std::collections::HashSet;

fn select_entry_link(entry: &feed_rs::model::Entry) -> &str {
    if let Some(link) = entry.links.iter().find(|l| l.rel.as_deref() == Some("alternate")) {
        return &link.href;
    }
    if let Some(link) = entry.links.iter().find(|l| l.rel.as_deref() != Some("self")) {
        return &link.href;
    }
    entry.links.first().map(|l| l.href.as_str()).unwrap_or("")
}

fn entry_summary(entry: &feed_rs::model::Entry) -> &str {
    entry
        .summary
        .as_ref()
        .or(entry
            .content
            .as_ref()
            .and_then(|c| c.body.as_ref().and(entry.summary.as_ref())))
        .map(|s| s.content.as_str())
        .unwrap_or("")
}

fn render_feed_entry(out: &mut String, idx: usize, entry: &feed_rs::model::Entry, base_url: &str) {
    let title = entry.title.as_ref().map_or("Untitled", |t| t.content.as_str());
    let link = resolve_relative_url(select_entry_link(entry), base_url);
    let date = entry
        .published
        .or(entry.updated)
        .map_or(String::new(), |d| d.to_rfc3339());
    let summary = entry_summary(entry);

    out.push_str(&format!("## {idx}. {title}\n"));
    if !date.is_empty() {
        out.push_str(&format!("Date: {date}\n"));
    }
    if !link.is_empty() {
        out.push_str(&format!("URL: {link}\n"));
    }
    if !summary.is_empty() {
        let plain = super::html::html_to_text(summary, 100);
        out.push_str(&format!("{}\n", plain.trim()));
    }
    out.push('\n');
}

fn render_feed(feed: feed_rs::model::Feed, base_url: &str) -> String {
    let mut out = String::new();
    let title = feed.title.as_ref().map_or("Feed", |t| t.content.as_str());
    out.push_str(&format!("# Feed: {title}\n"));
    if let Some(ref desc) = feed.description {
        out.push_str(&format!("{}\n\n", desc.content));
    }

    for (i, entry) in feed.entries.iter().take(50).enumerate() {
        render_feed_entry(&mut out, i + 1, entry, base_url);
    }
    out.trim().to_string()
}

pub fn extract_feed_or_xml(raw: &str, base_url: &str) -> String {
    if let Ok(feed) = feed_rs::parser::parse(raw.as_bytes()) {
        return render_feed(feed, base_url);
    }
    if raw.contains("<urlset") || raw.contains("<sitemapindex") {
        return extract_sitemap(raw);
    }
    super::html::html_to_text(raw, 100)
}

fn append_loc_text(e: &quick_xml::events::BytesText<'_>, seen: &mut HashSet<String>, urls: &mut Vec<String>) {
    if let Ok(decoded) = e.decode()
        && let Ok(txt) = quick_xml::escape::unescape(&decoded)
    {
        let url = txt.trim().to_string();
        if !url.is_empty() && seen.insert(url.clone()) {
            urls.push(url);
        }
    }
}

fn drain_sitemap_urls(xml_str: &str) -> Vec<String> {
    let mut reader = quick_xml::reader::Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let mut in_loc = false;
    let mut buf = Vec::new();

    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) => in_loc = e.name().as_ref() == b"loc",
            quick_xml::events::Event::End(e) if e.name().as_ref() == b"loc" => in_loc = false,
            quick_xml::events::Event::Text(ref e) if in_loc => append_loc_text(e, &mut seen, &mut urls),
            _ => {}
        }
        buf.clear();
    }
    urls
}

fn extract_sitemap(xml_str: &str) -> String {
    let urls = drain_sitemap_urls(xml_str);
    if urls.is_empty() {
        return xml_str.to_string();
    }
    let title = if xml_str.contains("<sitemapindex") {
        "Sitemap Index"
    } else {
        "Sitemap"
    };
    let mut out = format!("# {title}\n\n");
    for (i, u) in urls.iter().enumerate() {
        out.push_str(&format!("{}. {u}\n", i + 1));
    }
    out.trim_end().to_string()
}
