# Directory Structure

## Legend

| Color | Role |
|-------|------|
| 🟠 Orange | Entry — CLI argument parsing, command dispatch |
| 🔵 Blue | Domain — Pure data types, no I/O imports |
| 🟢 Green | Interface — Trait definitions for external services |
| 🔴 Red | Infrastructure — SQLite persistence, migrations |
| 🟣 Purple | Service — Cross-cutting domain logic |
| ⬜ White | Application — Business orchestration |
| ⚪ Gray | Shared — Configuration, utilities |
| ⏳ Deferred | Stub modules awaiting future phase |
| 💀 Dead | Code/table exists but no code reads or writes it |

## Tree

```
crates/
├── memory-runtime/                          # Core library crate
│   ├── Cargo.toml                           # Dependencies: rusqlite, thiserror, ndarray, linfa, ...
│   ├── src/
│   │   ├── lib.rs                           # Public module re-exports
│   │   ├── config.rs                        # ⚪ Settings struct + defaults + env overrides
│   │   │                                    # ⚠️ dirs_home() 不支持 Windows
│   │   ├── error.rs                         # ⚪ MemoryError enum (thiserror derive)
│   │   │
│   │   ├── models/                          # 🔵 Domain — Pure data types
│   │   │   ├── mod.rs                       # Re-exports all model submodules
│   │   │   ├── concept.rs                   # Concept, ConceptCandidate, ConceptType
│   │   │   │                                # ⚠️ ConceptCandidate.status 是 ObservationStatus
│   │   │   │                                #    应使用独立生命周期枚举 (D4)
│   │   │   ├── embedding.rs                 # EmbeddingRef, EmbeddingSearchResult
│   │   │   ├── evidence.rs                  # Evidence struct
│   │   │   ├── feedback.rs                  # FeedbackType enum, FeedbackResult
│   │   │   │                                # ⚠️ 无 feedback 表，无调用者
│   │   │   ├── hierarchy.rs                 # HierarchyType, RelationType enums
│   │   │   ├── memory_item.rs               # 💀 MemoryItem, MemoryType — 无代码读写 (D3)
│   │   │   ├── observation.rs               # Observation, ObservationSourceType
│   │   │   │                                # ⚠️ 三元组太弱，无法表达排障路径等 (D2)
│   │   │   ├── raw_memory.rs                # RawMemory, SourceType
│   │   │   ├── recall.rs                    # MemoryContext, RecallScore, Intent
│   │   │   │                                # ⚠️ token_count 字段不生效 (D8)
│   │   │   └── status.rs                    # ObservationStatus, ConceptStatus enums
│   │   │
│   │   ├── confidence/                      # 🟣 Service — Beta-distribution confidence
│   │   │   └── mod.rs                       # BetaConfidence, EvidenceType, weight table
│   │   │                                    # ⚠️ 无 pipeline 调用者，所有 confidence 永远 0.5 (D5)
│   │   │
│   │   ├── entity/                          # 🟣 Service — Entity normalization
│   │   │   └── mod.rs                       # canonical_key(), EntityNormalizer
│   │   │                                    # ⚠️ 无 pipeline 调用者，LLM 产出实体未归一化 (D7)
│   │   │
│   │   ├── pipeline/                        # ⬜ Application — Ingest + Extract orchestration
│   │   │   ├── mod.rs                       # Re-exports ingest, extract
│   │   │   ├── ingest.rs                    # SessionParser trait, TrellisJournalParser, JsonSessionParser
│   │   │   │                                # ⚠️ extract_session_id 每次调用编译 Regex
│   │   │   └── extract.rs                   # extract_observations(), LLM-powered extraction
│   │   │                                    # ⚠️ 无去重步骤 (D6)
│   │   │                                    # ⚠️ 无实体归一化步骤 (D7)
│   │   │                                    # ⚠️ 无长 session 分块策略
│   │   │
│   │   ├── store/                           # 🔴 Infrastructure — SQLite persistence
│   │   │   ├── mod.rs                       # Re-exports all store modules
│   │   │   ├── connection.rs                # Database struct (open, open_in_memory)
│   │   │   │                                # ⚠️ conn 被 move 后 Database 变空壳
│   │   │   │                                #    多 store 无法共享连接
│   │   │   ├── migration.rs                 # Versioned migration runner
│   │   │   ├── traits.rs                    # RawMemoryStore, ObservationStore, ConceptStore, EmbeddingStore traits
│   │   │   │                                # ⚠️ update_status 接受 &str 不是枚举类型
│   │   │   ├── raw_memory_store.rs          # SqliteRawMemoryStore impl
│   │   │   ├── observation_store.rs         # SqliteObservationStore impl
│   │   │   │                                # ⚠️ obs_params 每次 17 个 Box<dyn ToSql> 堆分配
│   │   │   │                                # ⚠️ list_by_workspace 无 LIMIT
│   │   │   │                                # ⚠️ parse_observation_status 默认值无日志
│   │   │   ├── concept_store.rs             # SqliteConceptStore impl
│   │   │   │                                # ⚠️ find_by_entities 用 LIKE 搜索 JSON — 假阳性 (C2)
│   │   │   │                                # ⚠️ update_recall_stats 不检查行是否存在
│   │   │   │                                # ⚠️ candidate_params 每次 22 个 Box<dyn ToSql>
│   │   │   └── embedding_store.rs           # ⏳ Placeholder (Phase 2)
│   │   │
│   │   ├── llm/                             # 🟢 Interface — LLM provider abstraction
│   │   │   ├── mod.rs                       # Re-exports traits
│   │   │   └── traits.rs                    # LlmProvider async trait
│   │   │
│   │   ├── embed/                           # 🟢 Interface — Embedding service abstraction
│   │   │   ├── mod.rs                       # Re-exports traits
│   │   │   └── traits.rs                    # EmbeddingService trait (embed, embed_batch)
│   │   │                                    # ⏳ 无实现，聚类和召回依赖此接口
│   │   │
│   │   ├── recall/                          # ⏳ Deferred — Phase 3
│   │   │   └── mod.rs                       # Stub comment
│   │   │
│   │   ├── feedback/                        # ⏳ Deferred — Phase 3
│   │   │   └── mod.rs                       # Stub comment
│   │   │
│   │   └── migrations/
│   │       └── 001_initial.sql              # Schema: 6 张表 + 索引
│   │                                        # 💀 memory_item 表存在但无代码读写
│   │                                        # ❌ 缺少 observation_candidate_link 表 (D1)
│   │                                        # ❌ 缺少 feedback 表 (D9)
│   │
│   └── tests/
│       └── integration_test.rs              # 7 个集成测试
│                                            # ⚠️ 用 std::mem::replace 传连接 — 绕过所有权问题
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
        ├── lib.rs                           # Re-exports mock_llm
        └── mock_llm.rs                      # MockLlmProvider: pattern-matched responses, call log
```

## Key Files

| Path | Role | Description | Issues |
|------|------|-------------|--------|
| `crates/memory-runtime/src/lib.rs` | Shared | Public API surface | |
| `crates/memory-runtime/src/error.rs` | Shared | `MemoryError` + `MemoryResult<T>` | |
| `crates/memory-runtime/src/config.rs` | Shared | `Settings` struct | `dirs_home()` 不支持 Windows |
| `crates/memory-runtime/src/models/status.rs` | Domain | 两个状态机 | `ConceptCandidate` 应该用 `ConceptStatus` 不是 `ObservationStatus` |
| `crates/memory-runtime/src/models/observation.rs` | Domain | 三元组 Observation | 无法表达排障路径、任务状态等 (D2) |
| `crates/memory-runtime/src/models/concept.rs` | Domain | Concept + ConceptCandidate | Candidate 无法增量生长 (D1) |
| `crates/memory-runtime/src/models/recall.rs` | Domain | MemoryContext + Intent | `token_count` 不生效 |
| `crates/memory-runtime/src/models/memory_item.rs` | Domain | MemoryItem + MemoryType | 💀 死代码，无任何读写 (D3) |
| `crates/memory-runtime/src/confidence/mod.rs` | Service | BetaConfidence | 无 pipeline 调用者 (D5) |
| `crates/memory-runtime/src/entity/mod.rs` | Service | canonical_key() + EntityNormalizer | 无 pipeline 调用者 (D7) |
| `crates/memory-runtime/src/pipeline/ingest.rs` | Application | SessionParser + 两实现 | |
| `crates/memory-runtime/src/pipeline/extract.rs` | Application | extract_observations() | 无去重、无归一化、无分块 |
| `crates/memory-runtime/src/store/traits.rs` | Infrastructure | Store trait 定义 | `update_status` 接受裸 `&str` |
| `crates/memory-runtime/src/store/connection.rs` | Infrastructure | Database struct | 连接 move 后变空壳 |
| `crates/memory-runtime/src/store/concept_store.rs` | Infrastructure | 最大 store 实现 | LIKE 搜索 JSON 有假阳性 |
| `crates/memory-runtime/src/store/observation_store.rs` | Infrastructure | Observation CRUD | 无 LIMIT、每次 17 个堆分配 |
| `crates/memory-runtime/src/llm/traits.rs` | Interface | LlmProvider async trait | |
| `crates/memory-runtime/src/embed/traits.rs` | Interface | EmbeddingService trait | 无实现 |
| `crates/memory-runtime/src/migrations/001_initial.sql` | Infrastructure | 完整 DDL | 缺 link 表、feedback 表 |
| `crates/sonny-cli/src/main.rs` | Entry | CLI binary | 3/8 命令实现 |
| `crates/memory-test-fixtures/src/mock_llm.rs` | Shared | MockLlmProvider | |
| `crates/memory-runtime/tests/integration_test.rs` | Verification | 7 个集成测试 | 无多 store 并发测试 |

## 设计文档

| Path | Description |
|------|-------------|
| `docs/memory-runtime-design.md` | 1307 行完整技术设计文档，定义闭环、对象模型、9 阶段生长流程 |
