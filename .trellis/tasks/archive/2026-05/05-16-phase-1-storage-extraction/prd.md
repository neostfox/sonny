# Phase 1: 存储与抽取

## Goal

实现 Memory Runtime 的存储与抽取层：将原始对话 session 导入系统，通过 LLM 提取结构化 observation，用 Beta-Bernoulli 管理置信度，通过实体规范化保证实体一致性，并提供 CLI 命令操作这些功能。

## What I already know

* Rust workspace 已搭建完成：3 crate（memory-runtime / sonny-cli / memory-test-fixtures）
* 所有 domain models 已定义（Observation, Concept, RawMemory, Evidence 等）
* Store traits 已定义（RawMemoryStore, ObservationStore, ConceptStore, EmbeddingStore）
* SQLite schema 已完成（migrations/001_initial.sql，6 张表 + 索引）
* LlmProvider trait 已定义，MockLlmProvider 已实现
* EmbeddingService trait 已定义（EMBEDDING_DIM = 512）
* Error handling 已完善（MemoryError enum）
* Config 系统已完成（Settings + 各子配置）
* CLI 目前只是占位符（打印欢迎信息）

## Roadmap 任务映射

| 任务 | 状态 | 说明 |
|------|------|------|
| 1.1 SQLite schema + migration | **已完成** | migration.rs + 001_initial.sql |
| 1.2 Session ingest（markdown/JSON/Trellis journal） | 待实现 | 需支持 ≥2 种格式 |
| 1.3 实体规范化层 | 待实现 | 大小写归一、后缀剥离、别名表 |
| 1.4 LLM Observation Extractor | 待实现 | 结构化 JSON 输出 + source_type 标注 |
| 1.5 Beta-Bernoulli confidence updater | 待实现 | ~80 行纯 Rust，零外部依赖 |
| 1.6 CLI: `memory ingest-session`, `memory list-observations` | 待实现 | |

## Assumptions (temporary)

* Session ingest 优先支持 Trellis journal 格式 + markdown 格式（项目本身使用 Trellis）
* LLM Observation Extractor 使用已有的 LlmProvider trait，prompt 模板内置
* 实体规范化层是一个独立模块，被 extractor 调用
* CLI 使用 clap v4，子命令结构映射到 pipeline 各阶段
* Store 实现使用 rusqlite 直接操作 SQLite

## Research References

* [`research/markdown-parsing.md`](research/markdown-parsing.md) — pulldown-cmark 推荐，trait-based SessionParser，Trellis journal 解析策略
* [`research/entity-normalization.md`](research/entity-normalization.md) — 7 步规范化管道，strsim 模糊匹配，unicode-normalization NFKC
* [`research/llm-extraction-prompts.md`](research/llm-extraction-prompts.md) — system/user split prompt，5 个 few-shot 示例，混合 predicate 词汇表

## Open Questions

* ~~Session ingest 第二种格式选什么？~~ → **JSON**（易实现 + 可复用为测试数据格式）
* ~~LLM extraction prompt 如何设计？~~ → system/user split + 5 few-shot 示例 + hybrid predicate 词汇表
* ~~实体规范化的后缀剥离规则？~~ → 静态后缀列表 + 7 步规范化管道

## Requirements (evolving)

### Store 实现
* RawMemoryStore: 插入、按 session_id 查询、按时间范围查询、去重检查
* ObservationStore: 插入、按 status 查询、按 concept_candidate_id 查询、状态转换、实体查询
* ConceptStore: 插入、查询候选/确认概念、别名管理
* 所有 store 使用 rusqlite 实现 trait 接口
* 所有 JSON 列有应用层校验（serde 反序列化）

### Session Ingest
* 解析原始 session 文件为 RawMemory 记录
* **格式 1**: Trellis journal（markdown，## Session N 分隔，**Key**: Value 元数据）
* **格式 2**: JSON（结构化 {session_id, messages: [{role, content}]} 格式）
* 自动格式检测（基于内容/路径启发式）
* 去重：相同 session_id 不重复导入
* SessionParser trait + per-format 实现

### 实体规范化
* 7 步确定性管道：trim → NFKC → lowercase → separator normalize → collapse → suffix strip → (optional version strip)
* SQLite alias table 查询（exact match → fuzzy fallback）
* 自动别名发现：canonical key collision → auto-alias
* 保留原始 subject_text/object_text，canonical key 仅用于查重和匹配
* 新增依赖：`unicode-normalization`, `strsim`

### LLM Observation Extractor
* System prompt: 角色定义 + 提取规则 + JSON schema + 5 个 few-shot 示例
* User prompt: 格式化的 RawMemory 对话内容
* 输出: `{ "observations": [...] }` 结构化 JSON
* JSON repair fallback（strip markdown fences, trim whitespace）
* 使用 LlmProvider::complete_json 调用
* MockLlmProvider 返回预定义响应用于测试

### Beta-Bernoulli Confidence
* BetaConfidence struct { alpha: f32, beta: f32 }
* 初始 prior: Beta(1, 1) → confidence = 0.5
* Evidence weight table 实现（quality-control.md 定义的 13 种类型）
* `update(evidence_type)` 方法
* `confidence()` 方法
* `display()` 方法（可读展示）
* ~80 行纯 Rust，零外部依赖

### CLI (clap v4)
* `memory init` — 初始化数据库（run migrations）
* `memory ingest-session <path> [--workspace <id>]` — 导入 session 文件
* `memory list-observations [--session <id>] [--status <status>]` — 列出 observations
* 所有命令有 --help

## Acceptance Criteria

* [ ] RawMemoryStore / ObservationStore / ConceptStore 有完整 rusqlite 实现
* [ ] SessionParser trait + TrellisJournalParser + JsonSessionParser 实现
* [ ] 自动格式检测工作正常
* [ ] 实体规范化层：canonical_key() 对中英文混合实体正确归一化
* [ ] 实体规范化层：alias table lookup (exact + fuzzy fallback) 工作正常
* [ ] LLM Extractor + MockLlmProvider 集成测试通过
* [ ] BetaConfidence 单元测试覆盖所有 evidence type
* [ ] CLI: `memory init`, `memory ingest-session`, `memory list-observations` 可用
* [ ] 集成测试：ingest → extract → store 完整流程
* [ ] `cargo clippy` 无 warning

## Definition of Done

* Unit tests: 每个 store 方法、实体规范化、BetaConfidence
* Integration test: ingest → extract → store 端到端
* `cargo check` / `cargo test` / `cargo clippy` green
* Store 实现有完整的错误处理（MemoryError）
* CLI 命令有 --help 文本

## Technical Approach

1. **Store 实现** — rusqlite 实现 store traits，所有 SQL 参数化，JSON 列通过 serde 校验
2. **Session Ingest** — pulldown-cmark 解析 markdown，serde_json 解析 JSON，SessionParser trait 统一接口
3. **实体规范化** — 7 步确定性管道 + SQLite alias table + strsim fuzzy fallback
4. **LLM Extractor** — system prompt（静态）+ user prompt（动态对话）+ JSON repair
5. **Beta-Bernoulli** — 纯 Rust struct，f32 alpha/beta，evidence weight HashMap
6. **CLI** — clap derive API，子命令映射到 pipeline 各阶段

### 新增依赖

```toml
# workspace Cargo.toml
pulldown-cmark = "0.13"
regex = "1"
unicode-normalization = "0.1"
strsim = "0.11"
clap = { version = "4", features = ["derive"] }
```

## Decision (ADR-lite)

**Context**: Session ingest 需要支持 ≥2 种格式，Trellis journal 已确定。
**Decision**: 第二种格式选择 JSON（而非 Claude 对话 markdown）。
**Consequences**: JSON 更易实现（~100 行 vs ~250 行），可直接复用为测试数据格式。Claude 对话格式可在后续 phase 按需添加为第三种 SessionParser 实现。

## Out of Scope (explicit)

* Embedding service 实现（Phase 2）
* Clustering / concept naming（Phase 2）
* Recall pipeline（Phase 3）
* Adversarial validation（Phase 4）
* Predictive coding（Phase 5）
* 多 workspace 支持（后续）
* HTTP API / daemon 模式（Phase 3）

## Technical Notes

* Store 实现参考已有 trait 定义：crates/memory-runtime/src/store/traits.rs
* 数据库 schema：crates/memory-runtime/src/store/migrations/001_initial.sql
* Beta-Bernoulli 权重表：.trellis/spec/memory-runtime/quality-control.md
* Concept growth pipeline 定义：.trellis/spec/memory-runtime/concept-growth.md
* LlmProvider 接口：crates/memory-runtime/src/llm/traits.rs
