# Domain Layer

> Pure data types with no I/O imports. All types derive `Debug, Clone, Serialize, Deserialize`.

## Purpose

Defines the core vocabulary of the memory system: observations, concepts, candidates, embeddings, predicates, statuses, recall context, and confidence.

## Directory

`crates/memory-runtime/src/models/` — 10 files, 0 I/O dependencies.

## Allowed Imports

- `serde::{Deserialize, Serialize}`
- `super::status::*` (cross-model references within domain)
- `crate::confidence::EvidenceType` (observation.rs only — pure domain)

## Forbidden Imports

- `crate::store::*` — no persistence awareness
- `crate::pipeline::*` — no orchestration
- `crate::llm::*` — no external service
- `crate::embed::*` — no embedding service

## Models

### Observation (`observation.rs`)

The central knowledge unit. Subject-predicate-object triple with evidence-backed confidence.

```rust
pub struct Observation {
    pub observation_id: String,
    pub workspace_id: String,
    pub memory_id: String,              // FK to raw_memory
    pub subject_text: String,
    pub subject_type: Option<String>,
    pub predicate: String,              // Canonical via normalize_predicate()
    pub object_text: Option<String>,
    pub object_type: Option<String>,
    pub evidence_text: Option<String>,  // Must be verbatim substring of source
    pub extraction_confidence: f64,     // Immutable after extraction
    pub evidence_alpha: f64,            // Beta distribution params
    pub evidence_beta: f64,
    pub status: ObservationStatus,
    pub surprise_score: f64,
    pub source_type: ObservationSourceType,
    pub consolidated: bool,
    pub memory_type_candidate: Option<MemoryType>,
    pub observation_detail_json: Option<String>,
    pub extraction_batch_id: Option<String>,  // P2-C: groups extractions
    pub superseded_by: Option<String>,        // P2-D: points to replacement batch
    pub created_at: String,
}
```

**MemoryType taxonomy** (§4.3 of design doc): `preference`, `task_state`, `architecture`, `troubleshooting`, `data_asset`, `bug_fix`.

### Concept (`concept.rs`)

A named, persistent knowledge unit that accumulates evidence across sessions.

```rust
pub struct Concept {
    pub concept_id: String,
    pub workspace_id: String,
    pub name: String,
    pub concept_type: Option<ConceptType>,  // architecture, bug_fix, etc.
    pub definition: Option<String>,
    pub related_entities_json: Option<String>,
    pub known_facts_json: Option<String>,
    pub rejected_hypotheses_json: Option<String>,
    pub open_questions_json: Option<String>,
    pub evidence_json: Option<String>,
    pub confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: ConceptStatus,
    pub parent_concept_id: Option<String>,  // Hierarchy
    pub hierarchy_depth: i64,
    pub last_recalled_at: Option<String>,
    pub recall_count: i64,
    pub successful_recall_count: i64,
    pub failed_recall_count: i64,
    pub connection_count: i64,
    pub created_at: String,
    pub updated_at: String,
}
```

### ConceptCandidate (`concept.rs`)

Intermediate entity between clustered observations and promoted concepts. Uses its own `CandidateStatus` lifecycle.

### Predicate (`predicate.rs`)

Canonical relation vocabulary. 7 variants: `Has`, `NotHas`, `DependsOn`, `NotDependsOn`, `RelatedTo`, `NotRelatedTo`, `Causes`. Unknown predicates are normalized (lowercase + spaces→underscores) rather than dropped.

### Status Enums (`status.rs`)

Three independent status machines:

- **ObservationStatus**: `Candidate` → `FastStored` → `Confirmed` / `AutoConfirmed` / `Rejected` / `Deprecated` / `Disputed` / `Orphan` / `Superseded`
- **ConceptStatus**: `Candidate` → `Active` → `Labile` / `Deprecated` / `Disputed`
- **CandidateStatus**: `Candidate` → `Active` / `Merged` / `Split`

### Recall Types (`recall.rs`)

- **MemoryContext**: Structured output for LLM consumption. Sections: `user_preferences`, `known_facts`, `rejected_hypotheses`, `task_state`, `relevant_entities`. `token_count` field is enforced by `build_context()`.
- **Intent**: `ContinueInvestigation`, `VerifyFact`, `CorrectMistake`, `AddKnowledge`, `ReviewHistory`, `GeneralQuery`.
- **RecallBudget**: Per-section token allocation (preferences 15%, facts 30%, rejected 10%, task 25%, entities 20%).
- **RecallScore**: Composite score with `semantic_score` (0.6 weight) + `entity_score` (0.4 weight).

### Embedding Types (`embedding.rs`)

- **EmbeddingSourceType**: `Observation`, `Concept`, `ConceptCandidate`
- **EmbeddingSearchResult**: `source_id`, `source_type`, `score`

### Hierarchy Types (`hierarchy.rs`)

- **HierarchyType**: `IsSubconceptOf`, `IsPartOf`, `IsInstanceOf`
- **RelationType**: `SharedEntity`, `SharedSession`, `EmbeddingSimilarity`, `Temporal`

### Evidence (`evidence.rs`)

Simple struct tracking provenance: `source_type`, `source_id`, `session_id`, `evidence_text`, `confidence`.

### Feedback Types (`feedback.rs`)

- **FeedbackType**: `Confirm`, `Negate`, `Supplement`, `Correct`, `Preference`, `General`
- **FeedbackResult**: Result of applying feedback (type, concept_id, new_confidence, status_changed)

## Anti-Patterns

### ❌ Adding I/O to models

```rust
// ❌ Domain type must not know about storage
impl Observation {
    pub fn save(&self, conn: &Connection) -> MemoryResult<()> { ... }
}
```

Instead: store traits in `store/traits.rs` accept domain types.

### ❌ Mutable state in domain types

Models are data carriers. State transitions are handled by pipeline/store logic, not by methods on the types themselves.
