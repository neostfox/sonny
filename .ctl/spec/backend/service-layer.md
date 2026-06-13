# Service Layer Spec — entity/ and confidence/

## Purpose

Cross-cutting domain services: entity normalization (canonical key generation + alias management) and confidence scoring (beta-distribution Bayesian updates).

设计文档 §5.3 要求实体重叠作为聚类依据（entity normalization），§5.6 要求证据升权降权（confidence scoring）。两个服务的数学模型完整，但都没有 pipeline 集成点。

## Directories

- `crates/memory-runtime/src/entity/` — Entity normalization
- `crates/memory-runtime/src/confidence/` — Confidence scoring

## Allowed Imports

- `crate::models` — domain types (minimal usage)
- `crate::error::MemoryResult` — error handling
- `rusqlite::Connection` — entity normalization reads entity_alias table (entity only)
- `unicode_normalization` — NFKC normalization (entity only)
- `std::fmt` — Display impl (confidence only)

## Forbidden Imports

- `crate::pipeline` — services don't orchestrate workflows
- `crate::store::traits` — services access DB directly via `&Connection`, not through store traits
- `reqwest`, `tokio` — no async I/O

## Patterns

### Entity normalization pipeline

```rust
// crates/memory-runtime/src/entity/mod.rs:23-62
pub fn canonical_key(raw: &str) -> String
```

Pipeline: trim → NFKC normalize → strip PascalCase suffixes → lowercase → normalize separators → collapse underscores → strip underscore suffixes.

Example transformations:
- `UserModel` → `user`
- `user_service` → `user`
- `POSMASK` → `posmask`
- `order-table` → `order`

### EntityNormalizer with database aliases

```rust
// crates/memory-runtime/src/entity/mod.rs:107-109
pub struct EntityNormalizer<'a> {
    conn: &'a Connection,
}
```

Reads `entity_alias` table for confirmed aliases. Falls back to `canonical_key()` when no alias found.

### Beta-distribution confidence

```rust
// crates/memory-runtime/src/confidence/mod.rs:4-7
pub struct BetaConfidence {
    pub alpha: f32,
    pub beta: f32,
}
```

- Positive evidence increments `alpha` (e.g., user confirmation: +2.0)
- Negative evidence increments `beta` (e.g., user negation: +3.0)
- Confidence = `alpha / (alpha + beta)`
- Prior: `alpha = 1.0, beta = 1.0` (uniform)

### Evidence weight table

```rust
// crates/memory-runtime/src/confidence/mod.rs:46-60
const EVIDENCE_WEIGHTS: [(EvidenceType, f32, f32); 13] = [
    (EvidenceType::UserConfirmation,    2.0, 0.0),
    (EvidenceType::UserNegation,        0.0, 3.0),
    // ...
];
```

13 evidence types with fixed (alpha_delta, beta_delta) weights. `LongInactivity` requires caller-scaled beta.

## Design Gaps

### D5: BetaConfidence 无 pipeline 集成点

设计 §5.6 Validate 阶段要求证据自动升降权。实现中：

| 自动化行为 | 代码 | 调用者 | 状态 |
|-----------|------|--------|------|
| 用户确认 → alpha+2 | `BetaConfidence::update(UserConfirmation)` | 无 | ❌ |
| 多次重复 → alpha+1 | `BetaConfidence::update(RepeatedOccurrence)` | 无 | ❌ |
| 用户否定 → beta+3 | `BetaConfidence::update(UserNegation)` | 无 | ❌ |
| 长期未使用 → beta 衰减 | `BetaConfidence::update_with_decay(days)` | 无 | ❌ |
| cross-session 3次 → auto_confirm | `ConfidenceConfig.auto_confirm_sessions` | 无 | ❌ |

**后果**: DB 中所有 `evidence_alpha` 和 `evidence_beta` 永远是初始值 `1.0`。`Observation.confidence` 永远是 `0.5`。自动确认/自动降权永远不会发生。

**需要**:
1. extract 后根据 `ObservationSourceType` 调用 `update()` 计算初始 confidence
2. 后台任务定期扫描 long-inactive concepts 调用 `update_with_decay()`
3. feedback 后根据反馈类型调用 `update()`

### D7: Entity Normalization 无 pipeline 调用者

`canonical_key()` 有 11 个单元测试，`EntityNormalizer` 有 DB-backed alias 查询。但：

- `extract_observations()` 不调用 entity normalization
- Observation 的 `subject_text` / `object_text` 是 LLM 返回的原始字符串
- 同一实体可能被表述为 "POSMASK"、"POSMASK 表"、"posmask"——存储为不同记录

**影响**: 设计 §5.3 Cluster 阶段要求“实体重叠”作为聚类依据。如果实体名称没有归一化，聚类无法工作。

**需要**: extract 后对 `subject_text` 和 `object_text` 执行 `canonical_key()` 归一化。用归一化后的 key 做 clustering。

## Anti-patterns

| Don't | Why | Instead |
|-------|-----|---------|
| Normalize at store time only | Aliases may change | Normalize at query time too |
| Hard-code confidence thresholds in domain types | Configuration varies by deployment | Use `ConfidenceConfig` from `config.rs` |
| Skip NFKC normalization | Unicode equivalence causes duplicate entities | Always normalize first |
| Access store traits from entity/confidence | Creates circular dependency | Use `&Connection` directly in `EntityNormalizer` |

## Testing

- `canonical_key()` has extensive unit tests covering all suffix types
- `BetaConfidence` tests verify weight application, mean calculation, Display format
- Integration tests verify entity normalization round-trip with database
- **缺失**: pipeline 集成测试 — extract 后 confidence 是否更新
- **缺失**: pipeline 集成测试 — extract 后 entity 是否归一化
