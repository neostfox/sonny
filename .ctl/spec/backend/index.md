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
    EXT --> CLT["Cluster<br/>✅ P3-B"]
    CLT --> NAM["Name<br/>⚠️ cluster→candidate"]
    NAM --> LNK["Link<br/>⚠️ merge/split"]
    LNK --> VAL["Validate<br/>✅ confidence"]
    VAL --> PRO["Promote<br/>⚠️ candidate→concept"]
    PRO --> USE["Use / Recall<br/>✅ P4-A"]
    USE --> REV["Revise / Feedback<br/>⏳ P4-B"]
    REV --> OBS

    style OBS fill:#c8e6c9
    style EXT fill:#c8e6c9
    style CLT fill:#c8e6c9
    style NAM fill:#fff9c4
    style LNK fill:#fff9c4
    style VAL fill:#c8e6c9
    style PRO fill:#fff9c4
    style USE fill:#c8e6c9
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
        OBS --> CLUSTER["Cluster (P3-B)"]
        CLUSTER --> CC["ConceptCandidate"]
        CC --> MERGE["Merge/Split (P3-C)"]
    end

    subgraph 实时链路
        USER["用户提问"] --> RECALL["RecallEngine (P4-A)"]
        RECALL --> INTENT["Intent 分类"]
        RECALL --> ENTITY["实体匹配"]
        RECALL --> SEMANTIC["Embedding 语义搜索"]
        RECALL --> CTX["MemoryContext"]
        CTX --> AGENT["Coding Agent"]
    end

    subgraph 修正链路
        FEED["用户反馈"] --> JUDGE["判断类型<br/>⏳ P4-B"]
        JUDGE --> UPDATE["更新 confidence"]
        JUDGE --> REJECT["生成 rejected_hypothesis"]
        UPDATE --> REV2["修正概念边界"]
    end

    MERGE --> RECALL
    AGENT --> FEED
```

### Layer Diagram（代码结构）

```mermaid
graph TD
    CLI["sonny-cli/ — Entry Point<br/>clap command dispatch"]
    PIPE["pipeline/ — Application Layer<br/>ingest + extract + cluster + merge"]
    RECALL["recall/ — Application Layer<br/>RecallEngine: intent + entity + semantic"]
    MODELS["models/ — Domain Layer<br/>data types, enums, status machines"]
    CONF["confidence/ — Service Layer<br/>beta-distribution scoring"]
    ENTITY["entity/ — Service Layer<br/>entity normalization"]
    STORE["store/ — Infrastructure Layer<br/>SQLite persistence + migrations"]
    LLM["llm/ — Interface Layer<br/>async LLM provider trait"]
    EMBED["embed/ — Interface + Impl Layer<br/>embedding provider trait + OpenAI impl"]
    CONFIG["config.rs — Shared<br/>settings with env overrides"]

    CLI --> PIPE
    CLI --> STORE
    PIPE --> MODELS
    PIPE --> LLM
    PIPE --> STORE
    RECALL --> MODELS
    RECALL --> STORE
    RECALL --> EMBED
    STORE --> MODELS
    STORE --> MIG["migrations/"]
    ENTITY --> MODELS
    CONF --> MODELS
    EMBED --> CONFIG

    style MODELS fill:#e1f5fe
    style CLI fill:#fff3e0
    style STORE fill:#fce4ec
    style LLM fill:#e8f5e9
    style EMBED fill:#e8f5e9
    style CONFIG fill:#f3e5f5
    style CONF fill:#e8eaf6
    style ENTITY fill:#e8eaf6
    style RECALL fill:#c8e6c9
```

### Status Machine — Observation

```mermaid
stateDiagram-v2
    [*] --> Candidate
    Candidate --> FastStored : auto-store
    FastStored --> Confirmed : user confirm
    FastStored --> AutoConfirmed : cross-session 3+<br/>✅ EvidenceType 支持
    Candidate --> Rejected : user negate
    Confirmed --> Deprecated : superseded
    Confirmed --> Disputed : conflicting evidence
    Confirmed --> Orphan : entity removed
    Candidate --> Superseded : reextract
    Rejected --> [*]
    Deprecated --> [*]
```

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

### Status Machine — ConceptCandidate

```mermaid
stateDiagram-v2
    [*] --> Candidate
    Candidate --> Merged : merge_group<br/>P3-C
    Candidate --> Split : split_candidate<br/>P3-C
    Candidate --> Active : promote
    Merged --> Active : promote
    Split --> Active : promote
```

### Dependency Direction

```
sonny-cli → pipeline → models
sonny-cli → store → models
pipeline → llm (trait only)
pipeline → models
pipeline → store
recall → models
recall → store (ConceptStore, EmbeddingStore)
recall → embed (EmbeddingProvider)
entity → models (pure function, no I/O)
confidence → models (pure function, no I/O)
store → models
store → migrations
embed → config (Settings)
```

**Violations**: None. Clean layered architecture.

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
| [design-reference.md](./design-reference.md) | 设计文档章节 → 代码映射 + 实现状态 | All | Needs Refresh |
| [directory-structure.md](./directory-structure.md) | Annotated file tree with roles | All | Refreshed |
| [domain-layer.md](./domain-layer.md) | Models, enums, status machines | Domain | Refreshed |
| [application-layer.md](./application-layer.md) | Pipeline: ingest + extract + cluster + merge | Application | Refreshed |
| [service-layer.md](./service-layer.md) | RecallEngine + Entity + Confidence | Service | Refreshed |
| [infrastructure-layer.md](./infrastructure-layer.md) | SQLite stores + 6 migrations | Infrastructure | Refreshed |
| [interface-layer.md](./interface-layer.md) | LLM trait + Embedding provider trait + OpenAI impl | Interface | Refreshed |
| [error-handling.md](./error-handling.md) | Error patterns with thiserror | All | Refreshed |
| [quality-guidelines.md](./quality-guidelines.md) | Forbidden/required patterns + known issues | All | Refreshed |
| [../guides/cross-layer-thinking-guide.md](../guides/cross-layer-thinking-guide.md) | Pipeline checklists + dependency direction | All | Needs Refresh |
