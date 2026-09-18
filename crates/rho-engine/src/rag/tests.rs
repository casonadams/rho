use super::chunker::{chunk_text, hash_content};
use super::embedder::{LocalEmbedder, cosine_similarity, deterministic_embed};
use super::index::CodebaseVectorIndex;
use super::indexer::index_workspace;
use super::store::CodebaseIndex;
use rig::vector_store::{VectorSearchRequest, VectorStoreIndexDyn};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_chunk_text_basic_and_overlap() {
    let content = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10";
    let chunks = chunk_text("test.rs", content, 4, 1);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 4);
    assert_eq!(chunks[1].start_line, 4);
    assert_eq!(chunks[1].end_line, 7);
    assert_eq!(chunks[2].start_line, 7);
    assert_eq!(chunks[2].end_line, 10);
}

#[test]
fn test_hash_content_deterministic() {
    let h1 = hash_content("fn main() {}");
    let h2 = hash_content("fn main() {}");
    let h3 = hash_content("fn other() {}");
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}

#[test]
fn test_deterministic_embed_properties() {
    let emb1 = deterministic_embed("hello world agent", 384);
    let emb2 = deterministic_embed("hello world agent", 384);
    let emb3 = deterministic_embed("completely different text about astronomy", 384);

    assert_eq!(emb1.len(), 384);
    assert_eq!(emb1, emb2);

    let sim_same = cosine_similarity(&emb1, &emb2);
    let sim_diff = cosine_similarity(&emb1, &emb3);

    assert!((sim_same - 1.0).abs() < 1e-5);
    assert!(sim_diff < 0.8);
}

#[tokio::test]
async fn test_index_workspace_and_vector_search() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("auth.rs"),
        "pub fn authenticate_user(token: &str) -> bool {\n    !token.is_empty()\n}\n",
    )
    .unwrap();
    std::fs::write(
        src.join("math.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .unwrap();

    let embedder = LocalEmbedder::new_deterministic();
    let summary = index_workspace(root, false, &embedder).await.unwrap();

    assert_eq!(summary.files_indexed, 2);
    assert!(summary.total_chunks >= 2);
    assert_eq!(summary.new_chunks, summary.total_chunks);
    assert_eq!(summary.reused_chunks, 0);

    // Verify index file exists and can be loaded
    let index_file = CodebaseIndex::index_path(root);
    assert!(index_file.exists());
    let loaded = CodebaseIndex::load(&index_file).expect("loaded index");
    assert_eq!(loaded.chunks.len(), summary.total_chunks);

    // Incremental indexing: re-indexing should reuse chunks
    let summary2 = index_workspace(root, false, &embedder).await.unwrap();
    assert_eq!(summary2.reused_chunks, summary.total_chunks);
    assert_eq!(summary2.new_chunks, 0);

    // Test vector search
    let vector_index = CodebaseVectorIndex::new(Arc::new(loaded), Arc::new(embedder));
    let req = VectorSearchRequest::builder()
        .query("authenticate user token")
        .samples(1)
        .build();

    let results = vector_index.top_n(req).await.unwrap();
    assert_eq!(results.len(), 1);
    let (score, id, doc) = &results[0];
    assert!(*score > 0.3);
    assert!(id.contains("auth.rs"));
    assert!(
        doc.get("snippet")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("authenticate_user")
    );
}
