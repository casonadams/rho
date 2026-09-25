//! Specialized extractor for YouTube video metadata and transcripts.

use crate::tools::web::http::{HttpClient, HttpRequest};
use rho_harness_core::error::AppError;
use url::Url;

/// Represents a recognized YouTube video target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeUrl {
    pub video_id: String,
}

/// Check if a string is a valid YouTube 11-character video ID.
pub fn is_valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parse a URL into a structured `YouTubeUrl` if it targets a YouTube video.
pub fn parse_youtube_url(raw_url: &str) -> Option<YouTubeUrl> {
    let parsed = Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?;

    if host.eq_ignore_ascii_case("youtu.be") {
        let mut segments = parsed.path_segments()?;
        let first = segments.next()?.trim();
        if is_valid_video_id(first) {
            return Some(YouTubeUrl {
                video_id: first.to_string(),
            });
        }
        return None;
    }

    if !host.eq_ignore_ascii_case("youtube.com")
        && !host.eq_ignore_ascii_case("www.youtube.com")
        && !host.eq_ignore_ascii_case("m.youtube.com")
    {
        return None;
    }

    let path = parsed.path();
    if path == "/watch" {
        for (k, v) in parsed.query_pairs() {
            if k == "v" && is_valid_video_id(&v) {
                return Some(YouTubeUrl {
                    video_id: v.into_owned(),
                });
            }
        }
        return None;
    }

    for prefix in ["/shorts/", "/embed/", "/v/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let candidate = rest.split('/').next().unwrap_or("").trim();
            if is_valid_video_id(candidate) {
                return Some(YouTubeUrl {
                    video_id: candidate.to_string(),
                });
            }
        }
    }

    None
}

/// Extract `ytInitialPlayerResponse` JSON object from YouTube watch HTML.
pub fn extract_player_response(html: &str) -> Option<serde_json::Value> {
    let needle = "ytInitialPlayerResponse";
    let start_idx = html.find(needle)?;
    let after_needle = &html[start_idx + needle.len()..];
    let eq_idx = after_needle.find('=')?;
    let json_start = after_needle[eq_idx + 1..].trim_start();
    if !json_start.starts_with('{') {
        return None;
    }

    let mut de = serde_json::Deserializer::from_str(json_start).into_iter::<serde_json::Value>();
    de.next()?.ok()
}

/// Select best caption track: English manual > English ASR > Any manual > First track.
pub fn select_caption_track(tracks: &[serde_json::Value]) -> Option<&serde_json::Value> {
    if tracks.is_empty() {
        return None;
    }

    // 1. Manual English track
    if let Some(track) = tracks.iter().find(|t| {
        let lang = t["languageCode"].as_str().unwrap_or("");
        let kind = t["kind"].as_str().unwrap_or("");
        lang.starts_with("en") && kind != "asr"
    }) {
        return Some(track);
    }

    // 2. Auto-generated English track
    if let Some(track) = tracks
        .iter()
        .find(|t| t["languageCode"].as_str().unwrap_or("").starts_with("en"))
    {
        return Some(track);
    }

    // 3. Any manual track
    if let Some(track) = tracks.iter().find(|t| t["kind"].as_str().unwrap_or("") != "asr") {
        return Some(track);
    }

    // 4. First available track
    tracks.first()
}

/// Parse XML timed-text captions into chronological (timestamp_seconds, text) cues.
pub fn parse_timedtext_xml(xml: &str) -> Vec<(u64, String)> {
    let mut reader = quick_xml::reader::Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut cues = Vec::new();
    let mut current_start = 0_u64;
    let mut current_text = String::new();
    let mut in_text = false;
    let mut last_text = String::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match &event {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(e)) if e.name().as_ref() == b"text" => {
                in_text = true;
                current_start = extract_start_attr(e);
                current_text.clear();
            }
            Ok(quick_xml::events::Event::End(e)) if e.name().as_ref() == b"text" => {
                in_text = false;
                finalize_cue(current_start, &current_text, &mut last_text, &mut cues);
                current_text.clear();
            }
            Ok(quick_xml::events::Event::Text(t)) if in_text => {
                if let Ok(raw) = std::str::from_utf8(t.as_ref()) {
                    current_text.push_str(raw);
                }
            }
            Ok(quick_xml::events::Event::GeneralRef(r)) if in_text => {
                if let Ok(name) = std::str::from_utf8(r.as_ref()) {
                    let entity = format!("&{name};");
                    if let Ok(unesc) = quick_xml::escape::unescape(&entity) {
                        current_text.push_str(&unesc);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }

    cues
}

fn finalize_cue(start: u64, raw_text: &str, last_text: &mut String, cues: &mut Vec<(u64, String)>) {
    let cleaned = raw_text.trim().replace('\n', " ");
    if !cleaned.is_empty() && cleaned != *last_text {
        *last_text = cleaned.clone();
        cues.push((start, cleaned));
    }
}

fn extract_start_attr(e: &quick_xml::events::BytesStart<'_>) -> u64 {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"start"
            && let Ok(val_str) = std::str::from_utf8(&attr.value)
            && let Ok(sec) = val_str.parse::<f64>()
        {
            return sec.max(0.0) as u64;
        }
    }
    0
}

/// Format seconds as `[MM:SS]` or `[HH:MM:SS]`.
pub fn format_timestamp(seconds: u64) -> String {
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("[{hours:02}:{mins:02}:{secs:02}]")
    } else {
        format!("[{mins:02}:{secs:02}]")
    }
}

/// Format duration in human readable string.
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {mins}m {secs}s")
    } else if mins > 0 {
        format!("{mins}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// Format large numbers with commas.
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + (s.len() / 3));
    let rem = s.len() % 3;
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (i == rem || (i > rem && (i - rem).is_multiple_of(3))) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Format video details and dialogue cues into clean Markdown.
pub fn format_youtube_markdown(
    video_id: &str,
    details: &serde_json::Value,
    language: Option<&str>,
    cues: &[(u64, String)],
) -> String {
    let title = details["title"].as_str().unwrap_or("YouTube Video");
    let author = details["author"].as_str().unwrap_or("Unknown Channel");
    let duration_sec = details["lengthSeconds"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let view_count = details["viewCount"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let description = details["shortDescription"].as_str().unwrap_or("").trim();

    let mut out = format!("# {title}\n\n");
    out.push_str(&format!("**Channel:** {author}\n"));
    if duration_sec > 0 {
        out.push_str(&format!("**Duration:** {}\n", format_duration(duration_sec)));
    }
    if view_count > 0 {
        out.push_str(&format!("**Views:** {}\n", format_number(view_count)));
    }
    out.push_str(&format!("**Video ID:** {video_id}\n\n"));

    if !description.is_empty() {
        let desc_preview = if description.len() > 1000 {
            format!("{}…", &description[..1000])
        } else {
            description.to_string()
        };
        out.push_str(&format!("---\n\n## Description\n\n{desc_preview}\n\n"));
    }

    out.push_str("---\n\n");
    if cues.is_empty() {
        out.push_str("*No transcript available for this video.*\n");
    } else {
        let lang_label = language.unwrap_or("Auto");
        out.push_str(&format!("## Transcript ({lang_label})\n\n"));
        for (start, text) in cues {
            out.push_str(&format!("{} {}\n", format_timestamp(*start), text));
        }
    }

    out.trim_end().to_string()
}

/// Extract YouTube video metadata and captions via HTTP from a specific watch page URL.
pub async fn extract_youtube_url(
    http: &HttpClient,
    target: &YouTubeUrl,
    watch_url: &str,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let req = HttpRequest {
        url: watch_url,
        user_agent: Some(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        ),
        timeout_sec,
        max_bytes: 5 * 1024 * 1024,
        pdf_max_bytes: None,
    };

    let resp = http.get_text(req).await?;
    let player_response =
        extract_player_response(&resp.body).unwrap_or_else(|| serde_json::json!({ "videoDetails": {} }));

    let details = &player_response["videoDetails"];
    let tracks = player_response["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
        .as_array()
        .map(|v| v.as_slice())
        .unwrap_or_default();

    let best_track = select_caption_track(tracks);
    let (lang_label, cues) = match best_track {
        Some(track) => {
            let base_url = track["baseUrl"].as_str().unwrap_or("");
            let lang = track["name"]["simpleText"]
                .as_str()
                .or_else(|| track["languageCode"].as_str());
            let cues = if !base_url.is_empty() {
                fetch_caption_cues(http, base_url, timeout_sec).await
            } else {
                Vec::new()
            };
            (lang, cues)
        }
        None => (None, Vec::new()),
    };

    Ok(format_youtube_markdown(&target.video_id, details, lang_label, &cues))
}
/// Extract YouTube video metadata and captions via HTTP.
pub async fn extract_youtube(http: &HttpClient, target: &YouTubeUrl, timeout_sec: u64) -> Result<String, AppError> {
    let watch_url = format!("https://www.youtube.com/watch?v={}", target.video_id);
    extract_youtube_url(http, target, &watch_url, timeout_sec).await
}

async fn fetch_caption_cues(http: &HttpClient, base_url: &str, timeout_sec: u64) -> Vec<(u64, String)> {
    let req = HttpRequest {
        url: base_url,
        user_agent: Some(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        ),
        timeout_sec,
        max_bytes: 5 * 1024 * 1024,
        pdf_max_bytes: None,
    };

    match http.get_text(req).await {
        Ok(resp) => parse_timedtext_xml(&resp.body),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_watch_url() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "dQw4w9WgXcQ".into(),
            })
        );
    }

    #[test]
    fn parse_watch_url_with_additional_params() {
        let url = "https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=42s&feature=shared";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "dQw4w9WgXcQ".into(),
            })
        );
    }

    #[test]
    fn parse_youtu_be_url() {
        let url = "https://youtu.be/dQw4w9WgXcQ?si=abcdef";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "dQw4w9WgXcQ".into(),
            })
        );
    }

    #[test]
    fn parse_shorts_url() {
        let url = "https://www.youtube.com/shorts/jNQXAC9IVRw";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "jNQXAC9IVRw".into(),
            })
        );
    }

    #[test]
    fn parse_embed_url() {
        let url = "https://youtube.com/embed/dQw4w9WgXcQ";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "dQw4w9WgXcQ".into(),
            })
        );
    }

    #[test]
    fn parse_v_url() {
        let url = "https://youtube.com/v/dQw4w9WgXcQ";
        assert_eq!(
            parse_youtube_url(url),
            Some(YouTubeUrl {
                video_id: "dQw4w9WgXcQ".into(),
            })
        );
    }

    #[test]
    fn parse_non_video_youtube_urls() {
        assert_eq!(parse_youtube_url("https://www.youtube.com/"), None);
        assert_eq!(parse_youtube_url("https://www.youtube.com/feed/subscriptions"), None);
        assert_eq!(parse_youtube_url("https://www.youtube.com/watch"), None);
        assert_eq!(parse_youtube_url("https://www.youtube.com/watch?v=too_short"), None);
        assert_eq!(parse_youtube_url("https://example.com/watch?v=dQw4w9WgXcQ"), None);
    }

    #[test]
    fn test_extract_player_response_json() {
        let html = r#"<html><head><script>var ytInitialPlayerResponse = {"videoDetails":{"title":"Never Gonna Give You Up","author":"Rick Astley","lengthSeconds":"212"}};var foo = 123;</script></head><body></body></html>"#;
        let response = extract_player_response(html).expect("parsed response");
        assert_eq!(response["videoDetails"]["title"], "Never Gonna Give You Up");
        assert_eq!(response["videoDetails"]["author"], "Rick Astley");
    }

    #[test]
    fn test_select_caption_track_priority() {
        let tracks = vec![
            serde_json::json!({
                "languageCode": "es",
                "kind": "standard",
                "baseUrl": "https://example.com/es"
            }),
            serde_json::json!({
                "languageCode": "en",
                "kind": "asr",
                "baseUrl": "https://example.com/en-asr"
            }),
            serde_json::json!({
                "languageCode": "en",
                "kind": "standard",
                "name": { "simpleText": "English (United States)" },
                "baseUrl": "https://example.com/en-manual"
            }),
        ];

        let selected = select_caption_track(&tracks).expect("selected track");
        assert_eq!(selected["baseUrl"], "https://example.com/en-manual");
    }

    #[test]
    fn test_parse_timedtext_xml_unescapes_and_deduplicates() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<transcript>
  <text start="0.4" dur="2.14">hello &amp; welcome</text>
  <text start="2.54" dur="1.8">hello &amp; welcome</text>
  <text start="4.34" dur="2.0">it&#39;s &quot;great&quot; to see you</text>
</transcript>"#;

        let cues = parse_timedtext_xml(xml);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0], (0, "hello & welcome".into()));
        assert_eq!(cues[1], (4, "it's \"great\" to see you".into()));
    }

    #[test]
    fn test_formatting_helpers() {
        assert_eq!(format_timestamp(45), "[00:45]");
        assert_eq!(format_timestamp(125), "[02:05]");
        assert_eq!(format_timestamp(3665), "[01:01:05]");

        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(125), "2m 5s");
        assert_eq!(format_duration(3665), "1h 1m 5s");

        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(12345678), "12,345,678");
    }

    #[test]
    fn test_format_youtube_markdown_with_cues() {
        let details = serde_json::json!({
            "title": "Rust in 100 Seconds",
            "author": "Fireship",
            "lengthSeconds": "142",
            "viewCount": "2400100",
            "shortDescription": "Rust is a fast and memory-efficient systems programming language."
        });
        let cues = vec![
            (0, "Rust is a systems programming language".into()),
            (5, "developed by Graydon Hoare at Mozilla".into()),
        ];

        let md = format_youtube_markdown("u72H_zZAEcw", &details, Some("English"), &cues);
        assert!(md.contains("# Rust in 100 Seconds"));
        assert!(md.contains("**Channel:** Fireship"));
        assert!(md.contains("**Duration:** 2m 22s"));
        assert!(md.contains("**Views:** 2,400,100"));
        assert!(md.contains("**Video ID:** u72H_zZAEcw"));
        assert!(md.contains("Rust is a fast and memory-efficient systems programming language."));
        assert!(md.contains("## Transcript (English)"));
        assert!(md.contains("[00:00] Rust is a systems programming language"));
        assert!(md.contains("[00:05] developed by Graydon Hoare at Mozilla"));
    }

    #[test]
    fn test_format_youtube_markdown_without_cues() {
        let details = serde_json::json!({
            "title": "Music Track",
            "author": "Artist",
            "lengthSeconds": "180"
        });

        let md = format_youtube_markdown("abcdefghijk", &details, None, &[]);
        assert!(md.contains("# Music Track"));
        assert!(md.contains("*No transcript available for this video.*"));
    }

    #[tokio::test]
    async fn test_extract_youtube_url_mock() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        crate::install_crypto_provider();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let html_body = format!(
            r#"<html><head><script>var ytInitialPlayerResponse = {{"videoDetails":{{"title":"Mock Video","author":"Mock Author","lengthSeconds":"60"}},"captions":{{"playerCaptionsTracklistRenderer":{{"captionTracks":[{{"baseUrl":"http://{addr}/timedtext","languageCode":"en","kind":"asr"}}]}}}}}};</script></head><body></body></html>"#
        );
        let timedtext_body = r#"<?xml version="1.0" encoding="utf-8" ?><transcript><text start="1.5" dur="3.0">Hello from mock transcript</text></transcript>"#;

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html_body.len(),
                    html_body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    timedtext_body.len(),
                    timedtext_body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let http = HttpClient::new(true).unwrap();
        let target = YouTubeUrl {
            video_id: "dQw4w9WgXcQ".to_string(),
        };
        let md = extract_youtube_url(&http, &target, &format!("http://{addr}/watch"), 5)
            .await
            .expect("extracted mock youtube");

        assert!(md.contains("# Mock Video"));
        assert!(md.contains("**Channel:** Mock Author"));
        assert!(md.contains("## Transcript (en)"));
        assert!(md.contains("[00:01] Hello from mock transcript"));
    }
}
