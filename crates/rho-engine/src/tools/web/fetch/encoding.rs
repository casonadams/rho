use encoding_rs::Encoding;
use std::sync::LazyLock;

static CHARSET_CT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?i)charset\s*=\s*["']?([^\s;"']+)"#).expect("valid regex"));

static META_CHARSET: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?i)<meta\s+[^>]*charset=["']?\s*([^\s"'/>]+)|<meta\s+[^>]*content=["'][^"']*charset=([^\s"';/>]+)"#,
    )
    .expect("valid regex")
});

pub fn charset_from_bom(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some("utf-8")
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        Some("utf-16le")
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        Some("utf-16be")
    } else {
        None
    }
}

pub fn charset_from_content_type(content_type: &str) -> Option<String> {
    CHARSET_CT
        .captures(content_type)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

pub fn charset_from_html(bytes: &[u8]) -> Option<String> {
    let limit = bytes.len().min(4096);
    let head = String::from_utf8_lossy(&bytes[..limit]);
    META_CHARSET
        .captures(&head)
        .and_then(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string()))
}

pub fn decode_body(bytes: &[u8], content_type: &str) -> String {
    let label = charset_from_bom(bytes)
        .map(ToString::to_string)
        .or_else(|| charset_from_content_type(content_type))
        .or_else(|| charset_from_html(bytes))
        .unwrap_or_else(|| "utf-8".to_string());

    if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
        let (cow, _) = encoding.decode_with_bom_removal(bytes);
        cow.into_owned()
    } else {
        let (cow, _) = encoding_rs::UTF_8.decode_with_bom_removal(bytes);
        cow.into_owned()
    }
}

pub fn is_pdf(bytes: &[u8], content_type: &str) -> bool {
    content_type.to_lowercase().contains("application/pdf") || bytes.starts_with(b"%PDF-")
}

pub fn is_supported_content_type(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.is_empty()
        || ct.starts_with("text/")
        || ct.contains("html")
        || ct.contains("xhtml")
        || ct.contains("json")
        || ct.contains("xml")
        || ct.contains("csv")
        || ct.contains("tab-separated")
        || ct.contains("markdown")
        || ct.contains("pdf")
}
