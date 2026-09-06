fn render_feed_header(out: &mut String, feed: &feed_rs::model::Feed) {
    if let Some(ref title) = feed.title {
        out.push_str(&format!("# {}\n", title.content));
    }
    if let Some(ref desc) = feed.description {
        out.push_str(&format!("{}\n\n", desc.content));
    }
}

fn render_feed_entry(out: &mut String, entry: &feed_rs::model::Entry) {
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

fn render_feed(feed: feed_rs::model::Feed) -> String {
    let mut out = String::new();
    render_feed_header(&mut out, &feed);
    for entry in feed.entries.iter().take(30) {
        render_feed_entry(&mut out, entry);
    }
    out.trim().to_string()
}

pub fn extract_feed_or_xml(raw: &str, _base_url: &str) -> String {
    if let Ok(feed) = feed_rs::parser::parse(raw.as_bytes()) {
        return render_feed(feed);
    }
    if raw.contains("<urlset") || raw.contains("<sitemapindex") {
        return extract_sitemap_urls(raw);
    }
    html2text::from_read(raw.as_bytes(), 100).unwrap_or_else(|_| raw.to_string())
}

fn handle_sitemap_text(e: &quick_xml::events::BytesText<'_>, in_loc: bool, urls: &mut Vec<String>) {
    if in_loc
        && let Ok(decoded) = e.decode()
        && let Ok(txt) = quick_xml::escape::unescape(&decoded)
    {
        urls.push(txt.to_string());
    }
}

fn handle_sitemap_event(event: &quick_xml::events::Event, in_loc: &mut bool, urls: &mut Vec<String>) {
    match event {
        quick_xml::events::Event::Start(e) => {
            if e.name().as_ref() == b"loc" {
                *in_loc = true;
            }
        }
        quick_xml::events::Event::End(e) => {
            if e.name().as_ref() == b"loc" {
                *in_loc = false;
            }
        }
        quick_xml::events::Event::Text(e) => handle_sitemap_text(e, *in_loc, urls),
        _ => {}
    }
}

fn drain_sitemap_urls(xml_str: &str) -> Vec<String> {
    let mut reader = quick_xml::reader::Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);
    let mut urls = Vec::new();
    let mut in_loc = false;
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        if matches!(event, quick_xml::events::Event::Eof) {
            break;
        }
        handle_sitemap_event(&event, &mut in_loc, &mut urls);
        buf.clear();
    }
    urls
}

fn extract_sitemap_urls(xml_str: &str) -> String {
    let urls = drain_sitemap_urls(xml_str);
    if urls.is_empty() {
        return xml_str.to_string();
    }
    let mut out = format!("Sitemap containing {} URLs:\n", urls.len());
    for u in urls.iter().take(100) {
        out.push_str(&format!("- {u}\n"));
    }
    if urls.len() > 100 {
        out.push_str(&format!("[... and {} more URLs]", urls.len() - 100));
    }
    out
}
