use async_trait::async_trait;
use memory_runtime::embed::traits::EmbeddingProvider;
use memory_runtime::error::MemoryResult;
use ndarray::Array1;

/// Deterministic, network-free embedding service for tests. Maps text to a stable vector via
/// an FNV-1a hash folded into a `dim`-length, L2-normalized vector. Same text → same vector;
/// distinct texts are near-orthogonal in expectation, so cosine search behaves realistically.
///
/// Used by pipeline integration tests (embed → store → search) without hitting any server.
pub struct StubEmbeddingService {
    dim: usize,
}

impl StubEmbeddingService {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

fn hash_vector(text: &str, dim: usize) -> Array1<f32> {
    let mut out = Array1::<f32>::zeros(dim);
    // FNV-1a offset basis / prime.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        out[(h as usize) % dim] += 1.0;
    }
    let norm = out.dot(&out).sqrt();
    if norm > 0.0 {
        out /= norm;
    }
    out
}

#[async_trait]
impl EmbeddingProvider for StubEmbeddingService {
    async fn embed(&self, text: &str) -> MemoryResult<Array1<f32>> {
        Ok(hash_vector(text, self.dim))
    }

    async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        Ok(texts.iter().map(|t| hash_vector(t, self.dim)).collect())
    }

    fn dim(&self) -> usize {
        self.dim
    }

    async fn health_check(&self) -> MemoryResult<bool> {
        Ok(true)
    }

    fn name(&self) -> &str {
        "stub"
    }
}
