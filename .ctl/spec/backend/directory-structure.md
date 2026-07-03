# Directory Structure

## Legend

| Color | Role |
|-------|------|
| 🟠 Orange | Entry — CLI argument parsing, command dispatch |
| 🔵 Blue | Domain — Pure data types, no I/O imports |
| 🟢 Green | Interface — Trait definitions for external services |
| 🔴 Red | Infrastructure — SQLite persistence, migrations |
| 🟣 Purple | Service — Cross-cutting domain logic (confidence, entity) |
| ⬜ White | Application — Business orchestration (pipeline, recall) |
| ⚪ Gray | Shared — Configuration, utilities |
| 💀 Dead | Code/table exists but no code reads or writes it |

## Tree

```
crates/
├── memory-runtime/                          # Core library crate
│   ├── Cargo.toml                           # Dependencies: rusqlite, thiserror, ndarray, linfa, reqwest, tokio, ...
│   ├── src/
│   │   ├── lib.rs                           # Public module re-exports (11 modules)
│   │   ├── config.rs                        # ⚪ Settings struct + defaults + env overrides
│   │   ├── error.rs                         # ⚪ MemoryError enum (thiserror derive, 11 variants)
│   │   │
│   │   ├── models/                          # 🔵 Domain — Pure data types (10 files)
│   │   │   ├── mod.rs                       # Re-exports all model submodules
│   │   │   ├── concept.rs                   # Concept, ConceptCandidate, ConceptType
│   │   │   ├── embedding.rs                 # EmbeddingRef, EmbeddingSourceType, EmbeddingSearchResult
│   │   │   ├── evidence.rs                  # Evidence struct
│   │   │   ├── feedback.rs                  # FeedbackType enum, FeedbackResult (model only, no table)
│   │   │   ├── hierarchy.rs                 # HierarchyType, RelationType enums
│   │   │   ├── observation.rs               # Observation, ObservationSourceType, MemoryType
│   │   │   ├── predicate.rs                 # Predicate enum (7 variants) + normalize_predicate()
│   │   │   ├── raw_memory.rs                # RawMemory, SourceType
│   │   │   ├── recall.rs                    # MemoryContext, RecallScore, Intent, RecallBudget
│   │   │   └── status.rs                    # ObservationStatus, ConceptStatus, CandidateStatus
│   │   │
│   │   ├── confidence/                      # 🟣 Service — Beta-distribution confidence
│   │   │   └── mod.rs                       # BetaConfidence, EvidenceType (12 types), weight table
│   │   │                                    # ✅ Used by extract pipeline + recall stats
│   │   │
│   │   ├── entity/                          # 🟣 Service — Entity normalization
│   │   │   └── mod.rs                       # canonical_key(), canonical_key_light()
│   │   │                                    # ✅ Used by cluster engine for entity Jaccard
│   │   │
│   │   ├── pipeline/                        # ⬜ Application — Full growth pipeline
│   │   │   ├── mod.rs                       # Re-exports: cluster, extract, ingest, merge
│   │   │   ├── ingest.rs                    # SessionParser trait, JournalParser, JsonSessionParser
│   │   │   │                                # ✅ Regex via LazyLock, detect_and_parse()
│   │   │   ├── extract.rs                   # extract_observations(), extract_and_dedup(), reextract()
│   │   │   │                                # ✅ Anti-hallucination gate, evidence validation
│   │   │   ├── cluster.rs                   # ClusterEngine: HAC + embedding distance
│   │   │   │                                # ✅ P3-B: combined_distance, ObservationCluster
│   │   │   └── merge.rs                     # MergeSplitEngine: candidate merging + splitting
│   │   │                                    # ✅ P3-C: merge_group, split_candidate, Jaccard
│   │   │
│   │   ├── recall/                          # ⬜ Application — Recall engine
│   │   │   └── mod.rs                       # ✅ P4-A: RecallEngine, classify_intent(), build_context()
│   │   │                                    #    Intent bilingual classification + entity match + embedding semantic search
│   │   │                                    #    Dynamic token budget enforcement + recall stats update
│   │   │
│   │   ├── store/                           # 🔴 Infrastructure — SQLite persistence
│   │   │   ├── mod.rs                       # Re-exports all store modules
│   │   │   ├── connection.rs                # Database struct (Arc<Mutex<Connection>>, WAL, FK)
│   │   │   ├── migration.rs                 # 6 versioned migrations via user_version PRAGMA
│   │   │   ├── traits.rs                    # RawMemoryStore, ObservationStore, ConceptStore, EmbeddingStore
│   │   │   ├── raw_memory_store.rs          # SqliteRawMemoryStore impl
│   │   │   ├── observation_store.rs         # SqliteObservationStore: CRUD + find_coclaim + replace_session
│   │   │   ├── concept_store.rs             # SqliteConceptStore: CRUD + entity JOIN + recall stats
│   │   │   └── embedding_store.rs           # SqliteEmbeddingStore: BLOB vector storage + cosine search
│   │   │
│   │   ├── llm/                             # 🟢 Interface — LLM provider abstraction
│   │   │   ├── mod.rs                       # Re-exports traits
│   │   │   └── traits.rs                    # LlmProvider async trait (complete, complete_json, health_check)
│   │   │
│   │   ├── embed/                           # 🟢 Interface + Impl — Embedding service
│   │   │   ├── mod.rs                       # build_embedding_service(), embed_and_store()
│   │   │   ├── traits.rs                    # EmbeddingProvider async trait (embed, embed_batch, dim)
│   │   │   └── openai.rs                    # ✅ OpenAiCompatibleEmbeddingProvider (reqwest HTTP)
│   │   │
│   │   ├── feedback/                        # ⏳ Deferred — Feedback subsystem
│   │   │   └── mod.rs                       # Stub: "Phase 3"
│   │   │
│   │   └── migrations/
│   │       ├── 001_initial.sql              # DDL: raw_memory, observation, concept_candidate,
│   │       │                                #   concept, entity_alias (memory_item dropped in 003)
│   │       ├── 002_entity_concept.sql       # entity_concept JOIN table (P0-C)
│   │       ├── 003_p1_model_alignment.sql   # observation.confidence → extraction_confidence,
│   │       │                                #   +memory_type_candidate, DROP memory_item
│   │       ├── 004_p2c_observation_coclaim.sql  # extraction_batch_id + observation_coclaim (P2-C)
│   │       ├── 005_p2d_reverse_correction.sql   # raw_memory.extraction_version + superseded_by (P2-D)
│   │       └── 006_p3a_embedding_storage.sql    # embedding BLOB table (P3-A)
│   │
│   └── tests/
│       └── integration_test.rs              # 12 integration tests (full pipeline + embed + recall)
│
├── sonny-cli/                               # 🟠 Entry — CLI binary
│   ├── Cargo.toml                           # Depends on memory-runtime, clap, tokio, tracing
│   └── src/
│       └── main.rs                          # clap derive: init, ingest-session, list-observations
│                                            # 设计 §11 要求 8 个命令，实现了 3 个
│
└── memory-test-fixtures/                    # ⚪ Shared — Test helpers
    ├── Cargo.toml                           # Depends on memory-runtime, async-trait
    └── src/
        ├── lib.rs                           # Re-exports mock_llm, stub_embedding
        ├── mock_llm.rs                      # MockLlmProvider: pattern-matched responses, call log
        └── stub_embedding.rs                # StubEmbeddingService: FNV-1a hash → deterministic vector
```

## Key Files

| Path | Role | Description |
|------|------|-------------|
| `crates/memory-runtime/src/lib.rs` | Shared | Public API surface (11 modules) |
| `crates/memory-runtime/src/error.rs` | Shared | `MemoryError` (11 variants) + `MemoryResult<T>` |
| `crates/memory-runtime/src/config.rs` | Shared | `Settings` struct with LLM/Embedding/Cluster/Recall/Confidence configs |
| `crates/memory-runtime/src/models/status.rs` | Domain | 3 status machines: ObservationStatus, ConceptStatus, CandidateStatus |
| `crates/memory-runtime/src/models/observation.rs` | Domain | Observation + MemoryType taxonomy + ObservationSourceType |
| `crates/memory-runtime/src/models/concept.rs` | Domain | Concept + ConceptCandidate + ConceptType |
| `crates/memory-runtime/src/models/recall.rs` | Domain | MemoryContext (token_count enforced) + RecallScore + Intent + RecallBudget |
| `crates/memory-runtime/src/models/predicate.rs` | Domain | Predicate enum (7 variants) + normalize_predicate() |
| `crates/memory-runtime/src/confidence/mod.rs` | Service | BetaConfidence + 12 EvidenceType weights |
| `crates/memory-runtime/src/entity/mod.rs` | Service | canonical_key() + canonical_key_light() |
| `crates/memory-runtime/src/pipeline/ingest.rs` | Application | SessionParser + 2 impls (Journal, JSON) |
| `crates/memory-runtime/src/pipeline/extract.rs` | Application | LLM extraction + dedup + reextract (P2-D) |
| `crates/memory-runtime/src/pipeline/cluster.rs` | Application | HAC clustering + embedding distance (P3-B) |
| `crates/memory-runtime/src/pipeline/merge.rs` | Application | Candidate merge/split via Jaccard (P3-C) |
| `crates/memory-runtime/src/recall/mod.rs` | Application | RecallEngine: intent+entity+semantic (P4-A) |
| `crates/memory-runtime/src/store/traits.rs` | Infrastructure | 4 store traits (RawMemory, Observation, Concept, Embedding) |
| `crates/memory-runtime/src/store/connection.rs` | Infrastructure | Database: `Arc<Mutex<Connection>>` + WAL + FK |
| `crates/memory-runtime/src/store/embedding_store.rs` | Infrastructure | BLOB vector storage + cosine similarity search |
| `crates/memory-runtime/src/embed/traits.rs` | Interface | EmbeddingProvider async trait |
| `crates/memory-runtime/src/embed/openai.rs` | Interface | OpenAI-compatible HTTP embedding client |
| `crates/memory-runtime/src/llm/traits.rs` | Interface | LlmProvider async trait |
| `crates/memory-runtime/src/store/migration.rs` | Infrastructure | 6 versioned migrations |
| `crates/sonny-cli/src/main.rs` | Entry | CLI binary (3/8 commands) |
| `crates/memory-test-fixtures/src/mock_llm.rs` | Shared | MockLlmProvider for tests |
| `crates/memory-test-fixtures/src/stub_embedding.rs` | Shared | StubEmbeddingService (deterministic, network-free) |
| `crates/memory-runtime/tests/integration_test.rs` | Verification | 12 integration tests |
