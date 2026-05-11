# Memory Runtime Spec Index

Spec documents for the Memory Runtime system — a concept-growth-based long-term memory system for Coding Agents.

## Spec Files

- [principles.md](./principles.md) — Irreducible principles, design decisions (neuroscience mechanisms, active learning, adversarial validation, GNN), anti-patterns
- [concept-growth.md](./concept-growth.md) — Full pipeline: Observe → Extract → Validate → Embed → Cluster → Name → Link → Consolidate → Validate → Promote → Use → Revise
- [memory-model.md](./memory-model.md) — Object model, DB schemas (SQLite + sqlite-vec + hippocampal buffer + hierarchy), status lifecycle
- [recall-design.md](./recall-design.md) — Dual-channel recall, priming, spreading activation, sparse activation, hierarchy expansion, intent classification
- [quality-control.md](./quality-control.md) — Beta-Bernoulli confidence, vitality model, reconsolidation, active learning, adversarial validation, auto-confirmation
- [roadmap.md](./roadmap.md) — MVP phases, post-MVP enhancements, scale features (GNN, meta-learning), evaluation framework

## Quick Reference

| Topic | File | Key Section |
|-------|------|-------------|
| Why no NER | principles.md | Design Decisions |
| Neuroscience mechanisms | principles.md | Neuroscience-Inspired Mechanisms |
| Active Learning decision | principles.md | Active Learning for Human-in-the-Loop |
| Adversarial Validation decision | principles.md | Adversarial Validation for Extraction Quality |
| GNN auto-activation decision | principles.md | GNN Auto-Activation at Scale |
| Predictive coding extraction | concept-growth.md | Stage 2: Extract |
| Adversarial validation gate | concept-growth.md | Stage 3: Validate |
| Incremental embedding update | concept-growth.md | Stage 4: Embed |
| Clustering algorithm | concept-growth.md | Stage 5: Cluster |
| Hippocampal buffer (complementary learning) | concept-growth.md | Stage 8: Consolidate |
| Hierarchy detection (chunking) | concept-growth.md | Stage 8: Consolidate |
| DB schema | memory-model.md | All object tables |
| Hippocampal buffer table | memory-model.md | Hippocampal Buffer |
| Concept hierarchy table | memory-model.md | Concept Hierarchy |
| Vector storage | memory-model.md | Embedding Tables |
| Contextual priming | recall-design.md | Contextual Priming |
| Sparse activation | recall-design.md | Sparse Activation |
| Hierarchy expansion | recall-design.md | Hierarchy Expansion |
| Recall flow | recall-design.md | Recall Execution Flow |
| Reconsolidation | quality-control.md | Memory Reconsolidation |
| Active Learning questions | quality-control.md | Active Learning |
| Adversarial Validation criteria | quality-control.md | Adversarial Validation |
| Confidence weights | quality-control.md | Evidence Weight Table |
| Auto-confirmation | quality-control.md | Auto-Confirmation Algorithm |
| MVP phases | roadmap.md | MVP (Phase 1-4) |
| Post-MVP enhancements | roadmap.md | Post-MVP Enhancements |
| GNN scale feature | roadmap.md | Scale Feature 1: GNN |
| Meta-learning scale feature | roadmap.md | Scale Feature 2: Meta-Learning |
| Evaluation criteria | roadmap.md | Evaluation Framework |

## Related Documents

- [Technical Design Document](../../docs/memory-runtime-design.md) — Full system design (original source)
- [Implementation Plan](file:///home/shaobolua/.claude/plans/mvp-ner-goofy-kitten.md) — Phased implementation plan
