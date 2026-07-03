# Application Layer

> Business orchestration: the full growth pipeline (ingest → extract → cluster → merge) and the recall engine.

## Purpose

Orchestrates data flow between domain types, store traits, and external providers. No direct SQLite or HTTP calls — delegates to infrastructure and interface layers.

## Directory

`crates/memory-runtime/src/pipeline/` — 4 modules
`crates/memory-runtime/src/recall/` — 1 module (580 lines)
`crates/memory-runtime/src/recall/` — 1 module

## Allowed Imports

- `crate::models::*` — domain types
- `crate::store::traits::*` — store trait interfaces
- `crate::llm::traits::LlmProvider` — LLM provider
- `crate::embed::traits::EmbeddingProvider` — embedding provider
- `crate::entity::*` — entity normalization
- `crate::confidence::*` — confidence scoring
- `crate::error::*` — error types

## Forbidden Imports

- Direct `rusqlite::` usage — must go through store traits
- Direct `reqwest::` usage — must go through provider traits

## Pipeline Modules

### ingest.rs — Session Parsing

```rust
pub trait SessionParser: Send + Sync {
    fn parse(&self, content: &str, filename: &str, workspace_id: &str, source_ref: &str)
        -> MemoryResult<Vec<RawMemory>>;
}
```

Two implementations:
- **JournalParser**: Parses Trellis-style journal files with `## Session N` headers
- **JsonSessionParser**: Parses JSON arrays of `{role, content}` messages

**`detect_and_parse()`**: Auto-detects format from filename extension, returns `Vec<RawMemory>`.

### extract.rs — LLM Observation Extraction

- **`extract_observations()`**: Sends raw memories to LLM, parses JSON response, validates evidence (anti-hallucination gate), maps to `Observation` structs.
- **`extract_and_dedup()`**: Extract + filter against store duplicates (matches by normalized subject+predicate+object).
- **`reextract()`**: P2-D reverse correction — re-extract a session with current prompt, atomically supersede old observations, stamp prompt version.
- **`EXTRACTION_PROMPT_VERSION`**: `"2026-06-14.v1"` — bumped when system prompt changes.

Key constants:
- `EXTRACTION_SYSTEM_PROMPT`: Full LLM prompt defining observation schema and extraction rules.

### cluster.rs — HAC Clustering (P3-B)

```rust
pub struct ClusterEngine { config: ClusterConfig }
```

- **`combined_distance()`**: Weighted blend of entity Jaccard + embedding cosine distance.
- **`cluster()`**: Hierarchical Agglomerative Clustering using `linfa-clustering`. Produces `ObservationCluster` groups.
- **`cluster_to_candidate()`**: Converts each cluster into a `ConceptCandidate` with merged facts/evidence.
- Union-Find for cluster merging. Threshold-based cut.

### merge.rs — Candidate Merge/Split (P3-C)

```rust
pub struct MergeSplitEngine { config: MergeSplitConfig }
```

- **`merge_group()`**: Merges overlapping candidates via Jaccard similarity on entity sets and source terms.
- **`split_candidate()`**: Splits a candidate with heterogeneous observations into sub-groups.
- Union-Find for group detection. Jaccard threshold for merge eligibility.

## Recall Module

### recall/mod.rs — RecallEngine (P4-A)

```rust
pub struct RecallEngine<'a, C, E, P>
where
    C: ConceptStore,
    E: EmbeddingStore,
    P: EmbeddingProvider,
{
    concepts: &'a C,

**Three-stage recall pipeline:**

1. **Intent Classification** (`classify_intent()`): Rule-based bilingual keyword matching → `Intent` enum. No LLM call. Supports Chinese and English keywords per intent.
2. **Entity Matching** (`entity_recall()`): Extracts entities from query via `extract_query_entities()`, finds concepts via `find_by_entities()`.
3. **Semantic Search** (`semantic_recall()`): Embeds query via `EmbeddingProvider`, searches `EmbeddingStore` for top-K similar concepts.

**Key Methods:**
- `recall(query, workspace_id)` — Full pipeline with default 1500 token budget
- `recall_with_budget(query, workspace_id, max_tokens)` — Full pipeline with custom budget
- `rank(query, workspace_id)` — Returns `Vec<RecallScore>` sorted by composite score

**Scoring:**
- `SEMANTIC_WEIGHT = 0.6`, `ENTITY_WEIGHT = 0.4`
- `SEMANTIC_TOP_K = 10`, `SEMANTIC_THRESHOLD = 0.0`
- `total_score = semantic_score × 0.6 + entity_score × 0.4`
- `RecallScore` tracks both `semantic_score` and `entity_score` separately

**Context Building** (`build_context()`):
- Budget allocation via `RecallBudget::for_intent(intent, max_tokens)`: Intent-specific percentages (e.g., `VerifyFact`: 45% facts, 20% rejected, 10% task)
- `estimate_tokens()`: `text.chars().count().div_ceil(4).max(1)` — rough 4-chars-per-token heuristic
- `token_count` field on `MemoryContext` is enforced by `enforce_total_budget()`
- `enforce_total_budget()`: pops lowest-priority items when over budget (priority: entities → task_state → rejected → preferences → facts)
- `truncate_section()`: per-section truncation before global budget enforcement

**Entity Extraction** (`extract_query_entities()`):
- Splits on non-alphanumeric/non-Unicode chars, filters 2+ char tokens
- `canonical_key_light()` normalization + stopword filtering
- `entity_overlap()`: Jaccard similarity between query entities and concept's `related_entities_json`

**Post-Recall Updates:**
- Top 3 concepts get `update_recall_stats()` called (increments `recall_count`, updates `last_recalled_at`)
- Successful recall counted when `current_concept` is present
- `token_count` field on `MemoryContext` is enforced by `enforce_total_budget()`.

**Entity Extraction** (`extract_query_entities()`):
- Regex-based: splits on whitespace/punctuation, filters stopwords, normalizes case.
- `entity_overlap()`: Jaccard similarity between query entities and concept's `related_entities_json`.

## Anti-Patterns

### ❌ Bypassing store traits

```rust
// ❌ Direct SQL in pipeline
conn.execute("INSERT INTO observation ...", params![])?;
```

Instead: call `store.insert(&observation)?`.

### ❌ Hardcoded thresholds

```rust
// ❌ Magic numbers scattered in code
if score > 0.5 { ... }
```

Instead: use `SEMANTIC_THRESHOLD`, `SEMANTIC_WEIGHT`, `ENTITY_WEIGHT` constants at module top.
