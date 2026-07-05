# Cross-Layer Thinking Guide

When making changes that span multiple modules, consider the following:

## Current Implementation Status

| Stage | Status | Key Files |
|-------|--------|-----------|
| Observe (Ingest) | ✅ Complete | `pipeline/ingest.rs` |
| Extract | ✅ Complete | `pipeline/extract.rs` |
| Validate (Confidence) | ✅ Complete | `confidence/mod.rs` |
| Embed | ✅ Complete (P3-A) | `embed/`, `store/embedding_store.rs` |
| Cluster | ✅ Complete (P3-B) | `pipeline/cluster.rs` |
| Merge/Split | ✅ Complete (P3-C) | `pipeline/merge.rs` |
| Recall | ✅ Complete (P4-A) | `recall/mod.rs` |
| Feedback | ⏳ Planned (P4-B) | `feedback/mod.rs` (stub) |

## Recall Pipeline Checklist

P4-A is implemented. For changes to recall:

1. **Intent classification** is rule-based — no LLM calls. Modify keywords in `classify_intent()` (bilingual: Chinese + English).
2. **Entity matching** uses `entity_overlap()` (Jaccard on `canonical_key_light`-normalized query entities vs concept's `related_entities_json`).
3. **Semantic search** embeds query via `EmbeddingProvider`, searches `EmbeddingStore` for top-K.
4. **Scoring** weights: `SEMANTIC_WEIGHT=0.6`, `ENTITY_WEIGHT=0.4`. `RecallScore` tracks both channels.
5. **Token budget** is dynamic per `Intent`: `RecallBudget::for_intent(intent, max_tokens)`. `enforce_total_budget()` pops lowest-priority items.
6. **RecallStats** — top 3 concepts get `ConceptStore::record_recall()` (attempt count + forgetting-clock reset only); success/failure is resolved later by explicit feedback via `record_recall_outcome()` (P4-B closed loop).
7. **`MemoryContext.token_count`** reflects actual output tokens (estimated).

Key constants in `recall/mod.rs`:
```rust
const DEFAULT_MAX_TOKENS: usize = 1500;
const SEMANTIC_WEIGHT: f64 = 0.6;
const ENTITY_WEIGHT: f64 = 0.4;
const SEMANTIC_TOP_K: usize = 10;
const SEMANTIC_THRESHOLD: f32 = 0.0;
```

For changes to recall that touch store traits:
- `ConceptStore::find_by_entities()` — entity match
- `ConceptStore::list_concepts()` — load candidates
- `ConceptStore::record_recall()` / `record_recall_outcome()` — recall attempt vs. feedback-resolved outcome (P4-B)
- `EmbeddingStore::search()` — semantic search
- `EmbeddingProvider::embed()` — query embedding

Priority order for `enforce_total_budget()`:
1. `relevant_entities` (popped first)
2. `task_state`
3. `rejected_hypotheses`
4. `user_preferences`
5. `known_facts` (popped last)
For changes to recall that touch store traits:
- `ConceptStore::find_by_entities()` — entity match
- `ConceptStore::list_concepts()` — load candidates
- `EmbeddingStore::search()` — semantic search
- `EmbeddingProvider::embed()` — query embedding

## Growth Pipeline Checklist

For changes to the growth pipeline:

### Cluster Engine (P3-B)

- `ClusterEngine::cluster()` takes `Vec<ClusteredObservation>` + embeddings → `Vec<ObservationCluster>`.
- `combined_distance()` blends entity Jaccard + embedding cosine.
- Uses `linfa-clustering` for HAC. Threshold-based cut produces variable cluster count.
- `cluster_to_candidate()` converts each cluster to a `ConceptCandidate`.

### Merge/Split Engine (P3-C)

- `MergeSplitEngine::merge_group()` merges overlapping candidates via Jaccard on entity sets.
- `split_candidate()` splits heterogeneous candidates.
- Union-Find for group detection. Both return `MergeResult` with candidates + stats.

### Extraction Pipeline

- `extract_observations()` → LLM call → `Vec<Observation>`.
- `extract_and_dedup()` → extract + filter duplicates (normalized subject+predicate+object).
- `reextract()` → P2-D: atomic supersede + re-extract + stamp prompt version.
- Anti-hallucination gate: `evidence_is_supported()` checks verbatim substring match.

## Adding a New Domain Type

1. Define the struct/enum in `models/` with standard derives
2. Add corresponding SQL table in `migrations/NNN_name.sql`
3. Update `migration.rs` to include the new migration
4. Add trait methods to `store/traits.rs`
5. Implement trait in a new `store/<name>_store.rs` file
6. Re-export from `store/mod.rs`

Checklist:
- [ ] Model has `Debug, Clone, Serialize, Deserialize`
- [ ] SQL table has appropriate indexes
- [ ] Store trait has `Send + Sync`
- [ ] Row-mapping helper handles `Option<T>` fields correctly
- [ ] Parse function for string → enum in store layer
- [ ] Integration test for CRUD round-trip

## Adding a New Pipeline Stage

1. New file in `pipeline/<name>.rs`
2. Accept trait parameters (stores, LLM) — don't import concrete impls
3. Return `MemoryResult<Vec<DomainType>>`
4. Register in `pipeline/mod.rs`
5. Wire into CLI in `sonny-cli/src/main.rs`

Checklist:
- [ ] Function takes `impl Trait` not `ConcreteType`
- [ ] No `rusqlite` imports in pipeline
- [ ] Error cases use `MemoryError` variants
- [ ] Unit test with mock LLM
- [ ] Integration test with in-memory DB

## Adding a New Provider

1. New file in `llm/` or `embed/` for the implementation
2. Implement the existing trait (`LlmProvider` or `EmbeddingProvider`)
3. Add constructor with configuration from `config.rs`
4. Add feature flag in `Cargo.toml` if dependency is heavy

Checklist:
- [ ] Implements all trait methods
- [ ] `Send + Sync` satisfied
- [ ] Errors wrapped in `MemoryError::*Error` variants
- [ ] Feature-gated if dependency is optional

## Modifying the Schema

1. Create new migration `migrations/NNN_name.sql` with next version number
2. Add to `MIGRATIONS` array in `migration.rs`
3. Update store implementation if columns changed
4. Update model struct if fields changed
5. Run `cargo test` to verify migration from scratch

Checklist:
- [ ] Migration is idempotent (`CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`)
- [ ] Down migration not needed (append-only by convention)
- [ ] Existing data preserved (ADD COLUMN, not ALTER COLUMN type)
- [ ] Integration tests pass with fresh DB

## Dependency Direction Check

Before adding any `use` statement:

```
Allowed directions:
  CLI → Pipeline → Models
  CLI → Store → Models
  Recall → Store (traits)
  Recall → Embed (traits)
  Recall → Models
  Pipeline → LLM (trait)
  Pipeline → Store (traits)
  Pipeline → Models
  Pipeline → Entity (pure function)
  Pipeline → Confidence (pure function)
  Store → Models
  Entity → Models (pure)
  Confidence → Models (pure)
  Embed → Config (Settings)

Forbidden:
  Models → anything (must be pure)
  Store → Pipeline
  LLM → Store
  Pipeline → Store (concrete impls — use traits as params)
  Embed → Store (use embed_and_store() free function)
```

## Testing Strategy

| Component | Test Type | Fixture |
|-----------|-----------|---------|
| Ingest (parser) | Unit tests in `pipeline/ingest.rs` | Raw strings |
| Extract (LLM) | Unit + integration with `MockLlmProvider` | `memory-test-fixtures` |
| Cluster | Unit tests in `pipeline/cluster.rs` | Synthetic observations |
| Merge/Split | Unit tests in `pipeline/merge.rs` | Synthetic candidates |
| Recall | Unit tests in `recall/mod.rs` | `StubEmbeddingService` + in-memory DB |
| Store impls | Integration tests in `tests/integration_test.rs` | `Database::open_in_memory()` |
| Confidence | Unit tests in `confidence/mod.rs` | Pure math |
| Entity | Unit tests in `entity/mod.rs` | String inputs |

Key patterns:
- `MockLlmProvider`: Pattern-matched responses, call log via `Mutex<Vec<String>>`.
- `StubEmbeddingService`: FNV-1a hash → deterministic vector. Same text → same vector. Network-free.
- `Database::open_in_memory()`: Fresh SQLite per test. No shared state.
- `#[tokio::test]` for async tests (extraction, embedding, recall).
