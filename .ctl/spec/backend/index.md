# Sonny Development Guidelines

> Memory Runtime for Coding Agents — 从历史对话经历中生长概念网络，在未来任务中精准召回。
> Language: Rust
> Build: cargo (workspace)
> Design Doc: `docs/memory-runtime-design.md`

---

## Architecture Overview

### 设计闭环（完整 9 阶段）

设计文档 §5 定义的概念生长流程：

```mermaid
flowchart LR
    OBS["Observe<br/>ingest"] --> EXT["Extract<br/>observation"]
    EXT --> CLT["Cluster<br/>❌ 未实现"]
    CLT --> NAM["Name<br/>❌ 未实现"]
    NAM --> LNK["Link<br/>❌ 未实现"]
    LNK --> VAL["Validate<br/>⚠️ 数学模型存在<br/>无集成点"]
    VAL --> PRO["Promote<br/>❌ 未实现"]
    PRO --> USE["Use / Recall<br/>❌ 未实现"]
    USE --> REV["Revise / Feedback<br/>❌ 未实现"]
    REV --> OBS

    style OBS fill:#c8e6c9
    style EXT fill:#c8e6c9
    style CLT fill:#ffcdd2
    style NAM fill:#ffcdd2
    style LNK fill:#ffcdd2
    style VAL fill:#fff9c4
    style PRO fill:#ffcdd2
    style USE fill:#ffcdd2
    style REV fill:#ffcdd2
```

绿色 = 已实现，黄色 = 部分实现，红色 = 未实现。

### 三条数据链路

设计文档 §3 定义了三条链路：

```mermaid
flowchart TB
    subgraph 离线链路
        HIST["历史 Session"] --> ING["Ingest"]
        ING --> RAW["RawMemory"]
        RAW --> EXTRACT["Extract"]
        EXTRACT --> OBS["Observation"]
        OBS --> DISTILL["Session Distiller<br/>❌ 未实现"]
        DISTILL --> MI["MemoryItem<br/>❌ 无代码"]
        MI --> CCBUILD["Concept Candidate Builder<br/>❌ 未实现"]
        CCBUILD --> CC["ConceptCandidate"]
    end

    subgraph 实时链路
        USER["用户提问"] --> IDENT["识别 workspace/repo/task<br/>❌ 未实现"]
        IDENT --> MATCH["概念匹配<br/>❌ 无语义搜索"]
        MATCH --> CTX["MemoryContext<br/>⚠️ 仅格式化"]
        CTX --> AGENT["Coding Agent"]
    end

    subgraph 修正链路
        FEED["用户反馈"] --> JUDGE["判断类型<br/>❌ 未实现"]
        JUDGE --> UPDATE["更新 confidence<br/>❌ 无调用者"]
        JUDGE --> REJECT["生成 rejected_hypothesis<br/>❌ 未实现"]
        UPDATE --> REV2["修正概念边界"]
    end

    CC --> MATCH
    AGENT --> FEED
```

### Layer Diagram（代码结构）

```mermaid
graph TD
    CLI["sonny-cli/ — Entry Point<br/>clap command dispatch"]
    PIPE["pipeline/ — Application Layer<br/>ingest + extract orchestration"]
    MODELS["models/ — Domain Layer<br/>data types, enums, status machines"]
    CONF["confidence/ — Service Layer<br/>beta-distribution scoring"]
    ENTITY["entity/ — Service Layer<br/>entity normalization"]
    STORE["store/ — Infrastructure Layer<br/>SQLite persistence + migrations"]
    LLM["llm/ — Interface Layer<br/>async LLM provider trait"]
    EMBED["embed/ — Interface Layer<br/>embedding service trait"]
    CONFIG["config.rs — Shared<br/>settings with env overrides"]

    CLI --> PIPE
    CLI --> STORE
    PIPE --> MODELS
    PIPE --> LLM
    PIPE --> STORE
    STORE --> MODELS
    STORE --> MIG["migrations/"]
    ENTITY --> MODELS
    CONF --> MODELS

    style MODELS fill:#e1f5fe
    style CLI fill:#fff3e0
    style STORE fill:#fce4ec
    style LLM fill:#e8f5e9
    style EMBED fill:#e8f5e9
    style CONFIG fill:#f3e5f5
    style CONF fill:#e8eaf6
    style ENTITY fill:#e8eaf6
```

### Status Machine — Observation

```mermaid
stateDiagram-v2
    [*] --> Candidate
    Candidate --> FastStored : auto-store
    FastStored --> Confirmed : user confirm
    FastStored --> AutoConfirmed : cross-session 3+<br/>⚠️ 无调用者
    Candidate --> Rejected : user negate
    Confirmed --> Deprecated : superseded
    Confirmed --> Disputed : conflicting evidence
    Confirmed --> Orphan : entity removed
    Rejected --> [*]
    Deprecated --> [*]
```

带 ⚠️ 的转换：数学模型存在但无 pipeline 代码触发。

### Status Machine — Concept

```mermaid
stateDiagram-v2
    [*] --> Candidate
    Candidate --> Active : sufficient evidence<br/>⚠️ 无 Promote 逻辑
    Active --> Labile : conflicting recall
    Labile --> Active : re-confirmed
    Active --> Deprecated : superseded
    Active --> Disputed : negation evidence
    Candidate --> Deprecated : rejected
    Deprecated --> [*]
    Disputed --> [*]
```

### Status Machine — ConceptCandidate（⚠️ 建模错误）

```mermaid
stateDiagram-v2
    [*] --> Candidate
    Candidate --> Candidate : 当前复用 ObservationStatus<br/>❌ 应有独立生命周期
```

`ConceptCandidate.status` 类型为 `ObservationStatus`，包含 `FastStored`、`Orphan` 等对候选概念无意义的状态变体。设计文档 §4.4 要求候选概念有独立生命周期，`ConceptStatus` 枚举已定义但未用于 Candidate。

### Dependency Direction

```
sonny-cli → pipeline → models
sonny-cli → store → models
pipeline → llm (trait only)
pipeline → models
entity → models (indirect)
confidence → (pure domain, no imports)
config → (shared, no imports)
store → models
store → migrations

recall → Phase 3 (stub)
feedback → Phase 3 (stub)
```

**Violations**: None. Clean layered architecture.

---

## Design Gaps Summary

| # | Gap | Design § | Impact | Decision Needed |
|---|-----|----------|--------|-----------------|
| D1 | ConceptCandidate 无法增量生长 | §5.3-5.5 | 聚类/链接阶段无法实现 | 需 `observation_candidate_link` 表 |
| D2 | Observation 三元组无法承载设计要求的丰富语义 | §4.2, §12.1 | 排障路径、任务状态等丢失结构 | 需 `memory_type_candidate` 字段或扩展模型 |
| D3 | MemoryItem 是设计核心中转站但无任何代码 | §4.3 | 数据流断裂 | 保留并实现 or 从设计删除 |
| D4 | ConceptCandidate 复用 ObservationStatus | §4.4-4.5 | Promote 时状态映射错误 | 改用独立枚举 |
| D5 | BetaConfidence 无 pipeline 集成点 | §5.6 | 所有 confidence 永远是 0.5 | 在 pipeline 中接入 update() |
| D6 | 去重是显式设计步骤但 pipeline 不执行 | §3.3 | 重复 session 产生重复 Observation | 在 extract 后调用 check_duplicate |
| D7 | Entity Normalization 有实现但无调用者 | §5.3 | 同一实体多种表述无法聚类 | extract 后接入 canonical_key() |
| D8 | Recall 无召回策略只有格式化 | §13 | 无法匹配概念 | 需语义搜索 + 意图识别 |
| D9 | Feedback 无数据模型 | §3.4 | 闭环断裂 | 需 feedback 表 + 关联 |
| D10 | Session Distiller 阶段缺失 | §12.2 | 只有碎片三元组无 session 级理解 | 需实现或合并到 extract |

---

## Directory Structure

See [directory-structure.md](./directory-structure.md) for the annotated tree.

---

## Pre-Development Checklist

Before writing code:
- [ ] Read the layer spec for the target module
- [ ] Check [design-reference.md](./design-reference.md) for the design doc section covering your change
- [ ] Verify change scope against dependency direction (see diagram above)
- [ ] Check [quality-guidelines.md](./quality-guidelines.md) for forbidden patterns
- [ ] Run `cargo check` from workspace root
- [ ] Run `cargo test` to confirm baseline passes

## Quality Check

After implementation:
- [ ] `cargo check` passes
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] No layer boundary violations (models must not import store/pipeline)
- [ ] New public types have `#[derive(Debug, Clone, Serialize, Deserialize)]` where appropriate
- [ ] Error cases use `MemoryError` variants, not `unwrap()` in library code
- [ ] 如果改动涉及设计文档 §5 的任何阶段，更新 design-reference.md 的实现状态
- [ ] 如果改动属于 roadmap 中的任务，更新 [roadmap.md](./roadmap.md) 中对应任务的状态

## Guidelines Index

| Guide | Description | Layer | Status |
|-------|-------------|-------|--------|
| [roadmap.md](./roadmap.md) | 项目路线图：P0-P4 阶段、依赖图、验收里程碑 | All | Active |
| [design-reference.md](./design-reference.md) | 设计文档章节 → 代码映射 + 实现状态 | All | Generated |
| [directory-structure.md](./directory-structure.md) | Annotated file tree with roles + known issues | All | Generated |
| [domain-layer.md](./domain-layer.md) | Models, enums, status machines + D2/D3/D4 | Domain | Generated |
| [application-layer.md](./application-layer.md) | Pipeline ingest + extract + D6/D7/D10 | Application | Generated |
| [infrastructure-layer.md](./infrastructure-layer.md) | SQLite stores + C1/C2/D1/D9 | Infrastructure | Generated |
| [interface-layer.md](./interface-layer.md) | LLM and embedding provider traits | Interface | Generated |
| [service-layer.md](./service-layer.md) | Entity/confidence + D5/D7 | Service | Generated |
| [error-handling.md](./error-handling.md) | Error patterns with thiserror | All | Generated |
| [quality-guidelines.md](./quality-guidelines.md) | Forbidden/required patterns + C1-M7 known issues | All | Generated |
| [../guides/cross-layer-thinking-guide.md](../guides/cross-layer-thinking-guide.md) | Pipeline checklists + dependency direction | All | Generated |
