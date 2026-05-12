# Memory Runtime — Object Model & Data Schema

## Object Hierarchy

```
RawMemory → Observation → MemoryItem → ConceptCandidate → Concept → MemoryContext
                ↓                                           ↑
        Hippocampal Buffer ──(consolidate)──→ Concept Store ─┘
                                         ↑                    ↑
                                         └─ Evidence ─────────┘
                                         └─ ConceptHierarchy ─┘
```

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
| source_type | TEXT | "session_file" \| "trellis_journal" \| "trellis_task" \| "user_input" \| "manual" |
| source_ref | TEXT | File path or reference |
| created_at | TEXT | ISO 8601 |

### Observation

Structured fact extracted from raw memory by LLM.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| observation_id | TEXT UNIQUE | UUID | |
| workspace_id | TEXT | | |
| memory_id | TEXT | FK | Source raw memory |
| subject_text | TEXT NOT NULL | | Entity name (e.g., "POSMASK") |
| subject_type | TEXT | | Entity type (e.g., "database_table") |
| predicate | TEXT NOT NULL | | Relation (e.g., "not_has_field") |
| object_text | TEXT | | Target entity or value |
| object_type | TEXT | | Target type |
| evidence_text | TEXT | | Verbatim supporting text |
| confidence | REAL | 0.5 | Materialized: alpha / (alpha + beta) |
| evidence_alpha | REAL | 1.0 | Beta distribution alpha (positive evidence) |
| evidence_beta | REAL | 1.0 | Beta distribution beta (negative evidence) |
| status | TEXT | "candidate" | "candidate" \| "fast_stored" \| "confirmed" \| "auto_confirmed" \| "rejected" \| "deprecated" \| "disputed" \| "orphan" |
| surprise_score | REAL | 0.5 | Predictive coding surprise level (0-1). Higher = more novel. |
| source_type | TEXT | | "user_message" \| "user_confirm" \| "user_negation" \| "assistant_guess" \| "file_evidence" |
| consolidated | BOOLEAN | FALSE | Whether consolidation engine has processed this |
| created_at | TEXT | | ISO 8601 |

### MemoryItem

Long-term memory unit with typed content.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| memory_item_id | TEXT UNIQUE | UUID | |
| workspace_id | TEXT | | |
| memory_type | TEXT NOT NULL | | See Memory Types below |
| title | TEXT | | Human-readable title |
| content | TEXT NOT NULL | | Structured content (JSON varies by type) |
| entities_json | TEXT | | Related entity names |
| relations_json | TEXT | | Relations to other objects |
| evidence_json | TEXT | | Evidence chain |
| confidence | REAL | 0.5 | Materialized: alpha / (alpha + beta) |
| evidence_alpha | REAL | 1.0 | |
| evidence_beta | REAL | 1.0 | |
| status | TEXT | "candidate" | |
| created_at | TEXT | | |
| updated_at | TEXT | | |

### ConceptCandidate

Candidate concept grown from clustered observations.

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
| confidence | REAL | 0.5 | |
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

Confirmed, stable concept for long-term use.

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
| confidence | REAL | 0.5 | |
| evidence_alpha | REAL | 1.0 | |
| evidence_beta | REAL | 1.0 | |
| status | TEXT | "active" | "active" \| "labile" \| "deprecated" \| "disputed" |
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

### sqlite-vec (Primary)

```sql
CREATE VIRTUAL TABLE vec_embedding USING vec0(
    embedding float[512]
);

CREATE TABLE embedding_ref (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    vec_rowid       INTEGER NOT NULL,
    source_type     TEXT NOT NULL,  -- 'observation' | 'concept' | 'concept_candidate'
    source_id       TEXT NOT NULL,
    workspace_id    TEXT,
    text_content    TEXT,           -- original text for re-embedding
    created_at      TEXT
);
```

### BLOB Fallback

```sql
CREATE TABLE embedding_blob (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type     TEXT NOT NULL,
    source_id       TEXT NOT NULL,
    workspace_id    TEXT,
    text_content    TEXT,
    vector_blob     BLOB NOT NULL,  -- ndarray Array1<f32> as raw bytes, 512 * 4 = 2048 bytes
    created_at      TEXT
);
```

Startup: try loading sqlite-vec extension. If failure → log warning, use embedding_blob with ndarray cosine similarity.

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
auto_confirmed → active (after promotion)
confirmed → active (after promotion)
active → labile (on recall — reconsolidation window)
labile → active (timeout or re-consolidation, possibly modified)
confirmed → deprecated (vitality drops below 0.40)
confirmed → disputed (new conflicting evidence)
deprecated → candidate (new supporting evidence)
```
