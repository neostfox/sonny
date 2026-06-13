# Application Layer Spec — pipeline/

## Purpose

Orchestrates the ingest and extraction workflows: parse raw session files into `RawMemory` records, then use LLM to extract structured `Observation` triples.

这是设计文档 §5 概念生长流程的前两个阶段（Observe + Extract）。后续 7 个阶段（Cluster, Name, Link, Validate, Promote, Use, Revise）尚未实现。

## Directory

`crates/memory-runtime/src/pipeline/`

## Allowed Imports

- `crate::models::*` — domain types
- `crate::llm::traits::LlmProvider` — LLM interface (trait only)
- `crate::error::{MemoryError, MemoryResult}` — error handling
- `serde::Deserialize` — parsing LLM responses
- `regex::Regex` — session parsing

## Forbidden Imports

- `rusqlite` — use store traits instead
- `crate::store::*` — pipeline should receive stores as parameters, not import them
- `reqwest` — use `LlmProvider` trait
- `tokio` runtime — functions are `async` but don't spawn tasks

## Patterns

### Session parser trait

```rust
// crates/memory-runtime/src/pipeline/ingest.rs:6-9
pub trait SessionParser: Send + Sync {
    fn parse(&self, content: &str, workspace_id: &str, source_ref: &str) -> MemoryResult<Vec<RawMemory>>;
    fn can_parse(&self, content: &str, filename: &str) -> bool;
}
```

Two implementations: `TrellisJournalParser` (markdown journals) and `JsonSessionParser` (structured JSON). The `detect_and_parse()` function auto-selects.

### LLM-powered extraction with JSON repair

```rust
// crates/memory-runtime/src/pipeline/extract.rs:99-128
pub async fn extract_observations(
    raw_memories: &[RawMemory],
    llm: &(impl LlmProvider + Sync),
) -> MemoryResult<Vec<Observation>>
```

- Takes `&[RawMemory]` and any `LlmProvider` impl
- Returns `Vec<Observation>` — pure domain output
- Includes `repair_json()` fallback for malformed LLM output
- LLM prompt is `const EXTRACTION_SYSTEM_PROMPT` with structured JSON schema

### Generic trait bounds over concrete impls

The `extract_observations` function uses `impl LlmProvider + Sync` rather than `Box<dyn LlmProvider>`. This avoids allocation and supports both real and mock providers without trait object overhead.

## Design Gaps

### D6: Extract 后无去重步骤

设计 §3.3 离线链路明确包含 Deduplication。`ObservationStore::check_duplicate()` 存在但 `extract_observations()` 从未调用。

**影响**: 两次 ingest 同一 session 产生重复 Observations。两个 session 讨论同一话题产生语义重复（表述不同但内容相同的三元组）。

**需要**:
1. 在 extract 后对每个 Observation 调用 `check_duplicate()`
2. 语义去重需要 embedding 相似度（当前无实现）

### D7: Extract 后无实体归一化

`canonical_key()` 和 `EntityNormalizer` 存在但 extract pipeline 不调用。LLM 返回的 `subject_text` 和 `object_text` 是原始字符串。

**影响**: 同一实体（"POSMASK"、"POSMASK 表"、"posmask"）存储为不同值，后续聚类无法匹配。

**需要**: extract 后对 `subject_text` 和 `object_text` 执行归一化。

### D10: Session Distiller 阶段缺失

设计 §12.2 定义了 Session Distiller：从完整 session 的全局视角提炼结构化记忆（架构事实、Bug修复路径等）。

当前 `extract_observations()` 从每条消息独立抽取三元组。区别：

| | Observation Extractor | Session Distiller |
|---|---|---|
| 视角 | 每条消息 | 整个 session |
| 输出 | 独立三元组 | 结构化记忆（MemoryItem） |
| 上下文理解 | 无跨消息推理 | 需要理解 session 全局 |

### 无长 session 分块策略

`format_messages()` 把所有 RawMemory 拼成一个 prompt。无 token 限制检查。长 session 会超过 LLM context window。

### 无 extraction prompt 版本管理

`EXTRACTION_SYSTEM_PROMPT` 是 hardcoded const。如果修改 prompt，之前提取的 Observations 有不同语义，但无版本标记。

## Anti-patterns

| Don't | Why | Instead |
|-------|-----|---------|
| Call store methods inside pipeline functions | Couples orchestration to infrastructure | Return results, let the caller persist |
| Hard-code a specific LLM provider | Breaks testability | Accept `impl LlmProvider` parameter |
| Use `unwrap()` on LLM responses | LLM output is unreliable by nature | Use `repair_json()` + `MemoryError::LlmInvalidJson` |
| Skip the `can_parse()` check | Silent wrong-format parsing | Always check via `SessionParser::can_parse()` |
| 发送超长 prompt 给 LLM | 超过 context window 导致截断 | 分块发送 + 合并结果 |
| 跳过去重直接写入 | 重复 session 产生重复数据 | extract 后调用 check_duplicate |

## Testing

- Unit tests with `MockLlmProvider` from `memory-test-fixtures`
- Integration tests in `crates/memory-runtime/tests/integration_test.rs`
- Test JSON repair logic with malformed inputs (see `extract.rs` `#[cfg(test)]` module)
