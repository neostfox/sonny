# Interface Layer Spec — llm/ and embed/

## Purpose

Define async traits for external service providers (LLM completion, text embedding). These are the abstraction boundaries that allow swapping implementations and mocking in tests.

## Directories

- `crates/memory-runtime/src/llm/` — LLM provider trait
- `crates/memory-runtime/src/embed/` — Embedding service trait

## Allowed Imports

- `async_trait::async_trait` — async trait support
- `crate::error::{MemoryError, MemoryResult}` — error propagation
- `serde::de::DeserializeOwned` — generic JSON deserialization
- `ndarray::Array1` — embedding vector type (embed only)

## Forbidden Imports

- `reqwest` — concrete HTTP client belongs in provider implementations, not trait definitions
- `rusqlite` — no database access in traits
- `tokio` runtime — traits don't spawn tasks

## Patterns

### LLM provider trait with JSON convenience method

```rust
// crates/memory-runtime/src/llm/traits.rs:7-22
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

- `complete()` — raw string completion
- `complete_json()` — default implementation that parses JSON with structured error
- `health_check()` — connectivity verification
- `name()` — provider identification for logging

### Embedding service trait with batch default

```rust
// crates/memory-runtime/src/embed/traits.rs:7-19
pub trait EmbeddingService: Send + Sync {
    fn embed(&self, text: &str) -> MemoryResult<Array1<f32>>;

    fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    fn dim(&self) -> usize { EMBEDDING_DIM }  // 512
    fn is_available(&self) -> bool;
}
```

- Synchronous API (embedding is CPU-bound)
- `embed_batch()` has sequential default impl; override for parallel models
- `EMBEDDING_DIM = 512` constant shared across store + service

### Mock provider for tests

```rust
// crates/memory-test-fixtures/src/mock_llm.rs:8-11
pub struct MockLlmProvider {
    responses: HashMap<String, String>,
    default_response: String,
    pub call_log: Mutex<Vec<String>>,
}
```

- Pattern-matched responses via `with_response(pattern, response)`
- `call_log` for assertions
- `Default` impl provided

## Anti-patterns

| Don't | Why | Instead |
|-------|-----|---------|
| Add HTTP-specific methods to trait | Not all providers use HTTP | Keep trait minimal, add provider-specific config in impl |
| Make traits synchronous for LLM | LLM calls are network-bound | Use `async_trait` |
| Hard-code embedding dimension | Different models have different dims | Use `EMBEDDING_DIM` constant + `dim()` method |
| Skip `Send + Sync` bounds | Stores need thread-safe providers | Always require `Send + Sync` on traits |

## Testing

- Use `MockLlmProvider` from `memory-test-fixtures` crate
- Verify `complete_json` error handling with malformed JSON
- Test batch embedding default implementation
