# Memory Runtime — Principles

## Irreducible Principles

1. **Memory is concept growth, not text storage.** The system grows concepts from historical sessions and observations. Raw text is the raw material, not the product.

2. **User input is experience, not structured data.** Treat user input as raw experience. Extract goals, objects, facts, negations, corrections, preferences, and task states from it.

3. **Every memory must have evidence.** No memory enters `confirmed` status without a traceable source. Evidence includes: source_type, source_id, session_id, evidence_text, created_at, confidence, status.

4. **Assistant speculation is NOT fact.** LLM-generated content defaults to `candidate` status. Upgrade only when: user explicitly confirms, file/project evidence supports, or cross-session consistency holds (≥3 independent sessions, no conflicts).

5. **Negative information is high-value memory.** User corrections, rejections, and failed approaches must be preserved as `rejected_hypothesis` memories. These prevent repetitive mistakes.

6. **Recall prioritizes concepts over raw history.** The recall pipeline matches user queries to concepts and expands their context. Never inject large raw session history into prompts.

7. **Memory Context must be short, precise, and actionable.** Default limit: 1500 tokens. Allocate across current concept, known facts, rejected hypotheses, user preferences, and task state.

## Design Decisions

### Decision: No Separate NER Pipeline

**Context**: The Observation Extractor needs to identify entities (tables, fields, services, etc.) from raw text.

**Options**:
1. Dedicated NER model (spaCy zh, LTP) — adds 100-500MB dependency
2. LLM-based extraction (already in the pipeline) — zero additional cost

**Decision**: LLM handles entity extraction. The Observation Extractor already outputs structured subject/predicate/object with entity types. This is strictly more information than NER provides (relations, not just entities). Domain-specific terms like POSMASK, MASKSPECNAME are better handled by LLM with context than by pretrained NER models that classify them as MISC.

### Decision: Vector Embeddings for Cluster + Recall Only

**Context**: Need semantic similarity for grouping observations and retrieving concepts.

**Decision**: Use `BAAI/bge-small-zh-v1.5` (512-dim, ~90MB, CPU-friendly) for:
- Clustering observations into concept candidates (offline)
- Semantic recall matching queries to concepts (realtime)

Vectors do NOT replace LLM extraction. They complement it: LLM does understanding, embeddings do speed and scale.

### Decision: Beta-Bernoulli for Confidence Management

**Context**: Need a principled way to update confidence based on accumulated evidence.

**Decision**: Each concept/observation maintains Beta(alpha, beta) prior. Confidence = alpha / (alpha + beta). Pure Python, zero dependencies, interpretable, sequential updates.

### Decision: Consolidation Engine (Hippocampus Replay)

**Context**: At scale, human review is impossible. The system needs an algorithmic mechanism to validate and strengthen concepts automatically, analogous to how the brain consolidates memories from prefrontal cortex (short-term) through hippocampus (replay/integration) to cortex (long-term).

**Decision**: A dedicated Consolidation Engine runs offline, performing cross-session validation, frequency counting, conflict detection, and automatic promotion/demotion without human intervention.

### Decision: Python Core + TS Hooks

**Context**: Need ML ecosystem (sentence-transformers, sklearn) and Claude Code hook integration.

**Decision**: Core engine in Python. Claude Code hooks in TS/Shell calling Python CLI (`memory prehook` / `memory posthook`).

### Decision: Neuroscience-Inspired Mechanisms

Six mechanisms from neuroscience research are incorporated. **These are not optional enhancements — they are the product's core differentiation.** MVP proves the pipeline works; these mechanisms make it excellent.

| Mechanism | Brain Analog | Purpose | When Introduced |
|-----------|-------------|---------|----------------|
| Predictive Coding | Cortex prediction hierarchy | Only extract novel/surprising info | Phase 5 (core enhancement) |
| Complementary Learning | Hippocampus (fast) vs Cortex (slow) | Prevent catastrophic overwriting | Phase 5 (core enhancement) |
| Reconsolidation | Memory labile on recall | Allow corrections during recall | Phase 7 (advanced recall) |
| Sparse Activation | ~2% neuron firing rate | Prevent noise in Memory Context | Phase 7 (advanced recall) |
| Chunking / Hierarchy | Hierarchical representations | Zoom in/out from abstract to specific | Phase 7 (advanced recall) |
| Contextual Priming | Semantic priming | Pre-activate related concepts from recent context | Phase 7 (advanced recall) |

**MVP 简化原则**：在 MVP 中不实现完整机制，但保留其核心原则（简化实现）。详见 [roadmap.md](./roadmap.md)。

### Decision: Active Learning for Human-in-the-Loop Confirmation

**Context**: At scale, full manual review is impossible. But some concepts genuinely need human judgment.

**Decision**: The system proactively identifies the most valuable questions to ask the user, limited to N per session (default 3). Questions target: concepts near confirmation threshold, conflicting evidence, stale candidates, and entity alias confirmation. Timing is contextual (session start, task completion) not random.

**Priority**: Highest — directly enables scalable quality assurance.

### Decision: Adversarial Validation for Extraction Quality

**Context**: LLM extraction quality is the single biggest uncertainty. Wrong extractions poison the entire concept graph.

**Decision**: A second LLM (Validator) challenges each extracted observation on five criteria: evidence clarity, over-speculation, subject/predicate/object accuracy, confidence calibration, and source attribution. Cost is controlled via sampling (only high-surprise observations), caching, and self-consistency fallback.

### Decision: Incremental Embedding Update

**Context**: Concept summaries change as new evidence arrives. Stale embeddings degrade recall quality.

**Decision**: During consolidation runs, check embedding drift for updated concepts. If cosine distance between old and new embedding > 0.15, re-embed. Simple, deterministic, runs offline.

### Decision: GNN Auto-Activation at Scale

**Context**: Hand-crafted spreading activation rules work well at small scale (<500 concepts) but cannot learn optimal message-passing patterns.

**Decision**: When concept count ≥ 500 AND graph density ≥ 0.05, automatically activate GraphSAGE training in the background. GNN results initially run in parallel with hand-crafted rules (advisory mode), then gradually take over as they prove reliable. This is a scale feature, not an MVP feature.

## Anti-patterns

### Don't: Inject Raw Session History

```
// Don't: paste entire conversation into prompt
[Memory Context]
Session 1: User asked about MASK table...
(5000 lines of conversation)
```

**Why**: Token waste, noise, and the agent can't distinguish signal from noise.

**Instead**: Extract concepts and recall only relevant context.

### Don't: Confirm Assistant Guesses Without Evidence

```
// Don't: assistant said X → mark X as confirmed
observation.status = 'confirmed'  // WRONG if source is assistant
```

**Instead**: Assistant-originated observations stay at `candidate` until cross-validated.

### Don't: Discard User Corrections

```
// Don't: ignore "no, that's not right"
// User negation is HIGH VALUE — it prevents future mistakes
```

**Instead**: Create `rejected_hypothesis` memory with the negation reason.
