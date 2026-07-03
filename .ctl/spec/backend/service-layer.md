# Service Layer

> Cross-cutting domain logic: confidence scoring, entity normalization, and recall orchestration.

## Purpose

Pure domain logic that operates on models but has no I/O. Confidence computes beta-distribution updates. Entity normalization canonicalizes names. Recall orchestrates the three-stage retrieval pipeline.

## Directory

`crates/memory-runtime/src/confidence/` — 1 module
`crates/memory-runtime/src/entity/` — 1 module
`crates/memory-runtime/src/recall/` — 1 module (application layer, but orchestrates service logic)

## Allowed Imports

- `crate::models::*` — domain types
- `crate::store::traits::*` — for RecallEngine (store queries)
- `crate::embed::traits::*` — for RecallEngine (embedding search)
- `crate::error::*` — error types

## Confidence (`confidence/mod.rs`)

### BetaConfidence

```rust
pub struct BetaConfidence {
    pub alpha: f64,
    pub beta: f64,
}
```

- **`update(evidence_type)`**: Applies evidence weight to alpha/beta.
- **`mean()`**: `alpha / (alpha + beta)` — current confidence score.
- **`Default`**: `alpha=1.0, beta=1.0` — uniform prior (0.5 mean).

### EvidenceType (12 types)

Positive evidence: `UserConfirmation`, `FileEvidence`, `RepeatedOccurrence`, `CrossSession3Plus`, `CrossSession2`, `HumanReviewConfirm`, `RecallNotCorrected`.
Negative evidence: `UserNegation`, `ConflictingEvidence`, `RecallCorrected`, `InternalConflict`, `AssistantSpeculation`.

Each has (alpha_weight, beta_weight) in `EVIDENCE_WEIGHTS`. Test `all_evidence_types_have_weights` enforces full coverage.

## Entity Normalization (`entity/mod.rs`)

### canonical_key_light(raw) → String

Case + separator normalization only. `"UserService"` stays distinct from `"UserModel"`, but `"user_model"` and `"User Model"` both become `"user_model"`.

### canonical_key(raw) → String

Aggressive: also strips semantic suffixes (`_service`, `_table`, `_Impl`, etc.). Used for alias matching / clustering where different surface forms must collapse.

### Pipeline

1. NFKC Unicode normalization
2. Lowercase
3. Separator normalization (spaces/hyphens → underscores)
4. Underscore collapse
5. (Aggressive only) PascalCase suffix stripping → underscore suffix stripping

## RecallEngine (`recall/mod.rs`)

### Three-Stage Pipeline

1. **Intent Classification** (`classify_intent()`):
   - Rule-based: keyword matching on query text.
   - `ContinueInvestigation`: "继续", "what about", "how does"
   - `VerifyFact`: "是不是", "is it true", "confirm"
   - `CorrectMistake`: "错了", "wrong", "incorrect"
   - `AddKnowledge`: "记录", "remember", "note"
   - `ReviewHistory`: "之前", "previously", "history"
   - Default: `GeneralQuery`

2. **Entity Matching** (`entity_recall()`):
   - `extract_query_entities()`: Regex tokenization + stopword filtering.
   - `entity_overlap()`: Jaccard similarity between query entities and concept's `related_entities_json`.
   - Returns concepts where `entity_score > 0`.

3. **Semantic Search** (`semantic_recall()`):
   - Embeds query via `EmbeddingProvider`.
   - Searches `EmbeddingStore` for top-K similar embeddings.
   - Returns concepts with `semantic_score` from cosine similarity.

### Scoring

```rust
pub fn score_concept(
    concept: &Concept,
    query_entities: &[String],
    query_embedding: Option<&Array1<f32>>,
    concept_embedding: Option<&Array1<f32>>,
) -> RecallScore
```

- `total_score = SEMANTIC_WEIGHT * semantic_score + ENTITY_WEIGHT * entity_score`
- `SEMANTIC_WEIGHT = 0.6`, `ENTITY_WEIGHT = 0.4`
- Tiebreaker: higher confidence wins.

### Context Building

```rust
pub fn build_context(
    workspace_id: &str,
    intent: &Intent,
    concepts: &[Concept],
    max_tokens: usize,
) -> MemoryContext
```

- Allocates budget per section via `RecallBudget`.
- `token_count` is enforced: `enforce_total_budget()` pops lowest-priority items when over budget.
- `estimate_tokens()`: `text.chars().count().div_ceil(4)` — 4 chars ≈ 1 token heuristic.

## Anti-Patterns

### ❌ Side effects in confidence/entity functions

```rust
// ❌ Must not write to store
pub fn update(&mut self, evidence: EvidenceType, store: &impl ObservationStore) { ... }
```

Instead: confidence update is pure math. Caller persists the result.

### ❌ LLM calls in recall

Recall is fully local — intent classification is rule-based, entity matching is set operations, semantic search is embedding cosine. No LLM round-trips during recall.
