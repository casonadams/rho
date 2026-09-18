use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeChunk {
    pub id: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub content_hash: String,
    #[serde(default)]
    pub embedding: Vec<f32>,
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn chunk_text(file_path: &str, content: &str, chunk_size: usize, overlap: usize) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let effective_chunk = chunk_size.max(1);
    let effective_overlap = overlap.min(effective_chunk.saturating_sub(1));
    let step = (effective_chunk - effective_overlap).max(1);

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < lines.len() {
        let end = (start + effective_chunk).min(lines.len());
        let chunk_lines = &lines[start..end];
        let chunk_content = chunk_lines.join("\n");
        let content_hash = hash_content(&chunk_content);
        let start_line = start + 1;
        let end_line = end;
        let id = format!("{file_path}:{start_line}-{end_line}");

        chunks.push(CodeChunk {
            id,
            file_path: file_path.to_string(),
            start_line,
            end_line,
            content: chunk_content,
            content_hash,
            embedding: Vec::new(),
        });

        if end == lines.len() {
            break;
        }
        start += step;
    }

    chunks
}
