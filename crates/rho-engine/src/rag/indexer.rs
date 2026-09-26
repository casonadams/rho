use std::collections::HashMap;
use std::path::Path;

use super::chunker::{CodeChunk, chunk_text};
use super::embedder::LocalEmbedder;
use super::store::CodebaseIndex;
use crate::tools::traversal::walker_builder;

pub struct IndexSummary {
    pub files_indexed: usize,
    pub total_chunks: usize,
    pub reused_chunks: usize,
    pub new_chunks: usize,
}

const CHUNK_LINES: usize = 40;
const OVERLAP_LINES: usize = 10;
const MAX_FILE_SIZE_BYTES: u64 = 500_000;

fn discover_workspace_files(workspace_dir: &Path) -> Vec<(String, String)> {
    let walker = walker_builder(workspace_dir, false).build();
    let mut files = Vec::new();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.components().any(|c| {
            let s = c.as_os_str();
            s == ".rho" || s == ".git" || s == "target" || s == "node_modules"
        }) {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && (meta.len() > MAX_FILE_SIZE_BYTES || meta.len() == 0)
        {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            let rel_path = path
                .strip_prefix(workspace_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            files.push((rel_path, content));
        }
    }

    files
}

struct PartitionedChunks {
    all_chunks: Vec<CodeChunk>,
    needing_embeddings: Vec<(usize, String)>,
    reused_count: usize,
}

fn partition_file_chunks(
    files: Vec<(String, String)>,
    existing_map: &HashMap<(String, String), CodeChunk>,
) -> PartitionedChunks {
    let mut all_chunks = Vec::new();
    let mut needing_embeddings = Vec::new();
    let mut reused_count = 0;

    for (rel_path, content) in files {
        let chunks = chunk_text(&rel_path, &content, CHUNK_LINES, OVERLAP_LINES);
        for mut chunk in chunks {
            if let Some(existing) = existing_map.get(&(chunk.file_path.clone(), chunk.content_hash.clone())) {
                chunk.embedding = existing.embedding.clone();
                reused_count += 1;
                all_chunks.push(chunk);
            } else {
                let idx = all_chunks.len();
                needing_embeddings.push((idx, chunk.content.clone()));
                all_chunks.push(chunk);
            }
        }
    }

    PartitionedChunks {
        all_chunks,
        needing_embeddings,
        reused_count,
    }
}

async fn compute_missing_embeddings(
    needing: &[(usize, String)],
    all_chunks: &mut [CodeChunk],
    embedder: &LocalEmbedder,
) -> Result<(), String> {
    if needing.is_empty() {
        return Ok(());
    }
    let texts: Vec<String> = needing.iter().map(|(_, t)| t.clone()).collect();
    for (batch_idx, chunk_slice) in texts.chunks(32).enumerate() {
        let batch_embeddings = embedder.embed_batch(chunk_slice).await?;
        for (i, emb) in batch_embeddings.into_iter().enumerate() {
            let (target_idx, _) = needing[batch_idx * 32 + i];
            all_chunks[target_idx].embedding = emb;
        }
    }
    Ok(())
}

pub async fn index_workspace(
    workspace_dir: &Path,
    force: bool,
    embedder: &LocalEmbedder,
) -> Result<IndexSummary, String> {
    let index_file = CodebaseIndex::index_path(workspace_dir);
    let existing_index = if force {
        None
    } else {
        CodebaseIndex::load_async(&index_file).await
    };

    let mut existing_map: HashMap<(String, String), CodeChunk> = HashMap::new();
    if let Some(existing) = existing_index {
        for chunk in existing.chunks {
            existing_map.insert((chunk.file_path.clone(), chunk.content_hash.clone()), chunk);
        }
    }

    let files_to_chunk = discover_workspace_files(workspace_dir);
    let files_indexed = files_to_chunk.len();
    let mut partitioned = partition_file_chunks(files_to_chunk, &existing_map);
    let new_chunks = partitioned.needing_embeddings.len();

    compute_missing_embeddings(&partitioned.needing_embeddings, &mut partitioned.all_chunks, embedder).await?;

    let total_chunks = partitioned.all_chunks.len();
    let new_index = CodebaseIndex {
        version: 1,
        model: "bge-small-en-v1.5".to_string(),
        chunks: partitioned.all_chunks,
    };

    new_index.save_async(&index_file).await?;

    Ok(IndexSummary {
        files_indexed,
        total_chunks,
        reused_chunks: partitioned.reused_count,
        new_chunks,
    })
}
