pub fn tool_title_style(is_error: bool) -> anstyle::Style {
    if is_error {
        anstyle::Style::new()
            .bold()
            .fg_color(Some(anstyle::AnsiColor::Red.into()))
    } else {
        anstyle::Style::new().bold()
    }
}

fn kind_from_format(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        "json" => "json",
        "csv" => "csv",
        "xml" => "xml",
        _ => "text",
    }
}

fn kind_from_url(url: &str) -> &'static str {
    if url.ends_with(".pdf") {
        "pdf"
    } else if url.ends_with(".json") {
        "json"
    } else if url.ends_with(".csv") {
        "csv"
    } else if url.ends_with(".xml") || url.ends_with(".rss") || url.ends_with(".atom") {
        "xml"
    } else {
        "text"
    }
}

pub fn fetch_content_kind(arguments: &serde_json::Value) -> &'static str {
    if let Some(format) = arguments.get("format").and_then(serde_json::Value::as_str) {
        return kind_from_format(format);
    }
    let url = arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    kind_from_url(&url)
}

pub fn detect_language_from_args(args: &serde_json::Value) -> Option<&str> {
    let path = args.get("path").or_else(|| args.get("file_path"))?.as_str()?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}

pub fn format_bash_args_header(summary: &str, accent: anstyle::Style, dim: anstyle::Style) -> String {
    if let Some(idx) = summary.rfind(" (timeout ")
        && summary.ends_with(')')
    {
        let timeout_part = &summary[idx + 1..];
        let inner = &timeout_part["(timeout ".len()..timeout_part.len() - 1];
        if inner.ends_with('s') && inner[..inner.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            let cmd = &summary[..idx];
            return format!("{accent}{cmd}{accent:#} {dim}{timeout_part}{dim:#}");
        }
    }
    format!("{accent}{summary}{accent:#}")
}
