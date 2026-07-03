# Design Reference

设计文档位于 `docs/memory-runtime-design.md`。本文件将设计文档每个章节映射到实际代码和实现状态。

## §1-2 核心定位与原则

| 原则 | 实现状态 | 说明 |
|------|---------|------|
| 记忆是概念的原料，概念是记忆的压缩索引 | ⚠️ 部分实现 | Observation → ConceptCandidate 数据模型存在，但中间无生长机制 |
| 每条记忆必须有证据来源 | ✅ | `evidence_text` 字段在 Observation 上，extraction prompt 要求 |
| 助手推测默认不是事实 | ✅ | `ObservationSourceType::AssistantGuess` 区分来源 |
| 否定信息是高价值记忆 | ✅ | `ObservationSourceType::UserNegation` + `FeedbackType::Negate` |
| Memory Context 必须短、准、可操作 | ✅ | `MemoryContext.to_prompt()` + `token_count` enforced by `build_context()` + `enforce_total_budget()` |

## §3 系统流程

### §3.1 总体流程

```
设计要求:          实现状态:
Memory Ingest      ✅ pipeline/ingest.rs
Observation Extract ✅ pipeline/extract.rs
Session Distiller   ❌ 未实现
Concept Growth      ❌ 未实现
Memory Store        ✅ store/
Memory Retriever    ✅ recall/mod.rs (P4-A: Intent + Entity + Semantic)
Memory Context      ✅ models/recall.rs + recall/mod.rs (token_count enforced)
User Feedback       ❌ feedback/ stub
Memory Revision     ❌ 未实现
```

### §3.2 实时链路

```
设计要求:                    实现状态:
识别 workspace/repo/task      ❌ CLI 用 --workspace flag
召回相关概念                  ✅ recall/mod.rs: rank() combines semantic + entity
展开概念下的事实、约束、偏好    ✅ recall/mod.rs: build_context() populates sections
生成 Memory Context           ✅ recall/mod.rs: token_count enforced
注入 Coding Agent             ❌ 无 prehook 实现
```

### §3.3 离线链路

```
设计要求:                  实现状态:
Historical Session          ✅ detect_and_parse()
Topic Segmentation          ❌ 未实现
Observation Extraction      ✅ extract_observations()
Memory Type Classification  ❌ 未实现
Concept Candidate Building  ❌ 未实现
Deduplication               ✅ extract_and_dedup() → check_duplicate()（p2b）+ canonical_key_light 归一
Conflict Detection          ❌ 未实现
Concept Promotion / Review  ❌ 未实现
```

### §3.4 修正链路

```
设计要求:                    实现状态:
判断反馈类型                  ❌ 未实现
更新 confidence              ⚠️ BetaConfidence.update() 存在但无调用者
生成 rejected_hypothesis      ❌ 未实现
更新未来召回结果              ❌ 未实现
```

## §4 核心对象模型

### §4.1 RawMemory

| 设计要求 | 代码位置 | 状态 |
|---------|---------|------|
| 保存原始经历 | `models/raw_memory.rs` | ✅ |
| 来源：用户消息、助手回答、历史session、Trellis journal | `SourceType` enum | ✅ |
| 不直接作为长期记忆使用 | 设计意图正确 | ✅ |

### §4.2 Observation

| 设计要求 | 代码位置 | 状态 | Gap |
|---------|---------|------|-----|
| subject-predicate-object 三元组 | `models/observation.rs` | ✅ | |
| 表达 A包含B、A不包含B、A依赖B 等 | `models/predicate.rs` `Predicate` 枚举 + `normalize_predicate` | ✅ | p2e: 7 canonical 谓词 + 同义词归一；未知回退不 drop |
| 区分用户确认 vs 助手推测 | `ObservationSourceType` | ✅ | |
| 表达排障路径、任务状态、被否定假设 | `memory_type_candidate` + `Predicate` + `UserNegation` | ✅ | p2e 谓词归一（`causes` 表根因）+ p1d memory_type 覆盖任务状态/偏好/架构 |
| memory_type_candidate | `observation.memory_type_candidate` | ✅ | p1d 落地 |
| surprise_score | `observation.surprise_score` | ⚠️ | 字段存在，永远默认 0.5 |

**决策 (2026-06-13, p1c，p1d 已落地代码)**: confidence 拆为 `extraction_confidence`（提取时由 source_type 固定，不可变）与 `fact_confidence`（Beta 后验）。recall 排序用 `effective_confidence = extraction_confidence × fact_confidence`。代码：`ObservationSourceType::extraction_confidence()`、`Observation::effective_confidence()`。

### §4.3 MemoryItem

| 设计要求 | 代码位置 | 状态 |
|---------|---------|------|
| 9 种记忆类型 | `observation.rs` `MemoryType`（Observation 属性） | ✅ p1d 迁移 |
| 独立 MemoryItem 实体 / Store / Pipeline stage | 已删除（`memory_item.rs` 删除，migration 003 DROP） | ✅ p1d |

**决策 (2026-06-13, p1a，p1d 已落地代码)**: 删除 MemoryItem 独立实体与 pipeline 中转层。9 种 MemoryType 折叠为 Observation 属性（`memory_type_candidate` + `observation_detail_json`），概念生长直接基于 Observation 聚类。依据：P3-B 聚类引擎以 Observation 为输入（非 MemoryItem）；MemoryItem 为死代码（无 store/pipeline 调用）；增加中转层倍增去重/置信传播/drift 表面而无收益。

### §4.4 ConceptCandidate

| 设计要求 | 代码位置 | 状态 | Gap |
|---------|---------|------|-----|
| name, summary, source_terms | `models/concept.rs` | ✅ | |
| known_facts, rejected_hypotheses, open_questions | JSON 字段 | ✅ | |
| evidence, confidence, status | 字段存在 | ✅ | |
| 从一组 Observation 中生长出来 | 无增量机制 | ❌ | **D1**: 一次性 insert，无法生长 |
| 独立的生命周期状态 | 复用 `ObservationStatus` | ❌ | **D4**: 建模错误 |
| Store 实现 | `store/concept_store.rs` | ✅ | |

### §4.5 Concept

| 设计要求 | 代码位置 | 状态 |
|---------|---------|------|
| name, type, definition, related_entities | `models/concept.rs` | ✅ |
| known_facts, constraints, rejected_hypotheses | JSON 字段 | ✅ |
| confidence, status | `ConceptStatus` enum | ✅ |
| parent_concept_id, hierarchy_depth | 支持层级 | ✅ |
| recall_count, successful/failed | 统计字段存在 | ⚠️ 无代码更新这些字段 |

### §4.6 MemoryContext

| 设计要求 | 代码位置 | 状态 | Gap |
|---------|---------|------|-----|
| workspace, intent, entities | `models/recall.rs` | ✅ | |
| known_facts, rejected_hypotheses, preferences | 字段存在 | ✅ | |
| to_prompt() 格式化 | 方法存在 | ✅ | |
| ≤1500 tokens 限制 | `token_count` 字段 | ❌ | 字段存在但不生效，不截断 |
| token 预算分配 | 不存在 | ❌ | 设计 §13.4 定义了分配方案 |

## §5 概念生长流程

| Stage | 设计要求 | 代码位置 | 状态 |
|-------|---------|---------|------|
| Observe | 从历史 session 提取原始观察 | `pipeline/ingest.rs` | ✅ |
| Extract | 从 RawMemory 提取 Observation | `pipeline/extract.rs` | ✅ |
| Embed | 生成 1024 维语义向量并持久化 | `embed/`, `store/embedding_store.rs`, migration 006 | ✅ P3-A |
| Cluster | 按实体重叠、任务连续性等聚类 | 不存在 | ❌ **D1** |
| Name | 为候选主题生成概念名称 | 不存在 | ❌ |
| Link | 连接实体、事实、假设、偏好 | 不存在 | ❌ |
| Validate | 证据升权/降权 | `confidence/mod.rs` + `extract_and_dedup()` | ✅ P3-D（跨 session 留 consolidation） |
| Promote | Candidate → Concept 升级 | 不存在 | ❌ |
| Use | 基于概念召回上下文 | `recall/mod.rs` | ✅ P4-A（Intent + Entity + Semantic + Token Budget） |
| Revise | 反馈修正概念边界 | `feedback/` stub | ❌ **D9** |

## §6 记忆类型定义

9 种 MemoryType 在 `models/memory_item.rs` 定义。无任何代码使用。

## §7 Trellis 组织方式

memory-runtime 详细设计已迁移至 [../memory-runtime/](../memory-runtime/index.md)（concept-growth / quality-control / recall-design / memory-model）。本目录维护通用 backend 规范 + roadmap + design-status。

## §8 Claude Code 实现约束

| 约束 | 遵守情况 |
|------|---------|
| 不允许把历史 session 直接全量注入模型 | ✅ extract 只产出结构化 Observation |
| 不允许没有 evidence 就写入长期记忆 | ✅ extraction prompt 强制要求 evidence_text |
| 不允许把助手推测直接设为 confirmed | ✅ AssistantGuess 只能进 Candidate |
| 不允许丢弃用户否定信息 | ✅ UserNegation source_type 存在 |
| 不允许绕过 Concept 直接做纯文本召回 | ✅ recall/mod.rs 通过 ConceptStore 检索 |
| 不允许生成过长 Memory Context | ✅ enforce_total_budget() + truncate_section() |

## §9 MVP 范围

| MVP 要求 | 状态 |
|---------|------|
| 能导入历史 session | ✅ |
| 能提取 Observation | ✅ |
| 能生成候选概念 | ❌ |
| 能展示概念详情 | ❌ |
| 能保留被否定假设 | ⚠️ 模型支持，无 pipeline 产出 |
| 能基于用户问题召回 Memory Context | ✅ recall/mod.rs: Intent + Entity + Semantic + Token Budget |
| 能区分用户确认和助手推测 | ✅ |
| 能根据用户反馈调整记忆状态 | ❌ |

## §10-14 数据模型 / CLI / LLM规范 / Recall / 验收

数据模型（§10）在 `migrations/001_initial.sql` 中实现，基本对齐。

CLI（§11）只实现了 3 个命令（init, ingest-session, list-observations），设计要求 8 个。

LLM 抽取规范（§12）: Observation Extractor ✅, Session Distiller ❌, Concept Candidate Builder ❌。

Recall 设计（§13）: ✅ P4-A 实现：Intent 双语分类 + 实体匹配 + embedding 语义搜索 + 动态 token 预算 + recall stats 更新。

验收标准（§14）: 8 条基础验收中满足 5 条（+1 Recall 召回上下文）。
