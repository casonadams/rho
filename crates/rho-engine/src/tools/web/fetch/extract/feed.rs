pub fn extract_feed_or_xml(raw: &str, _base_url: &str) -> String {
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

    if raw.contains("<urlset") || raw.contains("<sitemapindex") {
        return extract_sitemap_urls(raw);
    }

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
