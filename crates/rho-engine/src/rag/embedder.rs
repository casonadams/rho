#[cfg(feature = "fastembed")]
use rig::embeddings::EmbeddingModel;
#[cfg(feature = "fastembed")]
use rig::fastembed::{Client, FastembedModel};
#[cfg(feature = "fastembed")]
use std::sync::Arc;

pub const EMBEDDING_DIM: usize = 384;

#[derive(Clone)]
pub enum EmbedderBackend {
    #[cfg(feature = "fastembed")]
    FastEmbed(Arc<rig::fastembed::EmbeddingModel>),
    Deterministic,
}

#[derive(Clone)]
pub struct LocalEmbedder {
    backend: EmbedderBackend,
}

impl Default for LocalEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalEmbedder {
    #[cfg(feature = "fastembed")]
    pub fn try_new_fastembed() -> Result<Self, String> {
        let client = Client::new();
        let model = client
            .embedding_model(&FastembedModel::BGESmallENV15)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            backend: EmbedderBackend::FastEmbed(Arc::new(model)),
        })
    }

    #[cfg(not(feature = "fastembed"))]
    pub fn try_new_fastembed() -> Result<Self, String> {
        Err("FastEmbed is not enabled in this build".to_string())
    }

    pub fn new_deterministic() -> Self {
        Self {
            backend: EmbedderBackend::Deterministic,
        }
    }

    pub fn new() -> Self {
        match Self::try_new_fastembed() {
            Ok(embedder) => embedder,
            Err(_) => Self::new_deterministic(),
        }
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        match &self.backend {
            #[cfg(feature = "fastembed")]
            EmbedderBackend::FastEmbed(model) => {
                let embeddings = model
                    .embed_texts(vec![text.to_string()])
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(emb) = embeddings.into_iter().next() {
                    Ok(emb.vec.into_iter().map(|v| v as f32).collect())
                } else {
                    Err("FastEmbed returned empty embeddings".to_string())
                }
            }
            EmbedderBackend::Deterministic => Ok(deterministic_embed(text, EMBEDDING_DIM)),
        }
    }

    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        match &self.backend {
            #[cfg(feature = "fastembed")]
            EmbedderBackend::FastEmbed(model) => {
                let embeddings = model.embed_texts(texts.to_vec()).await.map_err(|e| e.to_string())?;
                Ok(embeddings
                    .into_iter()
                    .map(|emb| emb.vec.into_iter().map(|v| v as f32).collect())
                    .collect())
            }
            EmbedderBackend::Deterministic => Ok(texts.iter().map(|t| deterministic_embed(t, EMBEDDING_DIM)).collect()),
        }
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let x = x as f64;
        let y = y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

fn fnv1a_hash(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed ^ 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn accumulate_token_3grams(bytes: &[u8], dim: usize, vec: &mut [f32]) {
    if bytes.len() < 3 {
        return;
    }
    for window in bytes.windows(3) {
        let g1 = fnv1a_hash(window, 0x85ebca6b);
        let g2 = fnv1a_hash(window, 0xc2b2ae35);
        let g_idx = (g1 as usize) % dim;
        let g_sign = if (g2 & 1) == 0 { 0.5f32 } else { -0.5f32 };
        vec[g_idx] += g_sign;
    }
}

fn accumulate_token_embedding(token: &str, dim: usize, vec: &mut [f32]) {
    let lower = token.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    let h1 = fnv1a_hash(bytes, 0x9e3779b97f4a7c15);
    let h2 = fnv1a_hash(bytes, 0x517cc1b727220a95);
    let idx = (h1 as usize) % dim;
    let sign = if (h2 & 1) == 0 { 1.0f32 } else { -1.0f32 };
    vec[idx] += sign * 2.0;

    accumulate_token_3grams(bytes, dim, vec);
}

fn normalize_l2(vec: &mut [f32]) {
    let norm_sq: f32 = vec.iter().map(|&v| v * v).sum();
    if norm_sq > 0.0 {
        let norm = norm_sq.sqrt();
        for val in vec {
            *val /= norm;
        }
    }
}

pub fn deterministic_embed(text: &str, dim: usize) -> Vec<f32> {
    if dim == 0 {
        return Vec::new();
    }
    let mut vec = vec![0.0f32; dim];

    let tokens = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty());

    for token in tokens {
        accumulate_token_embedding(token, dim, &mut vec);
    }

    normalize_l2(&mut vec);
    vec
}
