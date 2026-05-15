use ndarray::Array1;

use crate::error::MemoryResult;

pub const EMBEDDING_DIM: usize = 512;

pub trait EmbeddingService: Send + Sync {
    fn embed(&self, text: &str) -> MemoryResult<Array1<f32>>;

    fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    fn dim(&self) -> usize {
        EMBEDDING_DIM
    }

    fn is_available(&self) -> bool;
}
