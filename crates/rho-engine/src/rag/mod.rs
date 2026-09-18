pub mod chunker;
pub mod embedder;
pub mod index;
pub mod indexer;
pub mod store;

#[cfg(test)]
mod tests;

pub use chunker::{CodeChunk, chunk_text, hash_content};
pub use embedder::{LocalEmbedder, cosine_similarity, deterministic_embed};
pub use index::CodebaseVectorIndex;
pub use indexer::{IndexSummary, index_workspace};
pub use store::CodebaseIndex;
