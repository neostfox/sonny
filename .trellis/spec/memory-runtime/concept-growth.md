# Memory Runtime — Concept Growth Pipeline

## Overview

### MVP Pipeline（Phase 1-3）

```
Observe → Extract → Embed → Cluster → Name → Store → Recall → Feedback
```

### 完整 Pipeline（Phase 1-7 完成后）

```
Observe → Extract → Validate → Embed → Cluster → Name → Link → Consolidate → Validate → Promote → Use → Revise
```

The pipeline has four operational modes:
- **Offline**: Session ingest → observation extraction → (adversarial validation) → clustering → (consolidation)
- **Realtime**: Query → concept recall → Memory Context generation
- **Feedback**: User feedback → Bayesian revision → concept update
- **Active**: System proactively asks user to confirm uncertain concepts

> **阶段标记说明**：每个 pipeline stage 标注了 `[MVP]` 或 `[Enhancement]`。
> MVP 阶段使用简化实现，核心增强阶段引入完整机制。
> 详见 [roadmap.md](./roadmap.md)。

## Pipeline Stages

### 1. Observe `[MVP]`

Extract raw observations from input sources.

**Input sources**:
- User messages
- Assistant responses
- Historical session files (markdown/json)
- Trellis journal entries
- Trellis task/prd files
- Project documentation

**Output**: RawMemory records in SQLite.

### 2. Extract `[MVP]`（Predictive Coding 在 Phase 5 引入）

LLM-powered extraction using **Predictive Coding** — the brain doesn't record everything, it generates predictions and only processes prediction errors (surprises).

**Signature**:
```python
def extract_observations(
    raw_memory: RawMemory,
    existing_concepts: list[Concept]
) -> ExtractResult:
    """
    Extract observations using predictive coding:
    1. Generate predictions from existing concepts
    2. Compare predictions against actual content
    3. Only extract "prediction errors" (novel information)
    4. Record confirming evidence separately (low surprise)
    """
```

**Output fields per observation**:
- `subject_text`: Entity name (e.g., "POSMASK")
- `subject_type`: Entity type (e.g., "database_table")
- `predicate`: Relation (e.g., "not_has_field", "depends_on", "is_root_cause")
- `object_text`: Target entity or value
- `object_type`: Target type
- `evidence_text`: Verbatim supporting text
- `confidence`: Initial confidence (0.5 default)
- `status`: Always starts as "candidate"
- `source_type`: "user_message" | "user_confirm" | "user_negation" | "assistant_guess" | "file_evidence"
- `surprise_score`: How unexpected this observation is relative to existing concepts (0-1)

**Predictive Coding Flow**:
```python
def predictive_extract(raw_memory, existing_concepts):
    # 1. Generate predictions from existing concepts
    predictions = generate_predictions(existing_concepts, raw_memory.context)

    # 2. Compare against reality
    prediction_errors = []
    for prediction in predictions:
        actual = check_against_reality(prediction, raw_memory)
        if actual != prediction.expected:
            prediction_errors.append(PredictionError(
                concept_id=prediction.concept_id,
                expected=prediction.expected,
                actual=actual,
                surprise_score=abs(prediction.confidence - actual.confidence)
            ))

    # 3. High surprise = worth extracting as new observation
    novel = [e for e in prediction_errors if e.surprise_score > 0.3]

    # 4. Low surprise = confirming evidence (update alpha, don't create new obs)
    confirming = [e for e in prediction_errors if e.surprise_score <= 0.1]

    return novel, confirming
```

**Why**: Avoids redundant storage. A project queried 100 times about POSMASK stores 1 observation + 99 confirmations, not 100 duplicate observations. Novel information (prediction errors) naturally gets higher initial weight.

**Constraints**:
- Only extract information with clear evidence
- Distinguish user-provided vs user-confirmed vs user-negated vs assistant-speculated
- Negative information MUST be preserved
- Novel observations get surprise_score as initial alpha boost
- Output as structured JSON

### 3. Validate (Adversarial Validation) `[Enhancement — Phase 4]`

Second LLM challenges extracted observations before they enter the system. **Not in MVP** — MVP uses simple evidence-text substring check as a basic quality gate.

**Signature**:
```python
def validate_observations(
    observations: list[Observation],
    raw_text: str
) -> list[ValidatedObservation]:
    """
    Adversarial validation: Validator LLM challenges each observation.
    Accepted observations proceed. Rejected ones marked disputed.
    """
```

**Validation criteria**: evidence clarity, over-speculation, triple accuracy, confidence calibration, source attribution.

**Verdicts**: `accepted` (pass), `adjusted` (confidence × 0.7), `rejected` (status → disputed).

**Cost control**: Only validate observations with surprise_score > 0.5 (sampling). See [quality-control.md](./quality-control.md) for full spec.

### 4. Embed `[MVP]`

Generate vector embeddings for observations. Includes incremental update for existing concepts.

**Signature**:
```python
def embed_observation(observation: Observation) -> np.ndarray:
    """
    Embed observation text as 512-dim vector using bge-small-zh-v1.5.
    Text = subject_text + " " + predicate + " " + object_text
    """
```

**Embedding targets**:

| Object | Embed Text |
|--------|-----------|
| Observation | `subject_text + " " + predicate + " " + object_text` |
| Concept Candidate | `name + " " + summary` |
| Concept | `name + " " + definition` |
| Recall Query | Raw user query |

**Storage**: `vec_embedding` (sqlite-vec) with `embedding_ref` metadata table. Fallback: `embedding_blob` (BLOB) with numpy cosine similarity.

**Incremental Embedding Update** `[Enhancement — Phase 7]`: During consolidation, check if concept content has drifted from its embedding. If cosine distance between current text embedding and stored embedding > 0.15, re-embed the concept. Runs offline as part of consolidation.

### 5. Cluster `[MVP]`

Group related observations into concept candidates.

**Primary algorithm**: Hierarchical Agglomerative Clustering (HAC) with cosine distance.

```python
from sklearn.cluster import AgglomerativeClustering

clusterer = AgglomerativeClustering(
    n_clusters=None,
    distance_threshold=0.25,  # configurable per workspace
    metric='cosine',
    linkage='average'
)
labels = clusterer.fit_predict(observation_embeddings)
```

**Secondary merge**: After HAC, merge clusters sharing entity names (subject or object fields) if centroid distance < 0.40. This compensates for embedding weaknesses on domain-specific terms.

**Combined distance function**:
```python
def combined_distance(obs_a, obs_b, embedding_dist):
    entities_a = {obs_a.subject_text, obs_a.object_text}
    entities_b = {obs_b.subject_text, obs_b.object_text}
    jaccard = len(entities_a & entities_b) / max(len(entities_a | entities_b), 1)
    if jaccard > 0:
        return embedding_dist * (1 - 0.5 * jaccard)
    return embedding_dist
```

**Incremental clustering**: New observations are compared against existing cluster centroids. If nearest centroid distance < merge_threshold, add to existing cluster and update centroid. Otherwise, create new cluster. Periodic full rebuild (every 7 days) prevents drift.

### 6. Name `[MVP]`

Generate human-readable concept names from clustered observations.

**Naming rules**:
- Close to user's actual expressions
- Express a long-term theme
- Not overly abstract
- Not a single technical word

**Good examples**: "MASK 与机器关系排查", "BOE 内网 DNS 排障", "Spring Boot Mapper 启动失败"
**Bad examples**: "数据库", "网络", "错误"

### 7. Link `[MVP]`

Connect concept to related objects:
- Related entities (union of subject/object from observations)
- Known facts (positive-predicate observations)
- Rejected hypotheses (negation observations)
- Open questions (uncertainty markers)
- User preferences
- Task state
- Evidence fragments

### 8. Consolidate (Hippocampus Replay + Complementary Learning) `[Enhancement — Phase 5]`

Offline consolidation engine — runs after each session import or on schedule. **Not in MVP** — MVP uses direct observation-to-concept integration with simple "don't overwrite" rule (new observations update alpha/beta but don't modify known_facts directly).

**Complementary Learning**: Observations are first stored in the **hippocampal buffer** (fast storage, no modification of existing concepts). After a cooling period (default: 24 hours), the consolidation engine gradually integrates them into the concept store (slow storage). This prevents catastrophic overwriting — a single wrong session cannot destroy established concepts.

**Signature**:
```python
def consolidate(workspace_id: str) -> ConsolidationReport:
    """
    1. Collect settled observations from hippocampal buffer (age >= 24h).
    2. Cross-validate against all existing concepts.
    3. Perform frequency counting, conflict detection, hierarchy detection.
    4. Auto promote/demote based on vitality thresholds.
    """
```

**Steps**:
1. Collect settled observations from hippocampal buffer (age >= 24h, status='fast_stored')
2. For each observation, find related concepts via embedding similarity
3. Check consistency: does the observation support, contradict, or extend the concept?
4. Update concept's alpha/beta based on consistency result (never overwrite — only adjust weights)
5. Track supporting_sessions set for cross-session counting
6. Unmatched observations → mark as 'orphan' for next clustering round
7. Run cross-session equivalence detection
8. **Detect hierarchy** (Chunking): if concept A's entities ⊂ concept B's entities and similarity > 0.7, A is a subconcept of B
9. Execute auto promote/demote decisions
10. Clear consolidated entries from hippocampal buffer

**Cross-session equivalence**:
- Hard match: identical subject + predicate + object
- Soft match: entity overlap + embedding similarity > 0.85
- Conflict detection: same entity + opposite predicates

**Hierarchy detection (Chunking)**:
```python
def detect_hierarchy(concepts):
    """
    Auto-detect parent/child concept relationships.
    Rule: if A.entities ⊂ B.entities AND embedding_sim(A, B) > 0.7 → A is child of B.
    """
    edges = []
    for child in concepts:
        for parent in concepts:
            if child.entities < parent.entities:
                sim = cosine_similarity(child.embedding, parent.embedding)
                if sim > 0.7:
                    edges.append((child.id, parent.id, 'is_subconcept_of'))
    return edges
```

### 9. Validate `[MVP]`（Hysteresis threshold 在 Phase 7 引入）

Assess concept quality using multi-dimensional vitality model.

**Concept Vitality Score**:
```python
vitality = (
    0.30 * bayesian_confidence +    # alpha / (alpha + beta)
    0.25 * success_rate +           # successful_recalls / total_recalls
    0.20 * diversity_score +        # min(1, unique_sessions / 5)
    0.15 * time_decay +             # exp(-days_since_recall / 90)
    0.10 * connectivity             # min(1, connection_count / 10)
)
```

### 10. Promote `[MVP]`

Auto-confirmation without human review.

**Auto-confirm conditions** (ALL must be true):
- `vitality > 0.80`
- `unique_session_count >= 3`
- `conflicts == 0`
- `bayesian_confidence > 0.75`

**Auto-demote conditions**:
- `vitality < 0.40` AND `days_since_creation > 30` → deprecated
- `conflicts >= supporting_count` → disputed

### 11. Use (Recall) `[MVP]`（高级召回在 Phase 7 引入）

Retrieve relevant concepts for a user query. See [recall-design.md](./recall-design.md).

### 12. Revise `[MVP]`（Reconsolidation 在 Phase 7 引入）

Update concepts based on user feedback. See [quality-control.md](./quality-control.md).

## Pipeline Data Flow

```
Session 导入
  ↓
[Predictive Coding] LLM Extractor → only novel observations + confirming evidence
  ↓
[Adversarial Validator] Validator LLM challenges → accepted / adjusted / rejected
  ↓
[Hippocampal Buffer] fast_stored (24h cooling period)
  ↓
[Embedding] embed observation text → vec_embedding
  ↓
[HAC + Entity Overlap] cluster → concept_candidate
  ↓
[LLM] concept naming → named concept_candidate
  ↓
[Consolidation Engine] after cooling: cross-session validation → confidence update
  ↓
[Incremental Embedding] re-embed concepts with drift > 0.15
  ↓
[Chunking] hierarchy detection → parent/child relationships
  ↓
[Vitality Model] auto promote/demote
  ↓
[Active Learning] generate questions for uncertain concepts
  ↓
[Concept Store] confirmed concepts ready for recall
```
