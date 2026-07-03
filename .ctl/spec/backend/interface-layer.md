# Interface Layer

> Trait definitions and implementations for external services: LLM and Embedding providers.

## Purpose

Defines async provider traits that the pipeline and recall modules consume. Implementations handle HTTP communication, auth, and response parsing.

## Directory

`crates/memory-runtime/src/llm/` — 2 files (trait only)
`crates/memory-runtime/src/embed/` — 3 files (trait + OpenAI impl)

## Allowed Imports

- `crate::error::*` — error types
- `crate::config::*` — configuration (embed/mod.rs)
- `async_trait` — async trait support
- `reqwest` — HTTP client (openai.rs)
- `ndarray::Array1` — vector type (embed/)

## Forbidden Imports

- `crate::store::*` — no persistence in providers
- `crate::pipeline::*` — no business logic in providers

## LLM Provider (`llm/traits.rs`)

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> MemoryResult<String>;

    async fn complete_json<T: DeserializeOwned>(
        &self, prompt: &str, system: Option<&str>,
    ) -> MemoryResult<T> {
        let raw = self.complete(prompt, system).await?;
        serde_json::from_str(&raw).map_err(|e| MemoryError::LlmInvalidJson { source: e, raw })
    }

    async fn health_check(&self) -> MemoryResult<bool>;
    fn name(&self) -> &str;
}
```

- `complete_json()` has a default implementation that wraps `complete()` with JSON parsing.
- No real implementation exists — only `MockLlmProvider` in test fixtures.

## Embedding Provider (`embed/traits.rs`)

```rust
pub const EMBEDDING_DIM: usize = 1024;  // bge-m3 default

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> MemoryResult<Array1<f32>>;

    async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        // Default: sequential calls
    }

    fn dim(&self) -> usize;
    async fn health_check(&self) -> MemoryResult<bool>;
    fn name(&self) -> &str;
}
```

### OpenAI-Compatible Implementation (`embed/openai.rs`)

```rust
pub struct OpenAiCompatibleEmbeddingProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dim: usize,
}
```

- Talks `/v1/embeddings` protocol (OpenAI cloud, Ollama, Xinference, TEI, vLLM, LM Studio).
- `embed_batch()` sends all texts in one HTTP request.
- Configurable timeout, base_url, api_key, model, dim.

### Embed Service Builder (`embed/mod.rs`)

```rust
pub fn build_embedding_service(settings: &Settings) -> MemoryResult<Box<dyn EmbeddingProvider>> {
    // Falls back to LLM endpoint when embedding-specific fields are empty
}

pub async fn embed_and_store<S: EmbeddingStore>(
    service: &dyn EmbeddingProvider,
    store: &S,
    source_type: EmbeddingSourceType,
    source_id: &str,
    workspace_id: &str,
    text: &str,
) -> MemoryResult<()> {
    // End-to-end: provider → vector → store
}
```

## Configuration (`config.rs`)

```rust
pub struct Settings {
    pub llm: LlmConfig,           // api_url, api_key, model
    pub embedding: EmbeddingConfig, // api_url, api_key, model_id, dim, timeout_secs
    pub cluster: ClusterConfig,    // entity_weight, embedding_weight, distance_threshold
    pub recall: RecallConfig,      // max_tokens, semantic_weight, entity_weight, top_k
    pub confidence: ConfidenceConfig, // auto_confirm_threshold, ...
}
```

- Loaded from `~/.config/sonny/config.toml` with env override support.
- Embedding config falls back to LLM endpoint when empty (common: same server serves both).

## Anti-Patterns

### ❌ Provider implementations importing store

```rust
// ❌ Provider must not persist directly
impl OpenAiCompatibleEmbeddingProvider {
    pub async fn embed_and_store(&self, store: &impl EmbeddingStore, ...) { ... }
}
```

Instead: `embed_and_store()` is a free function in `embed/mod.rs` that composes provider + store.
