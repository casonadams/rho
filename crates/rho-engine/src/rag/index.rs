use futures::Future;
use rig::vector_store::{VectorSearchRequest, VectorStoreError, VectorStoreIndexDyn};
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

use super::embedder::{LocalEmbedder, cosine_similarity};
use super::store::CodebaseIndex;

pub struct CodebaseVectorIndex {
    index: Arc<CodebaseIndex>,
    embedder: Arc<LocalEmbedder>,
}

impl CodebaseVectorIndex {
    pub fn new(index: Arc<CodebaseIndex>, embedder: Arc<LocalEmbedder>) -> Self {
        Self { index, embedder }
    }
}

impl VectorStoreIndexDyn for CodebaseVectorIndex {
    fn top_n<'a>(
        &'a self,
        req: VectorSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<(f64, String, Value)>, VectorStoreError>> + Send + 'a>> {
        Box::pin(async move {
            let query = req.query();
            let query_emb = self
                .embedder
                .embed(query)
                .await
                .map_err(VectorStoreError::BuilderError)?;

            let mut scored: Vec<(f64, &super::chunker::CodeChunk)> = self
                .index
                .chunks
                .iter()
                .filter(|c| !c.embedding.is_empty())
                .map(|c| {
                    let sim = cosine_similarity(&query_emb, &c.embedding);
                    (sim, c)
                })
                .collect();

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            let threshold = req.threshold().unwrap_or(-1.0);
            let samples = req.samples() as usize;

            let results = scored
                .into_iter()
                .filter(|(s, _)| *s >= threshold)
                .take(samples)
                .map(|(score, chunk)| {
                    let val = serde_json::json!({
                        "file": chunk.file_path,
                        "lines": format!("{}-{}", chunk.start_line, chunk.end_line),
                        "snippet": chunk.content,
                    });
                    (score, chunk.id.clone(), val)
                })
                .collect();

            Ok(results)
        })
    }

    fn top_n_ids<'a>(
        &'a self,
        req: VectorSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<(f64, String)>, VectorStoreError>> + Send + 'a>> {
        Box::pin(async move {
            let docs = self.top_n(req).await?;
            Ok(docs.into_iter().map(|(score, id, _)| (score, id)).collect())
        })
    }
}
