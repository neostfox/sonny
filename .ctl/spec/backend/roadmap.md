# Roadmap（归档：P0–P4）

> **状态：本文件已归档（2026-07）。** P0–P4 及后续 P5–P8 已全部完成并通过验收测试。
>
> **当前路线与状态见仓库根目录 [`TODO.md`](../../../TODO.md)。** 本文件不再扩展新任务。
>
> 基于代码评审（10 项 Critical/High）、设计差距分析（D1-D10）、设计评审（P1-P10）综合制定。

---

## 阶段总览

```mermaid
gantt
    title Sonny Memory Runtime Roadmap
    dateFormat  YYYY-MM-DD
    section P0 关键修复
    Windows 兼容          :p0a, 2026-06-13, 1d
    连接所有权模型         :p0b, after p0a, 2d
    LIKE 搜索修复          :p0c, after p0b, 1d
    Store trait 类型安全   :p0d, after p0c, 1d
    section P1 设计决策
    MemoryItem 去留        :p1a, after p0d, 1d
    ConceptCandidate 状态  :p1b, after p1a, 1d
    Confidence 双维度      :p1c, after p1b, 2d
    section P2 管道加固
    提取验证层             :p2a, after p1c, 3d
    去重+归一化集成        :p2b, after p2a, 2d
    Observation 关系       :p2c, after p2b, 2d
    反向修正机制           :p2d, after p2c, 3d
    section P3 智能生长
    Embedding 实现         :p3a, after p2b, 5d
    聚类引擎              :p3b, after p3a, 5d
    Concept 合并/拆分      :p3c, after p3b, 3d
    Confidence 集成        :p3d, after p3b, 2d
    section P4 闭环
    Recall 引擎            :p4a, after p3b, 5d
    Feedback 系统          :p4b, after p4a, 3d
    时间感知+衰减          :p4c, after p4d, 2d
    动态 token 预算        :p4d, after p4a, 2d
```

---

## P0: 关键修复（阻塞性）

这些 bug 阻止系统在真实环境运行。不涉及设计变更，纯代码修复。

### P0-A: Windows 兼容 — `dirs_home()` 修复

| 项 | 值 |
|---|---|
| 问题 | C3: `config.rs` `dirs_home()` 用 `$HOME`，Windows 不存在 |
| 修复 | 引入 `dirs` crate，`dirs::home_dir().unwrap_or_else(std::env::temp_dir)` |
| 文件 | `crates/memory-runtime/src/config.rs`, `Cargo.toml` (workspace + crate) |
| 验收 | `cargo test` 在 Windows 上通过，`Settings::load()` 路径有效 |
| 依赖 | 无 |
| 状态 | ✅ 已完成 (`p0a-windows-compat`) — dirs v6，34 tests pass，2 gates PASS |

### P0-B: 连接所有权模型重构

| 项 | 值 |
|---|---|
| 问题 | C1: 每个 store move Connection，多 store 无法共享 |
| 修复 | `Database` 持有 `Arc<parking_lot::Mutex<Connection>>`，store clone Arc；`parking_lot::lock()` 免 unwrap |
| 文件 | `connection.rs`, 所有 store impl, `sonny-cli/main.rs`, `integration_test.rs` |
| 验收 | 单个 `Database` 可以创建多个 store，集成测试不再用 `mem::replace` |
| 依赖 | 无 |
| 状态 | ✅ 已完成 (`p0b-connection-ownership`) — 35 tests pass，新增 `test_multiple_stores_share_connection`，2 gates PASS |

### P0-C: `find_by_entities` LIKE 搜索替换

| 项 | 值 |
|---|---|
| 问题 | C2: `LIKE '%?%'` 搜索 JSON 产生假阳性 |
| 修复 | 新增 `entity_concept` 连接表（migration 002，WITHOUT ROWID），`find_by_entities` 改用 JOIN；insert/update_concept 同步连接表 |
| 文件 | `concept_store.rs`, `migrations/002_entity_concept.sql`, `migration.rs` |
| 验收 | 搜索 "user" 不再匹配 "user_profile" |
| 依赖 | P0-B |
| 状态 | ✅ 已完成 (`p0c-entity-concept-table`) — 37 tests pass，2 gates PASS |

### P0-D: Store trait 类型安全

| 项 | 值 |
|---|---|
| 问题 | H1: `update_status` 接受裸 `&str`; H3: parse 函数静默默认值 |
| 修复 | trait 方法改为接受枚举类型；parse 函数加 `tracing::warn!` |
| 文件 | `store/traits.rs`, 所有 store impl |
| 验收 | 编译时捕获状态拼写错误 |
| 依赖 | 无 |

### P0-E: 堆分配优化

| 项 | 值 |
|---|---|
| 问题 | H2: 每次 insert 17-23 个 `Box<dyn ToSql>` 分配 |
| 修复 | 改用 `params![]` 宏或元组 |
| 文件 | `observation_store.rs`, `concept_store.rs` |
| 验收 | 无功能变更，性能改善 |
| 依赖 | 无 |

---

## P1: 设计决策（所有后续工作的前提）

这些任务的结果决定后续阶段的实现方式。**必须逐一做决策并记录。**

### P1-A: MemoryItem 去留决策

| 项 | 值 |
|---|---|
| 问题 | D3: MemoryItem 是设计核心中转站但无代码 |
| 选项 A | 保留：实现 MemoryItem 作为 Observation → ConceptCandidate 的蒸馏层 |
| 选项 B | 删除：Observation 直接聚类成 ConceptCandidate，从设计文档移除 MemoryItem |
| 决策标准 | 如果 session 级蒸馏（P10/D10）需要产出结构化记忆，保留。如果聚类可以直接基于 Observation，删除 |
| 影响文件 | 设计文档 §4.3, §16; `models/memory_item.rs`; `migrations/001_initial.sql` |
| 依赖 | 无 |
| 状态 | ✅ 已完成 (`p1a-memory-item-decision`) — 决策：删除 MemoryItem 独立实体，9 种类型折叠为 `Observation.memory_type_candidate` |

### P1-B: ConceptCandidate 独立状态枚举

| 项 | 值 |
|---|---|
| 问题 | D4: `ConceptCandidate.status` 是 `ObservationStatus` |
| 修复 | 定义 `CandidateStatus` 枚举（Candidate → Merging → Ready → Promoted → Rejected） |
| 文件 | `models/status.rs`, `models/concept.rs`, `concept_store.rs` |
| 验收 | Candidate 状态转换与 Observation 完全独立 |
| 依赖 | 无 |
| 状态 | ✅ 已完成 (`p1b-candidate-status`) — CandidateStatus 枚举独立于 ObservationStatus |

### P1-C: Confidence 双维度设计

| 项 | 值 |
|---|---|
| 问题 | P2: 提取正确性和事实真实性共用一个 confidence 字段 |
| 设计 | 拆分为 `extraction_confidence`（由 source_type 决定，提取时固定）和 `fact_confidence`（由 beta 分布累积） |
| 影响 | Observation 模型、extract pipeline、recall 排序 |
| 产出 | 设计决策文档 + 模型变更方案 |
| 依赖 | 无 |
| 状态 | ✅ 已完成 (`p1c-confidence-dual-dimension`) — 决策：`extraction_confidence`（source_type 固定）× `fact_confidence`（Beta 后验）= `effective_confidence` |

---

## P2: 管道加固（核心数据质量）

让 ingest → extract 管道产出高质量 Observation。

### P2-A: 提取验证层

| 项 | 值 |
|---|---|
| 问题 | P4: LLM 提取无验证，幻觉直接入库 |
| 实现 | 1. `evidence_text` 必须是某条 RawMemory.content 的子串<br/>2. `confidence` 由 source_type 权重覆盖（不直接用 LLM 值）<br/>3. 提取结果带 prompt 版本号 |
| 文件 | `pipeline/extract.rs` 新增 `validate_extraction()` 函数 |
| 验收 | 幻觉 Observation 被过滤；test: 构造 LLM 返回不存在的 evidence_text，验证被拒绝 |
| 依赖 | P1-C（confidence 双维度） |
| 状态 | ✅ 已完成 (`p2a-extraction-validation`) — evidence_text 子串回验过滤幻觉 + `EXTRACTION_PROMPT_VERSION`；"confidence 由 source_type 覆盖" 已在 p1d 完成。44 tests pass |

### P2-B: 去重 + 实体归一化集成

| 项 | 值 |
|---|---|
| 问题 | D6: extract 后无去重; D7: extract 后无实体归一化; H4: Regex 每次编译 |
| 实现 | 1. extract 后对每个 Observation 调用 `check_duplicate()`<br/>2. 对 `subject_text`/`object_text` 执行 `canonical_key()`<br/>3. `extract_session_id` 改用 `LazyLock<Regex>` |
| 文件 | `pipeline/extract.rs`, `pipeline/ingest.rs` |
| 验收 | 同一 session ingest 两次不产生重复；同一实体不同表述归一化为同一 key |
| 依赖 | P0-D（store trait 改为接受枚举） |
| 状态 | ✅ 已完成 (`p2b-dedup-normalize`) — `canonical_key_light` 轻量归一（不剥后缀，UserService≠UserModel）+ `extract_and_dedup` + LazyLock session regex。46 tests pass |

### P2-C: Observation 共现关系

| 项 | 值 |
|---|---|
| 问题 | P3: Observation 之间无关系，聚类必须从零推断 |
| 实现 | 1. 同一次 `extract_observations()` 调用产出的 Observations 加 `extraction_batch_id`<br/>2. 新增 `observation_coclaim` 表记录同批提取<br/>3. 聚类时利用共现关系作为加分项 |
| 文件 | `models/observation.rs`（加字段），新 migration，`concept_store.rs` |
| 验收 | 来自同一次提取的 Observations 可查询到关联 |
| 依赖 | P2-A（验证层先就位） |
| 状态 | ✅ 已完成 (`p2c-observation-cooccurrence`) — `extract_observations()` 每次调用生成一个 `extraction_batch_id` 共享给整批；migration 004 加列 + `observation_coclaim` 关联表（规范 (a,b) 对 + `INSERT OR IGNORE` 幂等）；`insert_batch` 写入共现边；`ObservationStore::find_coclaim()` 查询同批关联。共现查询放在 `observation_store.rs`/`ObservationStore` 而非 `concept_store.rs`（coclaim 是 observation 关系，分层更清晰；P3-B 聚类可直接消费）。47 tests pass，含 coclaim 集成测试 |

### P2-D: 反向修正机制

| 项 | 值 |
|---|---|
| 问题 | P1: 管道单向，后期无法修正前期错误 |
| 实现 | 1. `raw_memory.extraction_version` 记录提取版本<br/>2. `reextract(session_id, new_prompt_version)` 重新提取并标记旧 Observation 为 superseded<br/>3. Observation 加 `superseded_by` 字段 |
| 文件 | `store/traits.rs`（新方法），`pipeline/extract.rs`，新 migration |
| 验收 | 修改提取 prompt 后，可以重新提取已有 session 并替换旧结果 |
| 依赖 | P2-A, P2-C |
| 状态 | ✅ 已完成 (`p2d-reverse-correction-2`；原 `p2d-reverse-correction` 因 scope 漏 `ingest.rs`/`status.rs`/tests 启动后无法 revise，已 cancel+archive) — migration 005 加 `raw_memory.extraction_version` + `observation.superseded_by`；新增 `ObservationStatus::Superseded` 变体；`RawMemoryStore::session_extraction_version`/`set_session_extraction_version` + `ObservationStore::mark_session_superseded`（按 session_id 经 raw_memory 关联定位旧 Observation）；`reextract(session_id)` 用 `extract_observations`（非 dedup，避免旧行干扰）→ 先 mark 旧为 superseded 再插新批 → stamp 当前 `EXTRACTION_PROMPT_VERSION`。旧行保留可追溯，recall 按 status 过滤。48 tests pass，含 reextract 集成测试 |

### P2-E: 谓词词表归一化 ✅

| 项 | 值 |
|---|---|
| 问题 | D2: predicate 自由文本，LLM 随意发挥 → 去重/聚类/冲突检测失效（"包含"≠"包括"≠"has_field"） |
| 实现 | 1. `Predicate` 枚举（7 canonical: has/not_has/depends_on/not_depends_on/related_to/not_related_to/causes）<br/>2. `normalize_predicate()` 在 `raw_to_observation` 归一化 LLM 输出（canonical + 同义词映射；未知谓词回退为小写 snake_case，**不 drop**——未知关系不是幻觉）<br/>3. extraction prompt 列出 canonical 词表引导 LLM<br/>4. `Predicate::is_negation_of()` 为冲突检测铺路（design §3.3）<br/>5. bump `EXTRACTION_PROMPT_VERSION` → 已提取 session 标记 stale，reextract 回填 canonical 谓词 |
| 文件 | 新文件 `models/predicate.rs`, `pipeline/extract.rs`（prompt + 归一化 + 版本） |
| 验收 | "has_field"/"contains"/"lacks" 等归一为 canonical；去重/聚类按 canonical 匹配；未知谓词保留不丢 |
| 依赖 | P2-A（验证层） |
| 状态 | ✅ 已实现；53 tests pass（含 5 个 predicate 单元测试） |

---

## P3: 智能生长（概念从数据中涌现）

### P3-A: Embedding 实现 ✅

| 项 | 值 |
|---|---|
| 问题 | 所有 embedding 代码曾是 stub，聚类和召回依赖真实向量 |
| 实现 | `EmbeddingProvider` 抽象 + OpenAI-compatible `/v1/embeddings` provider；默认 `BAAI/bge-m3`，1024 维；`embed_and_store()` 串起 provider → SQLite store |
| 文件 | `embed/traits.rs`, `embed/openai.rs`, `embed/mod.rs`, `store/embedding_store.rs`, `migrations/006_p3a_embedding_storage.sql` |
| 验收 | stub provider 覆盖 embed → store → cosine search；OpenAI-compatible 请求/响应 serde + 维度校验单测 |
| 依赖 | 无（可与 P2 并行） |
| 状态 | ✅ 已实现；sqlite-vec 延后为性能优化，当前默认 BLOB + Rust cosine |

### P3-B: 聚类引擎

| 项 | 值 |
|---|---|
| 问题 | 设计 §5.3 Cluster 完全未实现 |
| 实现 | 1. 对 Observation 计算 embedding<br/>2. HAC 层次聚类（`config.hac_threshold`）<br/>3. 利用实体重叠 + 共现关系 + embedding 相似度<br/>4. 产出 ConceptCandidate 并关联 Observation |
| 文件 | 新模块 `pipeline/cluster.rs` |
| 验收 | 多个 session 讨论同一话题时，产出合并的 ConceptCandidate |
| 依赖 | P2-C（共现关系）, P3-A（embedding）, P0-C（连接表） |

### P3-C: Concept 合并/拆分

| 项 | 值 |
|---|---|
| 问题 | P7: 无合并策略 |
| 实现 | 1. 检测候选间重叠（实体集合 Jaccard > threshold）<br/>2. 合并：取并集的 known_facts，保留两个名称作为 alias<br/>3. 拆分：基于子聚类将 Observation 分配到新候选 |
| 文件 | 新模块 `pipeline/merge.rs` |
| 验收 | 两个重叠候选合并为一个；一个过宽候选拆分为两个 |
| 依赖 | P3-B（聚类引擎） |

### P3-D: Confidence Pipeline 集成 ✅

| 项 | 值 |
|---|---|
| 问题 | D5: BetaConfidence 无调用者 → fact_confidence 恒为 0.5 |
| 实现 | 1. extract 时按 `source_type.initial_evidence()` 种子 α/β（FileEvidence/UserConfirm/UserNegation/AssistantGuess；UserMessage 不种子，等去重/跨 session 佐证）<br/>2. 去重命中（新 `find_duplicate` 返回命中行）对已存在 observation 累加 `update(RepeatedOccurrence)`<br/>3. （Phase 5）跨 session 匹配 `update(CrossSession*)` — 待 consolidation 引擎<br/>~~4. 后台 decay~~ → **移除**：衰减是召回侧乘性因子，不碰 α/β（见 P4-C / quality-control.md §Time Decay） |
| 文件 | `pipeline/extract.rs`, `store/observation_store.rs`, `store/traits.rs`, `models/observation.rs`, `confidence/mod.rs`（删 `update_with_decay`） |
| 验收 | 按 source 种子的 observation α/β 反映证据权重（非恒 1.0/1.0）；去重命中的已存在 observation α 累加 RepeatedOccurrence（+1.0） |
| 依赖 | P2-A（验证层）, P1-C（双维度设计） |
| 状态 | ✅ 种子 + 去重累加已实现；跨 session 留 Phase 5 |

---

## P4: 闭环（Recall + Feedback）

### P4-A: Recall 引擎 ✅

| 项 | 值 |
|---|---|
| 问题 | D8: Recall 无召回策略 |
| 实现 | 1. Intent 分类（用 `Intent` 枚举）<br/>2. 实体匹配 + embedding 语义搜索<br/>3. 展开 Concept 下 facts/hypotheses/preferences<br/>4. `MemoryContext.token_count` 生效——截断超预算内容<br/>5. 动态 token 预算：根据 Intent 调整各段落分配 |
| 文件 | `recall/mod.rs`（从 stub 变为真实实现） |
| 验收 | 输入 query 输出 ≤1500 tokens 的 MemoryContext；不同 Intent 产生不同侧重 |
| 依赖 | P3-A（embedding）, P3-B（聚类，提供候选概念） |
| 状态 | ✅ 已实现（uncommitted）— Intent 双语分类 + 实体匹配 + embedding 语义搜索 + 动态 token 预算 + recall stats 更新。7 tests in recall/mod.rs |

### P4-B: Feedback 系统

| 项 | 值 |
|---|---|
| 问题 | D9: Feedback 无数据模型 |
| 实现 | 1. 新增 `feedback` 表<br/>2. 反馈分类：Confirm/Negate/Supplement/Correct/Preference<br/>3. 反馈触发 confidence 更新<br/>4. Negate 反馈生成 rejected_hypothesis<br/>5. Correct 反馈触发 Observation 修正（连接 P2-D） |
| 文件 | `feedback/mod.rs`（从 stub 变为真实实现），新 migration，`pipeline/` 新文件 |
| 验收 | 用户说"不对" → 对应 observation confidence 下降 + rejected_hypothesis 生成 |
| 依赖 | P3-D（confidence 集成）, P4-A（recall 产出 context 供用户反馈） |
| 状态 | ✅ 已实现 — feedback 表 + 账本、关键词分类（双语）、revision 权重、Negate→rejected_hypothesis + observation β↑、Correct→单条 supersede（P2-D 机制）、Supplement→实体合并、Preference→概念定型；**闭环**：recall 只记 attempt（`record_recall`），成功/失败由显式反馈经 `record_recall_outcome` 判定（修复 P4-C 审计的 rehearsal 正反馈缺陷）。实现落在 `feedback/`+`store/feedback_store.rs`，未新增 `pipeline/` 文件（分类无需 LLM） |

### P4-C: 时间感知 + 衰减

| 项 | 值 |
|---|---|
| 问题 | P9: 无时间感知 |
| 实现 | 1. 每次 recall 更新 `last_recalled_at`（rehearsal 重置遗忘钟）<br/>2. 衰减公式（Ebbinghaus + rehearsal，见 quality-control.md §Time Decay）：`recency = exp(−λ_eff·Δt)`，`λ_eff = (1/decay_half_life_days)/(1+0.5·successful_recall_count)`，Δt 锚 `last_recalled_at`（回退 `created_at`）<br/>3. `recall_count`/`successful_recall_count` 作活跃度 + 减速因子<br/>**注**：衰减是召回侧乘性 vitality 因子，**不**碰 α/β |
| 文件 | `concept_store.rs`（更新 recall stats）, `recall/mod.rs` |
| 验收 | 长期未 recall 的 concept 在排序中降权 |
| 依赖 | P4-A |

---

## 任务依赖图

```mermaid
graph TD
    subgraph P0
        P0A["P0-A Windows"]
        P0B["P0-B 连接所有权"]
        P0C["P0-C LIKE修复"]
        P0D["P0-D 类型安全"]
        P0E["P0-E 堆分配"]
    end

    subgraph P1
        P1A["P1-A MemoryItem决策"]
        P1B["P1-B 候选状态枚举"]
        P1C["P1-C Confidence双维度"]
    end

    subgraph P2
        P2A["P2-A 提取验证"]
        P2B["P2-B 去重+归一化"]
        P2C["P2-C 共现关系"]
        P2D["P2-D 反向修正"]
    end

    subgraph P3
        P3A["P3-A Embedding"]
        P3B["P3-B 聚类引擎"]
        P3C["P3-C 合并拆分"]
        P3D["P3-D Confidence集成"]
    end

    subgraph P4
        P4A["P4-A Recall引擎"]
        P4B["P4-B Feedback"]
        P4C["P4-C 时间衰减"]
    end

    P0B --> P0C
    P0D --> P2B
    P1C --> P2A
    P2A --> P2C
    P2A --> P2D
    P2B --> P2C
    P2C --> P2D
    P2C --> P3B
    P3A --> P3B
    P0C --> P3B
    P3B --> P3C
    P2A --> P3D
    P1C --> P3D
    P3B --> P3D
    P3A --> P4A
    P3B --> P4A
    P3D --> P4B
    P4A --> P4B
    P4A --> P4C
```

---

## 关键路径

```
P0-B → P0-C → P3-B → P4-A → P4-B
         ↗         ↑
P0-D → P2-B → P2-C
                   ↓
P1-C → P2-A → P3-D
```

关键路径长度约 20 个工作日。P0 可并行完成（1 周），P1 是决策点（需讨论），P2-P4 串行依赖。

---

## 验收里程碑

| 里程碑 | 包含任务 | 验收标准 | 状态 |
|--------|---------|---------|------|
| M0: 可运行 | P0 全部 | `cargo test` 在 Windows/macOS 通过；多 store 共享连接 | ✅ |
| M1: 设计就绪 | P1 全部 | MemoryItem 决策记录；Candidate 独立状态；Confidence 双维度设计文档 | ✅ |
| M2: 管道闭环 | P2 全部 | ingest → extract → validate → dedup → normalize → store；可重新提取 | ✅ |
| M3: 概念涌现 | P3-A, P3-B, P3-D | 多 session 输入 → 自动产出 ConceptCandidate（非人工创建） | ✅ |
| M4: 概念管理 | P3-C | 候选可合并/拆分；Concept 有 confidence 变化 | ✅ |
| M5: 闭环运行 | P4 全部 | query → recall → MemoryContext → 用户反馈 → confidence 更新 → 概念修正 | ✅ |

**后续（不属本归档文件）：** P5-A 概念关系边 → P6 概念分层 → P7 因果融合 → P8 结构迁移，均已完成。状态见 [`TODO.md`](../../../TODO.md)。
