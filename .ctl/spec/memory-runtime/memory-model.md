# Memory Runtime — Object Model & Data Schema

## Object Hierarchy

```
RawMemory → Observation → ConceptCandidate → Concept → MemoryContext
                ↓                              ↑
        Hippocampal Buffer ──(consolidate)──→ Concept Store ─┘
                                         ↑                    ↑
                                         └─ Evidence ─────────┘
                                         └─ ConceptHierarchy ─┘
```

> **Removed: `MemoryItem`.** An earlier design had a standalone `MemoryItem` object between
> `Observation` and `ConceptCandidate` (decisions p1a/p1d). It has been **deleted**. The 9 memory
> types it carried are now folded into `Observation.memory_type_candidate` +
> `Observation.observation_detail_json` (see the Observation table and the **Memory Types**
> taxonomy below). There is no `MemoryItem → ConceptCandidate` pipeline stage; the consolidation
> engine grows candidates directly from clustered `Observation`s.

## Core Objects

### RawMemory

Raw experience from any source.

| Field | Type | Description |
|-------|------|-------------|
| memory_id | TEXT UNIQUE | UUID |
| workspace_id | TEXT | Workspace identifier |
| session_id | TEXT | Source session |
| role | TEXT | "user" \| "assistant" \| "system" |
| content | TEXT | Raw text content |
| source_type | TEXT | "session_file" \| "journal" \| "user_input" \| "manual" |
| source_ref | TEXT | File path or reference |
| extraction_version | TEXT | Prompt version used by the last extraction over this memory's session (enables detecting stale extractions) |
| created_at | TEXT | ISO 8601 |

> **Source types** map 1:1 to the `SourceType` enum in code (`SessionFile`, `Journal`,
> `UserInput`, `Manual`). Journal-format markdown is ingested by `JournalParser`. The former
> `trellis_task` source has no producer and was removed; the `Journal` value replaces the
> legacy `trellis_journal`.

### Observation

Structured fact extracted from raw memory by LLM. Confidence is **dual-dimension** (see
[quality-control.md](./quality-control.md) for the full model):

- `extraction_confidence` — fixed by `source_type` provenance at extraction time, **immutable**
  thereafter (`ObservationSourceType::extraction_confidence()`):
  `file_evidence`=0.9, `user_confirm`=0.85, `user_negation`=0.8, `user_message`=0.7,
  `assistant_guess`=0.3.
- `fact_confidence` — Beta posterior `evidence_alpha / (evidence_alpha + evidence_beta)`,
  updated by Validate/feedback over time.
- `effective_confidence` — `extraction_confidence × fact_confidence`
  (`Observation::effective_confidence()`), used for recall ranking. An untrustworthy source
  cannot reach high confidence even if repeatedly uncorrected.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| observation_id | TEXT UNIQUE | UUID | |
| workspace_id | TEXT | | |
| memory_id | TEXT | FK | Source raw memory |
| subject_text | TEXT NOT NULL | | Entity name (e.g., "POSMASK") |
| subject_type | TEXT | | Entity type (e.g., "database_table") |
| predicate | TEXT NOT NULL | | Canonical relation from the `Predicate` vocabulary: has \| not_has \| depends_on \| not_depends_on \| related_to \| not_related_to \| causes. Normalized at extraction via `normalize_predicate` (synonyms mapped; unknown predicates stored lowercased, never dropped). |
| object_text | TEXT | | Target entity or value |
| object_type | TEXT | | Target type |
| evidence_text | TEXT | | Verbatim supporting text |
| extraction_confidence | REAL | | Immutable extraction-time weight fixed by `source_type` (`ObservationSourceType::extraction_confidence()`) |
| evidence_alpha | REAL | 1.0 | Beta distribution alpha (positive evidence). Drives `fact_confidence`. |
| evidence_beta | REAL | 1.0 | Beta distribution beta (negative evidence). Drives `fact_confidence`. |
| status | TEXT | "candidate" | "candidate" \| "fast_stored" \| "confirmed" \| "auto_confirmed" \| "rejected" \| "deprecated" \| "disputed" \| "orphan" \| "superseded" |
| surprise_score | REAL | 0.5 | Predictive coding surprise level (0-1). Higher = more novel. |
| source_type | TEXT | | "user_message" \| "user_confirm" \| "user_negation" \| "assistant_guess" \| "file_evidence" |
| memory_type_candidate | TEXT | nullable | One of the 9 Memory Types (see taxonomy below). Replaces the deleted `MemoryItem.memory_type`. |
| observation_detail_json | TEXT | nullable | Type-specific structured detail for `memory_type_candidate` (JSON blob). Replaces `MemoryItem.content`. |
| consolidated | BOOLEAN | FALSE | Whether consolidation engine has processed this |
| extraction_batch_id | TEXT | nullable | Groups observations from a single `extract_observations()` call; enables coclaim co-occurrence queries for clustering |
| superseded_by | TEXT | nullable | Replacement's `extraction_batch_id` when this observation is re-extracted. Paired with `status = "superseded"`. |
| created_at | TEXT | | ISO 8601 |

### ConceptCandidate

Candidate concept grown from clustered observations. Uses a single Beta-posterior
`confidence` = `evidence_alpha / (evidence_alpha + evidence_beta)` (no extraction dimension —
candidates are aggregated, not extracted).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| candidate_id | TEXT UNIQUE | UUID | |
| workspace_id | TEXT | | |
| name | TEXT NOT NULL | | Human-readable concept name |
| summary | TEXT | | What this concept covers |
| source_terms_json | TEXT | | Entity names from observations |
| source_sessions_json | TEXT | | Session IDs contributing to this |
| source_observations_json | TEXT | | Observation IDs in this cluster |
| known_facts_json | TEXT | | Confirmed facts |
| rejected_hypotheses_json | TEXT | | Failed approaches |
| open_questions_json | TEXT | | Unresolved questions |
| evidence_json | TEXT | | Evidence chain |
| evidence_count | INTEGER | 0 | Number of supporting observations |
| confidence | REAL | 0.5 | Beta posterior: `evidence_alpha / (evidence_alpha + evidence_beta)` |
| evidence_alpha | REAL | 1.0 | |
| evidence_beta | REAL | 1.0 | |
| status | TEXT | "candidate" | |
| last_recalled_at | TEXT | | For vitality calculation |
| recall_count | INTEGER | 0 | Times recalled |
| successful_recall_count | INTEGER | 0 | Recalls not corrected by user |
| failed_recall_count | INTEGER | 0 | Recalls corrected by user |
| created_at | TEXT | | |
| updated_at | TEXT | | |

### Concept

Confirmed, stable concept for long-term use. Uses a single Beta-posterior `confidence` =
`evidence_alpha / (evidence_alpha + evidence_beta)`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| concept_id | TEXT UNIQUE | UUID | |
| workspace_id | TEXT | | |
| name | TEXT NOT NULL | | |
| concept_type | TEXT | | "architecture" \| "bug_fix" \| "troubleshooting" \| "data_asset" \| "task_state" \| "preference" |
| definition | TEXT | | Clear description |
| related_entities_json | TEXT | | |
| known_facts_json | TEXT | | |
| rejected_hypotheses_json | TEXT | | |
| open_questions_json | TEXT | | |
| evidence_json | TEXT | | |
| confidence | REAL | 0.5 | Beta posterior: `evidence_alpha / (evidence_alpha + evidence_beta)` |
| evidence_alpha | REAL | 1.0 | |
| evidence_beta | REAL | 1.0 | |
| status | TEXT | "active" | "candidate" \| "active" \| "labile" \| "deprecated" \| "disputed" |
| parent_concept_id | TEXT | FK nullable | Parent concept (hierarchy/chunking) |
| hierarchy_depth | INTEGER | 0 | Depth in hierarchy (0 = root) |
| last_recalled_at | TEXT | | |
| recall_count | INTEGER | 0 | |
| successful_recall_count | INTEGER | 0 | |
| failed_recall_count | INTEGER | 0 | |
| connection_count | INTEGER | 0 | Links to other concepts |
| created_at | TEXT | | |
| updated_at | TEXT | | |

## Embedding Tables

P3-A ships a SQLite BLOB-backed store as the working default. Vectors are 1024-dimensional
`f32` arrays from the configured `EmbeddingProvider` (default model: `BAAI/bge-m3`). The
provider contract rejects model/config dimension mismatches before storage.

### Current SQLite Store

```sql
CREATE TABLE embedding (
    source_type   TEXT NOT NULL,  -- 'observation' | 'concept' | 'concept_candidate'
    source_id     TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    text          TEXT,
    vector        BLOB NOT NULL,  -- little-endian f32 stream, 1024 * 4 = 4096 bytes by default
    created_at    TEXT NOT NULL,
    PRIMARY KEY (source_type, source_id)
) WITHOUT ROWID;

CREATE INDEX idx_embedding_workspace ON embedding(workspace_id);
```

`SqliteEmbeddingStore::search()` loads rows for the workspace and computes cosine similarity in Rust. This keeps P3-A dependency-light and end-to-end usable; sqlite-vec remains a future performance optimization behind the existing feature flag.

> The embedding **vectors** are `f32`; all **scoring / confidence / recall** values elsewhere are `f64`.

## Hippocampal Buffer (Complementary Learning)

Fast storage for new observations before consolidation. Prevents catastrophic overwriting.

```sql
CREATE TABLE hippocampal_buffer (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    observation_id  TEXT UNIQUE NOT NULL,
    workspace_id    TEXT NOT NULL,
    content_json    TEXT NOT NULL,      -- full observation data
    source_session  TEXT,
    stored_at       TEXT NOT NULL,      -- when entered buffer
    settle_at       TEXT NOT NULL,      -- when eligible for consolidation (stored_at + 24h)
    status          TEXT DEFAULT 'fast_stored',  -- 'fast_stored' | 'consolidating' | 'consolidated'
    created_at      TEXT
);

CREATE INDEX idx_hippo_settle ON hippocampal_buffer(settle_at, status);
```

**Lifecycle**: `fast_stored` (0-24h) → `consolidating` (during consolidation run) → `consolidated` (cleared by maintenance).

**Cooling period**: Default 24 hours. Configurable per workspace. Purpose: let observations "settle" before they can affect established concepts.

## Concept Hierarchy (Chunking)

Hierarchical parent-child relationships between concepts.

```sql
CREATE TABLE concept_hierarchy (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_concept_id   TEXT NOT NULL,
    child_concept_id    TEXT NOT NULL,
    hierarchy_type      TEXT NOT NULL,  -- 'is_subconcept_of' | 'is_part_of' | 'is_instance_of'
    strength            REAL DEFAULT 1.0,
    detected_at         TEXT,
    UNIQUE(parent_concept_id, child_concept_id)
);
```

**Detection rule**: If child.entities ⊂ parent.entities AND cosine_similarity(child, parent) > 0.7, then child is a subconcept of parent.

## Concept Network

Concept-to-concept relationships for spreading activation.

```sql
CREATE TABLE concept_relation (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    source_concept_id   TEXT NOT NULL,
    target_concept_id   TEXT NOT NULL,
    relation_type       TEXT NOT NULL,  -- 'shared_entity' | 'shared_session' | 'embedding_similarity' | 'temporal'
    strength            REAL NOT NULL,  -- [0, 1]
    created_at          TEXT,
    updated_at          TEXT,
    UNIQUE(source_concept_id, target_concept_id, relation_type)
);
```

**Relation strength calculation**:
- `shared_entity`: 0.4 * min(1, shared_count / 3)
- `embedding_similarity`: 0.3 * cosine_similarity if sim > 0.6
- `shared_session`: 0.2 * min(1, shared_sessions / 3)
- `temporal`: 0.1 if activity within 7 days

## Memory Types

These 9 types now live as **`Observation.memory_type_candidate`** values (taxonomy from design §4.3),
with type-specific structured detail in **`Observation.observation_detail_json`**. They are an
attribute of `Observation`, **not** a separate persisted object.

| Type | Purpose | Key Fields |
|------|---------|-----------|
| `architecture_memory` | Project structure and module relations | entities, relations |
| `bug_fix_memory` | Bug symptoms, root cause, fix | error_signature, root_cause, solution, rejected_causes |
| `troubleshooting_memory` | Diagnostic paths | problem, diagnosis_path, result |
| `data_asset_memory` | Tables, fields, data sources | entity, fields, known_facts, negative_facts |
| `user_preference_memory` | Long-term user preferences | scope, content |
| `rejected_hypothesis_memory` | Failed approaches | task, hypothesis, reason |
| `task_state_memory` | Long-running task progress | task, status, known, open_questions, next_actions |
| `decision_memory` | Architectural decisions | decision, rationale, alternatives |
| `project_context_memory` | Project-wide context | scope, constraints |

## Evidence Object

Embedded in evidence_json fields:

```json
{
    "source_type": "user_message",
    "source_id": "raw_mem_001",
    "session_id": "session_2024_01_15",
    "message_range": [3, 5],
    "evidence_text": "POSMASK 没有机器字段",
    "created_at": "2024-01-15T10:30:00Z",
    "confidence": 0.92,
    "status": "candidate"
}
```

## Status Lifecycle

```
fast_stored → candidate (after 24h cooling period + consolidation)
candidate → auto_confirmed (vitality > 0.80, sessions >= 3, no conflicts)
candidate → confirmed (human review)
candidate → deprecated (vitality < 0.40, age > 30 days)
candidate → disputed (conflicts >= supporting)
candidate → orphan (no matching concept during consolidation)
candidate → superseded (replaced by a newer extraction from reextract(); kept for traceability)
auto_confirmed → active (after promotion)
confirmed → active (after promotion)
active → labile (on recall — reconsolidation window)
labile → active (timeout or re-consolidation, possibly modified)
confirmed → deprecated (vitality drops below 0.40)
confirmed → disputed (new conflicting evidence)
deprecated → candidate (new supporting evidence)
```

Full Observation status set: `candidate`, `fast_stored`, `confirmed`, `auto_confirmed`,
`rejected`, `deprecated`, `disputed`, `orphan`, **`superseded`**.
