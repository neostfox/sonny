# Infrastructure Layer

> SQLite persistence, migrations, and store trait implementations.

## Purpose

Implements the store traits defined in `store/traits.rs` using SQLite. Provides connection management, schema migration, and CRUD operations for all domain types.

## Directory

`crates/memory-runtime/src/store/` — 7 files
`crates/memory-runtime/src/migrations/` — 6 SQL files

## Allowed Imports

- `crate::models::*` — domain types
- `crate::error::*` — error types
- `rusqlite::*` — SQLite driver
- `parking_lot::Mutex` — connection mutex

## Forbidden Imports

- `crate::pipeline::*` — no business logic in stores
- `crate::llm::*` — no external services
- `crate::embed::*` — no embedding service

## Connection Management (`connection.rs`)

```rust
pub struct Database {
    pub conn: Arc<Mutex<Connection>>,
    pub has_vec: bool,
}
```

- `Arc<Mutex<Connection>>`: Shared ownership, parking_lot mutex (no poisoning).
- `open()` / `open_in_memory()`: Both set `PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;`
- Multiple stores can share one `Database` instance (unlike early versions where `Connection` was moved).

## Store Traits (`traits.rs`)

Four traits, all `Send + Sync`:

| Trait | Key Methods |
|-------|------------|
| `RawMemoryStore` | `insert`, `insert_batch`, `get_by_session`, `list_by_workspace`, `session_extraction_version`, `set_session_extraction_version` |
| `ObservationStore` | `insert`, `insert_batch`, `get`, `list_by_workspace`, `update_status`, `update_confidence`, `find_by_entity`, `find_coclaim`, `find_duplicate`, `replace_session_observations` |
| `ConceptStore` | `insert_candidate`, `get_candidate`, `list_candidates`, `insert_concept`, `get_concept`, `list_concepts`, `update_concept`, `find_by_entities`, `record_recall`, `record_recall_outcome` |
| `EmbeddingStore` | `store_embedding`, `search`, `get_embedding`, `delete` |

## Implementations

### SqliteRawMemoryStore (`raw_memory_store.rs`)

- `INSERT OR IGNORE` for idempotent ingestion
- `session_extraction_version` / `set_session_extraction_version` for P2-D

### SqliteObservationStore (`observation_store.rs`)

- **`find_coclaim()`**: Joins `observation_coclaim` table, excludes dead statuses (superseded/rejected/deprecated).
- **`find_duplicate()`**: Matches by normalized (subject, predicate, object) in workspace, excludes dead statuses.
- **`replace_session_observations()`**: Atomic transaction — supersede old + insert new in one commit.

### SqliteConceptStore (`concept_store.rs`)

- **`find_by_entities()`**: Uses `entity_concept` JOIN table for exact entity matching.
- **`record_recall()`**: increments `recall_count`, updates `last_recalled_at` (attempt only). **`record_recall_outcome()`**: increments `successful_recall_count`/`failed_recall_count` from explicit feedback (P4-B). Both return `ConceptNotFound` on zero affected rows (H4 resolved).
- JSON fields: `known_facts_json`, `rejected_hypotheses_json`, `open_questions_json`, `evidence_json` stored as TEXT.

### SqliteEmbeddingStore (`embedding_store.rs`)

- **BLOB storage**: Vectors stored as little-endian f32 bytes.
- **Cosine search**: Loads workspace vectors into memory, computes cosine similarity in Rust. No sqlite-vec required.
- **`EmbeddingSearchResult`**: Returns `source_id`, `source_type`, `score`.

## Migrations (`migration.rs`)

Versioned via SQLite `PRAGMA user_version`. Run on every `Database::open()`.

| Version | File | Changes |
|---------|------|---------|
| 1 | `001_initial.sql` | raw_memory, observation, concept_candidate, concept, entity_alias |
| 2 | `002_entity_concept.sql` | entity_concept JOIN table |
| 3 | `003_p1_model_alignment.sql` | extraction_confidence rename, memory_type_candidate, DROP memory_item |
| 4 | `004_p2c_observation_coclaim.sql` | extraction_batch_id + observation_coclaim table |
| 5 | `005_p2d_reverse_correction.sql` | extraction_version + superseded_by columns |
| 6 | `006_p3a_embedding_storage.sql` | embedding BLOB table |

## Anti-Patterns

### ❌ Business logic in store implementations

```rust
// ❌ Stores must not decide WHAT to store, only HOW
impl SqliteObservationStore {
    pub fn auto_confirm_stale(&self) -> MemoryResult<()> { ... }
}
```

Instead: pipeline layer decides business rules, store layer executes queries.

### ❌ Column list duplication

```rust
// ❌ String duplicated across queries
"INSERT INTO observation (observation_id, workspace_id, ...) VALUES (?1, ?2, ...)"
// later:
"SELECT observation_id, workspace_id, ... FROM observation WHERE ..."
```

Instead: define column constants and reuse.

### ❌ Unchecked transactions

```rust
// ❌ unchecked_transaction() doesn't verify autocommit state
let tx = conn.unchecked_transaction()?;
```

Prefer `transaction()` when possible; `unchecked_transaction()` is acceptable when the connection is guaranteed to be in autocommit (single-threaded access via Mutex).
