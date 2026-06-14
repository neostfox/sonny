pub mod openai;
pub mod traits;

use std::time::Duration;

use crate::config::Settings;
use crate::error::{MemoryError, MemoryResult};
use crate::models::embedding::EmbeddingSourceType;
use crate::store::traits::EmbeddingStore;

use self::openai::OpenAiCompatibleEmbeddingProvider;
use self::traits::EmbeddingProvider;

/// Construct the configured embedding provider from [`Settings`].
///
/// Embedding-specific `api_url`/`api_key` fall back to the LLM endpoint when empty — the
/// common case where one provider (or one local server) serves both chat and embeddings,
/// avoiding duplicated config.
pub fn build_embedding_service(settings: &Settings) -> MemoryResult<Box<dyn EmbeddingProvider>> {
    let e = &settings.embedding;
    let llm = &settings.llm;
    let base_url = if e.api_url.is_empty() {
        llm.api_url.as_str()
    } else {
        e.api_url.as_str()
    };
    let api_key = if e.api_key.is_empty() {
        llm.api_key.as_str()
    } else {
        e.api_key.as_str()
    };
    Ok(Box::new(OpenAiCompatibleEmbeddingProvider::new(
        base_url,
        api_key,
        &e.model_id,
        e.dim,
        Duration::from_secs(e.timeout_secs),
    )?))
}

/// Embed `text` and persist the resulting vector through an [`EmbeddingStore`].
///
/// This is the smallest P3-A end-to-end path: provider → vector → storage. P3-B
/// clustering can call this for Observation / ConceptCandidate / Concept inputs.
pub async fn embed_and_store<S: EmbeddingStore>(
    service: &dyn EmbeddingProvider,
    store: &S,
    source_type: EmbeddingSourceType,
    source_id: &str,
    workspace_id: &str,
    text: &str,
) -> MemoryResult<()> {
    let vector = service.embed(text).await?;
    let slice = vector.as_slice().ok_or_else(|| {
        MemoryError::Embedding("embedding vector storage expected contiguous Array1".into())
    })?;
    store.store_embedding(source_type.as_str(), source_id, workspace_id, text, slice)
}
