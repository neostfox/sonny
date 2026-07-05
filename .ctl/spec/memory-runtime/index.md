# Memory Runtime Spec Index

Spec documents for the Memory Runtime system — a concept-growth-based long-term memory system for Coding Agents.

## Spec Files

- [principles.md](./principles.md) — Irreducible principles, design decisions (neuroscience mechanisms, active learning, adversarial validation, GNN), anti-patterns
- [concept-growth.md](./concept-growth.md) — Full pipeline: Observe → Extract → Validate → Embed → Cluster → Name → Link → Consolidate → Validate → Promote → Use → Revise
- [memory-model.md](./memory-model.md) — Object model, DB schemas (SQLite + sqlite-vec + hippocampal buffer + hierarchy), status lifecycle
- [recall-design.md](./recall-design.md) — Dual-channel recall, priming, spreading activation, sparse activation, hierarchy expansion, intent classification
- [quality-control.md](./quality-control.md) — Beta-Bernoulli confidence, vitality model, reconsolidation, active learning, adversarial validation, auto-confirmation

## Related Documents

- [Master Design Document](../../../docs/memory-runtime-design.md) — Full system design (original source of truth)
- [Roadmap](../backend/roadmap.md) — MVP phases, post-MVP enhancements, scale features (GNN, meta-learning), evaluation framework
- [Design Reference](../backend/design-reference.md) — Backend design status and cross-cutting reference

## Implementation Status (as of 2026-06-22)

| Phase | Status | Notes |
|-------|--------|-------|
| P0 (Foundation) | ✅ Complete | Windows compat, connection ownership, entity_concept, type-safe store, heap alloc |
| P1 (Model Alignment) | ✅ Complete | MemoryItem removal, candidate status, confidence dual-dimension, model alignment |
| P2-A (Extraction Validation) | ✅ Complete | Anti-hallucination gate, evidence support check |
| P2-B (Dedup/Normalize) | ✅ Complete | Predicate normalization, duplicate detection |
| P2-C (Observation Co-occurrence) | ✅ Complete | extraction_batch_id, observation_coclaim table |
| P2-D (Reverse Correction) | ✅ Complete | reextract(), prompt versioning, supersede mechanism |
| P3-A (Embedding Storage) | ✅ Complete | BLOB storage, cosine search, OpenAI-compatible provider |
| P3-B (Cluster Engine) | ✅ Complete | HAC clustering, combined distance, entity+embedding |
| P3-C (Merge/Split) | ✅ Complete | Candidate merging, Jaccard similarity, Union-Find |
| P4-A (Recall Engine) | ✅ Complete | Intent classification, entity match, semantic search, token budget, recall stats |
| P4-B (Feedback) | ✅ Complete | feedback table + ledger, keyword classification, revision weights, Negate→rejected_hypothesis, Correct→supersede, closed recall loop (outcome from explicit feedback) |
| P4-C (Time Awareness) | ✅ Complete | Ebbinghaus decay + rehearsal deceleration, last_recalled_at anchor, recency salience factor |

## Quick Reference

| Topic | File | Key Section |
|-------|------|-------------|
| Why no NER | principles.md | Design Decisions |
| Neuroscience mechanisms | principles.md | Neuroscience-Inspired Mechanisms |
| Predictive coding extraction | concept-growth.md | Stage 2: Extract |
| Clustering algorithm | concept-growth.md | Stage 5: Cluster |
| DB schema | memory-model.md | All object tables |
| Vector storage | memory-model.md | Embedding Tables |
| Dual-dimension confidence | memory-model.md | Observation |
| Memory type taxonomy | memory-model.md | Memory Types |
| Recall flow | recall-design.md | Recall Execution Flow |
| Intent classification | recall-design.md | Intent Classification |
| Confidence weights | quality-control.md | Evidence Weight Table |

## Tech Stack

- **Language**: Rust (core engine) + TypeScript (Claude Code hooks)
- **Embedding**: OpenAI-compatible `/v1/embeddings` endpoint (bge-m3 1024-dim default)
- **Clustering**: `linfa-clustering` (HAC)
- **Storage**: `rusqlite` (SQLite with WAL mode)
- **Async**: `tokio` + `reqwest`
- **CLI**: `clap` (derive)
- **Concurrency**: `parking_lot::Mutex`
