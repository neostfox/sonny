# Cross-Layer Thinking Guide

When making changes that span multiple modules, consider the following:

## 概念生长 Pipeline Checklist

设计文档 §5 定义了 9 个阶段。实现任何后续阶段前：

### Cluster（D1 — 需要数据模型变更）

1. 新增 `observation_candidate_link` 表
2. 设计聚类算法：实体重叠 + embedding 相似度 + 时间邻近
3. Entity normalization 必须先接入 extract pipeline（D7）
4. Embedding 必须先实现（当前全是 stub）

### Name + Link

1. 调用 LLM 生成概念名称和摘要
2. 把 Observation 关联到 Candidate
3. 合并同一 Candidate 下的 known_facts

### Validate（D5 — 需要 pipeline 集成）

1. 在 extract 后根据 source_type 调用 `BetaConfidence::update()`
2. 后台 decay 任务扫描 long-inactive concepts
3. 设计 auto-confirm / auto-demote 触发逻辑

### Promote

1. `ConceptCandidate.status` 必须先改为 `ConceptStatus`（D4）
2. 实现 Candidate → Concept 的 promotion 逻辑
3. 保留 Candidate 作为历史记录

### Recall（D8）

1. 实现语义搜索（依赖 embedding 实现）
2. 实现 intent 分类（`Intent` 枚举已存在）
3. `MemoryContext.token_count` 必须生效（截断逻辑）
4. 实现 §13.4 的 token 预算分配

### Feedback（D9）

1. 新增 feedback 表
2. 实现反馈分类（Confirm/Negate/Supplement/Correct/Preference）
3. 反馈触发 confidence 更新
4. 反馈生成 rejected_hypothesis

## Adding a New Domain Type

1. Define the struct/enum in `models/` with standard derives
2. Add corresponding SQL table in `migrations/NNN_name.sql`
3. Update `migration.rs` to include the new migration
4. Add trait methods to `store/traits.rs`
5. Implement trait in a new `store/<name>_store.rs` file
6. Re-export from `store/mod.rs`

Checklist:
- [ ] Model has `Debug, Clone, Serialize, Deserialize`
- [ ] SQL table has appropriate indexes
- [ ] Store trait has `Send + Sync`
- [ ] Row-mapping helper handles `Option<T>` fields correctly
- [ ] Parse function for string → enum in store layer
- [ ] Integration test for CRUD round-trip

## Adding a New Pipeline Stage

1. New file in `pipeline/<name>.rs`
2. Accept trait parameters (stores, LLM) — don't import concrete impls
3. Return `MemoryResult<Vec<DomainType>>`
4. Register in `pipeline/mod.rs`
5. Wire into CLI in `sonny-cli/src/main.rs`

Checklist:
- [ ] Function takes `impl Trait` not `ConcreteType`
- [ ] No `rusqlite` imports in pipeline
- [ ] Error cases use `MemoryError` variants
- [ ] Unit test with mock LLM
- [ ] Integration test with in-memory DB

## Adding a New Provider

1. New file in `llm/` or `embed/` for the implementation
2. Implement the existing trait (`LlmProvider` or `EmbeddingService`)
3. Add constructor with configuration from `config.rs`
4. Add feature flag in `Cargo.toml` if dependency is heavy

Checklist:
- [ ] Implements all trait methods
- [ ] `Send + Sync` satisfied
- [ ] Errors wrapped in `MemoryError::*Error` variants
- [ ] Feature-gated if dependency is optional

## Modifying the Schema

1. Create new migration `migrations/NNN_name.sql` with next version number
2. Add to `MIGRATIONS` array in `migration.rs`
3. Update `CURRENT_VERSION` constant
4. Update store implementation if columns changed
5. Update model struct if fields changed
6. Run `cargo test` to verify migration from scratch

Checklist:
- [ ] Migration is idempotent (`CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`)
- [ ] Down migration not needed (append-only by convention)
- [ ] Existing data preserved (ADD COLUMN, not ALTER COLUMN type)
- [ ] Integration tests pass with fresh DB
- [ ] 如果新增关联表，更新 design-reference.md 的 D1 状态

## Dependency Direction Check

Before adding any `use` statement:

```
Allowed directions:
  CLI → Pipeline → Models
  CLI → Store → Models
  Store → Models
  Pipeline → LLM traits
  Pipeline → Models
  Pipeline → Service (entity, confidence)  ← 当前缺失的集成
  Entity → Models
  Confidence → (standalone)

Forbidden:
  Models → anything (must be pure)
  Store → Pipeline
  LLM traits → Store
  Pipeline → Store (use traits as parameters)
```
