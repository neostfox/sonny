# Quality Guidelines

## Forbidden Patterns

### `unwrap()` in library code

```rust
// ❌ Forbidden in src/ (outside #[cfg(test)])
let conn = self.conn.lock().unwrap();
```

The `Mutex::lock().unwrap()` pattern is the one exception — it only panics on poison, which indicates a logic bug. All other `unwrap()` calls must be in `#[cfg(test)]`.

### `todo!()` or `unimplemented()` in non-stub code

Only `feedback/mod.rs` has a placeholder comment. Active modules must not contain `todo!()` macros.

### `panic!()` for recoverable errors

All recoverable error conditions must return `MemoryError`. Only truly impossible states may panic (e.g., regex `unwrap()` on compile-time constants, LazyLock initialization).

### Domain types importing infrastructure

`models/` must not import from `store/`, `llm/`, `embed/`, or `pipeline/`. Check with `cargo check` after adding imports.

### `Box<dyn Error>` instead of `MemoryError`

Never return `Box<dyn std::error::Error>`. Always use `MemoryResult<T>`.

### Manual regex compilation

Use `std::sync::LazyLock` for compiled regexes:

```rust
// ✅ Correct — from pipeline/ingest.rs
use std::sync::LazyLock;
static SESSION_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^session\s+(\d+)").unwrap());
```

## Required Patterns

### Derive standard traits on public types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation { ... }
```

### `Send + Sync` on all store and provider traits

```rust
pub trait ObservationStore: Send + Sync { ... }
pub trait EmbeddingProvider: Send + Sync { ... }
pub trait LlmProvider: Send + Sync { ... }
```

### `async_trait` for async trait methods

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> MemoryResult<String>;
}
```

### Column constants for SQL queries

```rust
const OBS_COLUMNS: &str = "observation_id, workspace_id, ...";
```

### Foreign key enforcement in SQLite

```rust
conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
```

Always enabled. Every open must set these pragmas.

### `INSERT OR IGNORE` for idempotent raw_memory ingestion

```rust
"INSERT OR IGNORE INTO raw_memory ..."
```

### `params![]` macro for SQL parameters

```rust
conn.execute(
    "INSERT INTO observation (...) VALUES (?1, ?2, ...)",
    params![obs.observation_id, obs.workspace_id, ...],
)?;
```

## Known Issues

### 🟠 High

| ID | Issue | File | Impact |
|----|-------|------|--------|
| H1 | `list_by_workspace` for observations has no LIMIT | `observation_store.rs` | Large workspace may be slow |
| H2 | `obs_params` / `candidate_params` heap-allocate 17-23 `Box<dyn ToSql>` per row | `observation_store.rs`, `concept_store.rs` | Batch insert overhead |
| H3 | `parse_observation_status` etc. silently default on invalid DB values | `observation_store.rs`, `concept_store.rs` | Corrupted state hidden |
| H4 | `update_recall_stats` doesn't check affected row count | `concept_store.rs` | Non-existent concept_id silently succeeds |
| H5 | `find_by_entities` uses `LIKE '%?%'` on JSON columns | `concept_store.rs` | False positives: "user" matches "user_profile" |

### 🟡 Medium

| ID | Issue | File | Impact |
|----|-------|------|--------|
| M1 | `MemoryItem` table dropped in migration 003, but initial migration still creates it | `001_initial.sql` | Dead DDL in fresh DB |
| M2 | `repair_json` only handles markdown fence ```json | `extract.rs` | Other malformed LLM output not handled |
| M3 | `open_in_memory()` sets WAL PRAGMA | `connection.rs` | Harmless but misleading |
| M4 | `dirs_home()` uses `$HOME` which may not exist on Windows | `config.rs` | Falls back to temp dir |
| M5 | `ConceptCandidate` status enum is `CandidateStatus` which includes `Merged`/`Split` — valid but no pipeline triggers these | `models/status.rs` | States exist but no code path uses them |

## Naming Conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Crate name | kebab-case | `memory-runtime`, `sonny-cli` |
| Module file | snake_case | `raw_memory_store.rs` |
| Struct / Enum | PascalCase | `Observation`, `ObservationStatus` |
| Enum variant | PascalCase | `FastStored`, `AutoConfirmed` |
| Function / method | snake_case | `extract_observations`, `canonical_key` |
| Constant | SCREAMING_SNAKE | `EMBEDDING_DIM`, `EXTRACTION_PROMPT_VERSION` |
| Type alias | PascalCase | `MemoryResult<T>` |
| SQL table | snake_case | `raw_memory`, `concept_candidate` |
| SQL column | snake_case | `observation_id`, `workspace_id` |
| Test function | snake_case with `test_` prefix | `test_extract_with_mock_llm` |

## Build Commands

| Command | Purpose |
|---------|---------|
| `cargo check` | Fast type-check without codegen |
| `cargo test` | Run all tests (72 total) |
| `cargo test` | Run all tests (72+ total) |
| `cargo test -p memory-test-fixtures` | Test fixtures crate |
| `cargo clippy -- -D warnings` | Lint with zero warnings |
| `cargo build` | Full build |
| `cargo run -p sonny-cli -- --help` | Run CLI |
