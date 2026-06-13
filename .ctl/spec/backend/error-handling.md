# Error Handling

## Error Type

All library errors flow through a single `thiserror`-derived enum:

```rust
// crates/memory-runtime/src/error.rs
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("LLM provider error: {0}")]
    LlmProvider(String),

    #[error("LLM returned invalid JSON: {source}")]
    LlmInvalidJson { source: serde_json::Error, raw: String },

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Clustering error: {0}")]
    Clustering(String),

    #[error("Entity not found: {entity_id}")]
    EntityNotFound { entity_id: String },

    #[error("Concept not found: {concept_id}")]
    ConceptNotFound { concept_id: String },

    #[error("Observation not found: {observation_id}")]
    ObservationNotFound { observation_id: String },

    #[error("Invalid status transition: {from} -> {to} for {object_type} {object_id}")]
    InvalidStatusTransition { from: String, to: String, object_type: String, object_id: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Session parse error: {details}")]
    SessionParse { details: String },

    #[error("Recall produced no results for workspace {workspace}")]
    EmptyRecall { workspace: String },
}
```

## Result Alias

```rust
pub type MemoryResult<T> = Result<T, MemoryError>;
```

Used throughout the crate. Never use bare `Result<T, _>`.

## Rules

### `?` propagation everywhere in library code

```rust
// Correct — from crates/memory-runtime/src/store/connection.rs:14
let conn = Connection::open(db_path)?;
```

### Never `unwrap()` in library code

`unwrap()` is only acceptable in `#[cfg(test)]` blocks. In production code, use `?` or explicit error mapping.

### CLI may `expect()` with user-facing messages

```rust
// Acceptable — from crates/sonny-cli/src/main.rs:53
let db = Database::open(&cli.db).expect("Failed to initialize database");
```

CLI is the top-level entry point. It may use `expect()` with descriptive messages since errors are unrecoverable at that point.

### LLM errors preserve raw output

```rust
// crates/memory-runtime/src/llm/traits.rs:16
serde_json::from_str(&raw).map_err(|e| MemoryError::LlmInvalidJson { source: e, raw })
```

The `raw` field captures the original LLM response for debugging. Never discard it.

### String-based error variants for external failures

`LlmProvider(String)`, `Embedding(String)`, `Clustering(String)` — external service errors where structured types would be artificial. Use `String` for opaque error messages from providers.

### Structured fields for domain errors

`EntityNotFound { entity_id }`, `InvalidStatusTransition { from, to, ... }` — domain errors use named fields for structured context.

## Adding New Error Variants

1. Add variant to `MemoryError` with `#[error("...")]` message
2. Use `#[from]` for automatic conversion from dependency types
3. Use named fields for domain-specific context
4. Use `String` for opaque external errors
