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

const SUPPORTED_SUBSTRINGS: &[&str] = &[
    "html",
    "xhtml",
    "json",
    "xml",
    "csv",
    "tab-separated",
    "markdown",
    "pdf",
];

pub fn is_supported_content_type(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.is_empty() || ct.starts_with("text/") || SUPPORTED_SUBSTRINGS.iter().any(|&s| ct.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_content_type() {
        assert!(is_supported_content_type(""));
        assert!(is_supported_content_type("text/plain"));
        assert!(is_supported_content_type("text/html; charset=utf-8"));
        assert!(is_supported_content_type("application/json"));
        assert!(is_supported_content_type("application/xml"));
        assert!(is_supported_content_type("text/csv"));
        assert!(is_supported_content_type("application/tab-separated-values"));
        assert!(is_supported_content_type("text/markdown"));
        assert!(is_supported_content_type("application/pdf"));
        assert!(is_supported_content_type("application/xhtml+xml"));
        assert!(!is_supported_content_type("image/png"));
        assert!(!is_supported_content_type("application/octet-stream"));
        assert!(!is_supported_content_type("video/mp4"));
    }

    #[test]
    fn test_charset_from_bom() {
        assert_eq!(charset_from_bom(&[0xef, 0xbb, 0xbf, 0x48, 0x69]), Some("utf-8"));
        assert_eq!(charset_from_bom(&[0xff, 0xfe, 0x48, 0x00]), Some("utf-16le"));
        assert_eq!(charset_from_bom(&[0xfe, 0xff, 0x00, 0x48]), Some("utf-16be"));
        assert_eq!(charset_from_bom(b"hello"), None);
        assert_eq!(charset_from_bom(&[]), None);
    }

    #[test]
    fn test_charset_from_content_type() {
        assert_eq!(
            charset_from_content_type("text/html; charset=utf-8"),
            Some("utf-8".to_string())
        );
        assert_eq!(
            charset_from_content_type("text/html; charset=\"iso-8859-1\""),
            Some("iso-8859-1".to_string())
        );
        assert_eq!(charset_from_content_type("text/html"), None);
    }

    #[test]
    fn test_charset_from_html() {
        let html1 = b"<html><head><meta charset=\"utf-8\"></head></html>";
        assert_eq!(charset_from_html(html1), Some("utf-8".to_string()));

        let html2 = b"<html><head><meta content=\"text/html; charset=iso-8859-1\"></head></html>";
        assert_eq!(charset_from_html(html2), Some("iso-8859-1".to_string()));

        let html3 = b"<html><head><title>No charset</title></head></html>";
        assert_eq!(charset_from_html(html3), None);
    }

    #[test]
    fn test_decode_body() {
        let bom_bytes = [0xef, 0xbb, 0xbf, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(decode_body(&bom_bytes, "text/plain"), "Hello");

        let plain_bytes = b"Hello world";
        assert_eq!(decode_body(plain_bytes, "text/plain; charset=utf-8"), "Hello world");

        assert_eq!(
            decode_body(plain_bytes, "text/plain; charset=invalid-charset-label"),
            "Hello world"
        );
    }

    #[test]
    fn test_is_pdf() {
        assert!(is_pdf(b"", "application/pdf"));
        assert!(is_pdf(b"%PDF-1.4...", "application/octet-stream"));
        assert!(!is_pdf(b"not a pdf", "text/plain"));
    }
}
