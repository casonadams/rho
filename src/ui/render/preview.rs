pub fn tool_title_style(is_error: bool) -> anstyle::Style {
    if is_error {
        anstyle::Style::new()
            .bold()
            .fg_color(Some(anstyle::AnsiColor::Red.into()))
    } else {
        anstyle::Style::new().bold()
    }
}

pub fn fetch_content_kind(arguments: &serde_json::Value) -> &'static str {
    if let Some(format) = arguments.get("format").and_then(serde_json::Value::as_str) {
        return match format.to_ascii_lowercase().as_str() {
            "pdf" => "pdf",
            "json" => "json",
            "csv" => "csv",
            "xml" => "xml",
            _ => "text",
        };
    }
    let url = arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
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

pub fn detect_language_from_args(args: &serde_json::Value) -> Option<&str> {
    let path = args.get("path").or_else(|| args.get("file_path"))?.as_str()?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}
