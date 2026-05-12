# Implementation Thinking Guide — Memory Runtime

Pre-implementation checklist for Memory Runtime development.

## Before Starting Phase 2 (Embedding)

- [ ] Candle BERT example loads bge-small-zh-v1.5 successfully (`cargo run --example bert --release -- --model-id BAAI/bge-small-zh-v1.5`)
- [ ] Model weights cached to `~/.memory-runtime/models/` (first run downloads ~90MB)
- [ ] Offline fallback tested: if model download fails, system degrades gracefully to keyword matching
- [ ] Embedding dimension confirmed: 512 float32 = 2048 bytes per vector

## Before Starting Phase 3 (Recall)

- [ ] Recall path has ZERO LLM calls (pre-computed entity matching only)
- [ ] Embedding similarity uses ndarray cosine distance, not external service
- [ ] Workspace filtering applied before vector search (avoid cross-workspace leakage)
- [ ] Recall latency target: < 100ms (measure with realistic data)

## LLM Integration Checklist

- [ ] Identify if multiple LLM steps share context → merge into single call (see `principles.md`: Decision: LLM Call Merging)
- [ ] Never merge offline steps with realtime steps
- [ ] All LLM outputs validated as JSON before parsing (LLM can return malformed output)
- [ ] LLM calls use workspace-specific prompts (avoid cross-domain contamination)

## Rust + Candle Gotchas

> **Warning**: bge-small-zh-v1.5 uses mean pooling over token embeddings, not CLS token. The Candle BERT example must apply mean pooling (average of all token embeddings) to produce correct sentence embeddings. Using raw CLS token output will produce incorrect similarity scores.

> **Warning**: Candle downloads model weights from HuggingFace Hub on first use. In air-gapped environments, pre-download weights via `memory download-model` CLI command.

> **Warning**: sqlite-vec is a Rust crate but requires C compilation for some platforms. BLOB fallback (ndarray cosine similarity) must work without sqlite-vec.
