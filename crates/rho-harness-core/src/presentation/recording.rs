use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastFrame {
    /// Timestamp in milliseconds from recording start
    pub offset_ms: u64,
    /// Terminal event type ("o" for stdout output, "i" for user input)
    pub event_type: String,
    /// Redacted payload data
    pub data: String,
}

pub struct CastRecorder {
    file: File,
    start_time: Instant,
    active: bool,
}

impl CastRecorder {
    pub fn start(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let file = File::create(path)?;
        Ok(Self {
            file,
            start_time: Instant::now(),
            active: true,
        })
    }

    pub fn record_output(&mut self, text: &str) -> Result<(), std::io::Error> {
        if !self.active {
            return Ok(());
        }
        let offset_ms = self.start_time.elapsed().as_millis() as u64;
        let redacted = redact_secrets(text);
        let frame = CastFrame {
            offset_ms,
            event_type: "o".to_string(),
            data: redacted,
        };
        let serialized = serde_json::to_string(&frame).map_err(std::io::Error::other)?;
        writeln!(self.file, "{serialized}")?;
        self.file.flush()?;
        Ok(())
    }

    pub fn finish(&mut self) {
        self.active = false;
    }
}

pub fn redact_secrets(text: &str) -> String {
    // Redact API key shapes, bearer tokens, and secrets
    let mut sanitized = text.to_string();
    for prefix in &["sk-", "ghp_", "xoxb-", "AIzaSy"] {
        if let Some(pos) = sanitized.find(prefix) {
            let end = sanitized[pos..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|p| pos + p)
                .unwrap_or(sanitized.len());
            if end > pos + prefix.len() {
                sanitized.replace_range(pos..end, "[REDACTED_SECRET]");
            }
        }
    }
    sanitized
}

pub fn load_cast_frames(path: impl AsRef<Path>) -> Result<Vec<CastFrame>, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut frames = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(frame) = serde_json::from_str::<CastFrame>(&line) {
            frames.push(frame);
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redaction() {
        let sample = "Authorization: Bearer sk-123456789abcdef in production";
        let masked = redact_secrets(sample);
        assert!(!masked.contains("sk-123456789abcdef"));
        assert!(masked.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn test_recorder_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("cast_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cast_path = temp_dir.join("session.rhocast");

        {
            let mut recorder = CastRecorder::start(&cast_path).unwrap();
            recorder.record_output("Hello world\n").unwrap();
            recorder.record_output("API key: sk-abcdef123456\n").unwrap();
            recorder.finish();
        }

        let frames = load_cast_frames(&cast_path).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].data, "Hello world\n");
        assert!(frames[1].data.contains("[REDACTED_SECRET]"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
