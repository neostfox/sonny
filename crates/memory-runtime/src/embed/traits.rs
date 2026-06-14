use async_trait::async_trait;
use ndarray::Array1;

use crate::error::MemoryResult;

/// Default embedding dimension (bge-m3 = 1024). Per-impl [`EmbeddingProvider::dim`] reports
/// the actual model dimension. Switching models changes `dim` and invalidates every stored
/// vector — a full re-embed is then required (analogous to P2-D reextract for observations).
pub const EMBEDDING_DIM: usize = 1024;

/// Generates dense vector embeddings for text. The OpenAI-compatible implementation
/// ([`crate::embed::openai`]) covers OpenAI cloud and any `/v1/embeddings`-speaking local
/// server (Ollama / Xinference / TEI / vLLM); switch endpoints via config, no code change.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text into a `dim()`-length vector.
    async fn embed(&self, text: &str) -> MemoryResult<Array1<f32>>;

    /// Embed multiple texts. The default calls [`embed`](Self::embed) sequentially; HTTP-backed
    /// impls override this to batch all inputs in one request.
    async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            out.push(self.embed(t).await?);
        }
        Ok(out)
    }

    /// Dimensionality of vectors produced by this service. MUST match the configured
    /// storage dimension; mismatches are rejected at embed time.
    fn dim(&self) -> usize;

    /// Probe the endpoint: reachable + auth valid + model exists. Does NOT validate `dim`
    /// (that is checked on real embeds). Returns `Ok(false)` on connection/HTTP failure.
    async fn health_check(&self) -> MemoryResult<bool>;

    /// Human-readable backend identifier (e.g. "openai-compatible").
    fn name(&self) -> &str;
}
