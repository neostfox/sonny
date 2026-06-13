# Domain Layer Spec — models/

## Purpose

Pure data types representing the memory domain: observations, concepts, evidence, confidence, and recall context. No I/O, no side effects, no database imports.

设计文档 §4 定义了 7 个核心对象。当前实现了 6 个（RawMemory, Observation, MemoryItem, ConceptCandidate, Concept, MemoryContext/Evidence）。全部 MemoryItem 是死代码。

## Directory

`crates/memory-runtime/src/models/`

## Allowed Imports

- `serde::{Deserialize, Serialize}` — serialization derive
- `super::status::*` — internal cross-reference between model submodules
- `std` — standard library only

## Forbidden Imports

- `rusqlite` — persistence belongs in `store/`
- `crate::store` — no dependency on infrastructure
- `crate::pipeline` — no dependency on application layer
- `reqwest`, `tokio` — no async runtime

## Patterns

### Enum with string serialization

All enums use `#[serde(rename_all = "snake_case")]` and implement `as_str()` for database storage.

```rust
// crates/memory-runtime/src/models/status.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Candidate,
    FastStored,
    Confirmed,
    // ...
}

impl ObservationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            // ...
        }
    }
}
```

### Struct with JSON-serialized fields

Complex nested data stored as `Option<String>` JSON blobs. Serialization happens at the store boundary.

```rust
// crates/memory-runtime/src/models/concept.rs
pub struct Concept {
    pub concept_id: String,
    pub known_facts_json: Option<String>,
    pub rejected_hypotheses_json: Option<String>,
    // ...
}
```

### IDs are UUID strings

All entity IDs are `String` (UUID v4), generated at creation time. No auto-increment IDs exposed in domain types.

### Confidence via beta distribution parameters

`evidence_alpha` and `evidence_beta` fields on `Observation`, `ConceptCandidate`, and `Concept`. Confidence = `alpha / (alpha + beta)`.

设计文档 §5.6 定义了升权/降权条件。数学模型在 `confidence/mod.rs`，但当前所有 confidence 字段停留在初始值 `(1.0, 1.0)`。

**决策 (2026-06-13, p1c)**: Observation 的 confidence 拆为双维度：`extraction_confidence`（提取时由 source_type 固定，不可变）与 `fact_confidence`（上述 Beta 后验）。recall 排序用 `effective_confidence = extraction_confidence × fact_confidence`。Concept/Candidate 无 extraction 维度，只用 `fact_confidence`。

## Design Gaps

### D2: Observation 三元组无法承载设计要求的丰富语义

设计 §4.2 要求 Observation 能表达：

```
架构事实、Bug修复路径、排障路径、数据资产、
用户偏好、被否定假设、开放问题、任务状态
```

当前 `Observation` 结构：

```rust
pub struct Observation {
    pub subject_text: String,    // 实体名
    pub predicate: String,       // 关系（自由文本）
    pub object_text: Option<String>, // 目标实体
    // ...
}
```

**缺失**:

| 设计要求 | 当前状态 | Gap |
|---------|---------|-----|
| 排障路径（有序步骤序列） | 无法表达 | 三元组是扁平关系 |
| 任务状态（progress + open_questions） | 无法表达 | 需要结构化字段 |
| memory_type_candidate | 字段不存在 | 设计 §12.1 要求 |

**影响**: extract 产出的 Observations 全部是扁平三元组。概念生长阶段无法区分知识类型。

**可选方案**:
1. 在 Observation 上加 `memory_type_candidate: Option<MemoryType>` 字段
2. 让 predicate 承担更多语义（如 `troubleshooting_step`）
3. 新增 `observation_detail_json` 存储类型特定的结构化数据

### D3: MemoryItem 是死代码

设计 §4.3 定义 MemoryItem 为 9 种类型的长期记忆单元。设计 §16 的数据流：

```
Observation → MemoryItem → ConceptCandidate → Concept
```

当前状态：
- `models/memory_item.rs`: MemoryItem + MemoryType 定义存在
- `migrations/001_initial.sql`: memory_item 表存在
- 无 store trait、无 pipeline stage、无代码读写

**决策 (2026-06-13, p1a)**: 删除 MemoryItem 独立实体。`MemoryType` 枚举保留，迁移为 Observation 的 `memory_type_candidate: Option<MemoryType>` 字段（配合 `observation_detail_json`）。聚类直接基于 Observation（P3-B）。依据：MemoryItem 无任何 store/pipeline 调用（死代码）；设计 §5 核心生长流程 Cluster 阶段以 Observation 为输入；增加中转层违反 YAGNI 与"概念生长为核心"原则。代码删除为后续实现任务。

### D4: ConceptCandidate 复用 ObservationStatus

```rust
// models/concept.rs:22
pub status: ObservationStatus,  // ❌ 包含 FastStored, Orphan 等无意义状态
```

设计 §4.4-4.5 定义了两个独立生命周期：

- **Observation**: Candidate → FastStored → Confirmed → AutoConfirmed → Rejected → Deprecated → Disputed → Orphan
- **ConceptCandidate/Concept**: Candidate → Active → Labile → Deprecated → Disputed

`ConceptStatus` 枚举已存在于 `status.rs`，应同时用于 `ConceptCandidate.status`。

## Anti-patterns

| Don't | Why | Instead |
|-------|-----|---------|
| Add methods that do I/O in model types | Domain layer must be pure | Put logic in `pipeline/` or `store/` |
| Import `chrono` or `uuid` for generation | Creates hidden side-effect dependency | Generate IDs/timestamps at the call site |
| Store `DateTime<Utc>` directly | rusqlite maps to `String` anyway | Use `String` for timestamps |
| 用 `ObservationStatus` 作为 ConceptCandidate 状态 | 破坏生命周期语义 | 定义独立的 `CandidateStatus` 或直接用 `ConceptStatus` |

## Testing

- Unit test with plain struct construction
- No database needed
- Test enum round-trip through `as_str()` → parse (parse functions live in `store/`)
