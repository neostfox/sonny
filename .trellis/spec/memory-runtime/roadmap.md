# Memory Runtime — Roadmap

## 设计哲学

```
MVP 证明 pipeline 跑得通。
神经科学机制决定它跑得好不好。
它们不是可选增强——它们是产品的核心壁垒。
```

三阶段节奏：
1. **MVP（Phase 0-3）**：跑通 pipeline，验证核心假设
2. **核心增强（Phase 4-7）**：立即跟进，这是决定产品成败的关键
3. **规模化（Auto-Activate）**：数据量达标后自动启用

---

## Phase 0: 测试基础设施（Week 0）

**这是所有后续工作的前提。没有测试数据，开发是盲飞。**

| 任务 | 产出 |
|------|------|
| 0.1 | 标注 10-20 个 session（中英混合技术对话） |
| 0.2 | 每个标注 session 标记期望的 observation（subject/predicate/object/evidence） |
| 0.3 | 手动分组 3-5 组概念，作为聚类 ground truth |
| 0.4 | 创建 20-30 个 (query, expected_concept_id) 对，作为召回 benchmark |
| 0.5 | Mock LLM provider（返回预定义响应，支持确定性测试） |

---

## MVP（Phase 1-3, Week 1-8）

简化 pipeline：`Observe → Extract → Embed → Cluster → Name → Store → Recall → Feedback`

**MVP 不包含的内容**：预测编码、海马缓冲、对抗验证、扩散激活、稀疏激活、启动效应、分块层次、再巩固。这些全部在 Phase 4-7（核心增强）中实现。

**但 MVP 保留这些原则**（简化实现）：

| 原则 | MVP 简化实现 |
|------|------------|
| 不存储重复 | 实体去重检查（相同 subject+predicate+object 不新建） |
| 不立即覆盖 | 新 observation 不直接修改已有 concept，只更新 alpha/beta |
| 反馈可修正 | 用户否定 → beta 更新 + 创建 rejected_hypothesis |
| 召回有上限 | top-K（K=5）硬限制 |
| 证据必追溯 | 每 observation 必须有 evidence_text |

### Phase 1: 存储与抽取（Week 1-3）

| 任务 | 产出 | 评审关键点 |
|------|------|----------|
| 1.1 | SQLite schema + migration | 所有 JSON 列有应用层校验 |
| 1.2 | Session ingest（markdown/JSON/Trellis journal） | 支持至少 2 种格式 |
| 1.3 | **实体规范化层** | 大小写归一、后缀剥离、别名表 |
| 1.4 | LLM Observation Extractor | 结构化 JSON 输出 + source_type 标注 |
| 1.5 | Beta-Bernoulli confidence updater | ~80 行纯 Rust，零外部依赖 |
| 1.6 | CLI: `memory ingest-session`, `memory list-observations` | |

**评审修复**：实体规范化层是 CRITICAL——解决了 LLM 实体名称不一致问题。

### Phase 2: 向量与聚类（Week 4-5）

| 任务 | 产出 | 评审关键点 |
|------|------|----------|
| 2.1 | Embedding service（bge-small-zh-v1.5 via candle-transformers） | 模型缓存到 ~/.memory-runtime/models/ |
| 2.2 | **BLOB 存储优先**（sqlite-vec 作为优化后续加入） | 避免 beta 依赖（rusqlite BLOB） |
| 2.3 | HAC 聚类（linfa-clustering）+ 可配置阈值 | 聚类结果与 ground truth 对比 |
| 2.4 | 实体重叠二次合并 | |
| 2.5 | LLM 概念命名 | 回退：实体拼接命名 |
| 2.6 | Concept Candidate Builder | |
| 2.7 | CLI: `memory list-concepts`, `memory show-concept` | |

### Phase 3: 召回与集成（Week 6-8）

| 任务 | 产出 | 评审关键点 |
|------|------|----------|
| 3.1 | **预计算实体匹配**（无 LLM 调用） | 召回延迟 < 100ms |
| 3.2 | 语义召回（embedding 余弦相似度） | |
| 3.3 | 双通道合并（语义 60% + 实体 40%） | |
| 3.4 | Memory Context 生成（≤1500 tokens） | |
| 3.5 | **持久化 daemon**（编译后单二进制，非 CLI subprocess） | 解决冷启动和环境问题 |
| 3.6 | prehook：注入 Memory Context 到 Claude Code | |
| 3.7 | posthook：捕获对话用于后续抽取 | |
| 3.8 | Feedback 分类 + Beta 更新 | |
| 3.9 | CLI: `memory recall`, `memory prehook`, `memory posthook` | |

**评审修复**：recall 路径零 LLM 调用（预计算实体），延迟从 1500ms 降到 <100ms。

### MVP 验收标准

| 指标 | 目标 | 测量方式 |
|------|------|---------|
| 抽取精度 | > 70% | 标注 session 对比 |
| 聚类一致性 | 与 ground truth ≥ 80% 吻合 | 自动化对比 |
| 召回准确率 (Recall@3) | > 70% | benchmark query 测试 |
| 召回延迟 | < 100ms | 性能基准 |
| Memory Context 大小 | < 1500 tokens | 自动测量 |

### MVP 依赖

| Crate | 用途 | 大小 | 何时引入 |
|-------|------|------|---------|
| ndarray | 向量运算 + Beta 数学 | ~20MB | Phase 1 |
| candle-transformers + candle-nn | 嵌入模型推理（bge-small-zh-v1.5） | ~50MB | Phase 2 |
| bge-small-zh-v1.5 权重 | 中文嵌入 | ~90MB | Phase 2 |
| linfa-clustering | HAC 聚类 | ~10MB | Phase 2 |
| rusqlite | SQLite 存储 | ~5MB | Phase 1 |
| clap | CLI | ~2MB | Phase 1 |
| tokio | 异步运行时 | ~5MB | Phase 3 |
| serde + serde_json | 序列化 | ~3MB | Phase 1 |
| uuid | ID 生成 | ~1MB | Phase 1 |
| chrono | 时间处理 | ~1MB | Phase 1 |
| reqwest | HTTP（LLM API 调用） | ~3MB | Phase 1 |

总计约 ~190MB（含模型权重 ~90MB）。

---

## 核心增强（Phase 4-7, Week 9-16）

> **这些不是可选增强。它们是决定产品成败的关键差异化能力。**
>
> MVP 证明 pipeline 可行，但这些机制让系统从"能用"变成"好用"。
> Phase 1 完成后应立即启动，不应有间隔。

### Phase 4: 对抗验证 + 实体规范化增强（Week 9-10）

**Priority**: CRITICAL — 解决 LLM 抽取质量这一最大不确定性。

| 任务 | 产出 |
|------|------|
| 4.1 | Validator LLM prompt（5 维挑战） |
| 4.2 | 抽取 → 验证 → 接受/调整/拒绝 pipeline |
| 4.3 | 成本控制：抽样验证（surprise_score > 0.5） |
| 4.4 | Self-consistency 降级方案（3× 抽取取一致） |
| 4.5 | 实体别名主动学习确认 |
| 4.6 | CLI: `memory validate --session session_001` |

**激活阈值**: MVP 完成后立即启用。

**验收**: 验证拒绝 > 50% 的过度推测 observation。误拒率 < 10%。

### Phase 5: 预测编码 + 互补学习（Week 11-13）

**Priority**: HIGH — 这是系统从"文本检索"变成"概念引擎"的核心转折点。

预测编码让系统只关注新信息（delta），互补学习防止错误信息破坏已建立的知识。这两个机制合在一起，让系统有了"记忆的判断力"。

| 任务 | 产出 |
|------|------|
| 5.1 | 概念预测生成器（从已有 concept 生成对当前 session 的预期） |
| 5.2 | 预测误差检测 + surprise 评分 |
| 5.3 | 双路径：novel observation vs confirming evidence |
| 5.4 | Hippocampal buffer 表 + 24h 冷却期调度器 |
| 5.5 | **事务安全的巩固引擎**（`BEGIN IMMEDIATE` + crash recovery） |
| 5.6 | Surprise-based 初始置信度加权 |
| 5.7 | 巩固运行状态表 + 启动时恢复检查 |

**激活阈值**: concept_count ≥ 30（预测编码需要一定量的已有概念才有意义）。

**评审修复**：巩固引擎完整事务保护，防止崩溃导致数据丢失。

**验收**: > 80% 的已知信息被识别为"confirming"而非创建重复 observation。

### Phase 6: 主动学习（Week 14-15）

**Priority**: HIGH — 让系统主动向用户学习，而不是被动等待反馈。

| 任务 | 产出 |
|------|------|
| 6.1 | 问题生成引擎（near_confirm / conflict_resolve / alias_confirm / stale_review） |
| 6.2 | 优先级排序算法 |
| 6.3 | 时机控制器（session start / task completion / idle accumulation） |
| 6.4 | 用户响应 → Bayesian 更新 |
| 6.5 | prehook 集成：session 开始时展示问题 |
| 6.6 | CLI: `memory ask-review --workspace <ws>` |

**激活阈值**: MVP 完成后立即启用。

**验收**: 系统生成 ≤3 个优先级问题/session，用户回答率 > 60%。

### Phase 7: 高级召回机制（Week 16）

**Priority**: MEDIUM — 提升召回质量，从"找得到"到"找得准"。

| 任务 | 产出 |
|------|------|
| 7.1 | Spreading activation（深度=2，衰减=0.5） |
| 7.2 | Sparse activation（top 5% 激活，竞争性抑制） |
| 7.3 | Contextual priming（近期对话预激活） |
| 7.4 | Concept hierarchy 检测 + 层次化召回 |
| 7.5 | 再巩固（labile 状态 + 1h 超时 + 成功回忆增强） |
| 7.6 | Hysteresis vitality threshold（降级 0.40 / 恢复 0.50） |
| 7.7 | Incremental embedding update（drift > 0.15 时重新嵌入） |
| 7.8 | LLM 意图分类（替代 MVP 硬编码关键词，合并到 Memory Context 生成，零额外成本） |

**激活阈值**: concept_count ≥ 50（概念多到需要高级过滤机制）。

**验收**: 召回相关性提升 > 15%（与 Phase 3 基线对比）。

---

## 规模化特征（Auto-Activated）

数据量达标后自动启用，不需要人工决策。

### Scale Feature 1: GNN Spreading Activation

**Auto-activation trigger**: concept_count ≥ 500 AND graph_density ≥ 0.05

| Phase | Concept Count | Mode | Description |
|-------|--------------|------|-------------|
| GNN-0 | < 500 | Disabled | Hand-crafted spreading activation rules |
| GNN-1 | 500 - 1000 | Advisory | GraphSAGE 后台训练，与手工规则对比 |
| GNN-2 | 1000 - 5000 | Blended | GNN 和规则 50/50 加权 |
| GNN-3 | > 5000 | Primary | GNN 主导，规则作为 fallback |

**Dependencies**: PyTorch Geometric (PyG)，仅在激活时引入。

**Rollback**: GNN 与规则一致率 < 70% 时自动回退。

### Scale Feature 2: Meta-Learning

**Auto-activation trigger**: accumulated_feedback ≥ 100 confirmed/rejected observations with reasons.

| Milestone | Description |
|-----------|------------|
| M-1 | 反馈数据集：(observation, verdict, reason) 三元组 |
| M-2 | 错误模式分析：哪些抽取错误最常见？ |
| M-3 | 动态 evidence weights：根据错误频率调整 Beta 权重 |
| M-4 | 抽取 prompt 自动调优：从已确认 observation 生成 few-shot 示例 |
| M-5 | 置信度校准：学习 LLM confidence → 实际精度的映射 |

---

## 评审发现的关键修复

以下问题从评审中发现，必须在实现中解决：

### CRITICAL 修复（必须在 MVP 中解决）

| # | 问题 | 修复 | 在哪个 Phase |
|---|------|------|------------|
| C1 | 实体名称不一致 | 实体规范化层（大小写 + 后缀 + 别名） | Phase 1 |
| C2 | 召回路径 LLM 延迟 | 预计算实体匹配，recall 零 LLM 调用 | Phase 3 |
| C3 | 向量搜索无索引 | BLOB 优先 + workspace 过滤 + centroid 缓存 | Phase 2 |

### HIGH 修复（在核心增强中解决）

| # | 问题 | 修复 | 在哪个 Phase |
|---|------|------|------------|
| H1 | 中文语义等价阈值过高 | 降至 0.78-0.80，精确匹配替代子串匹配 | Phase 5 |
| H2 | 生命力阈值振荡 | Hysteresis（降 0.40 / 恢复 0.50）+ 最小停留时间 7 天 | Phase 7 |
| H3 | 单次否定压垮弱概念 | 降 negation 权重到 2.5，或要求 2 次独立否定才降级 | Phase 5 |
| H4 | 巩固引擎无事务保护 | `BEGIN IMMEDIATE` + crash recovery + consolidation_run 状态表 | Phase 5 |
| H5 | 隐式正反馈无限累积 | Diminishing returns: `alpha += 0.3 * min(1, 5/(recall_count+1))` | Phase 5 |
| H6 | HAC O(n²) 扩展性 | 增量聚类为主，全量重建限制最近 N=5000 + 随机采样 | Phase 2 |
| H7 | 对抗验证领域偏置 | 按 workspace 追踪验证结果，高误拒率 workspace 降低激进程度 | Phase 4 |

---

## 评估框架

| 指标 | MVP 目标 | 增强后目标 | 测量方式 |
|------|---------|----------|---------|
| 抽取精度 | > 70% | > 85%（对抗验证后） | 标注 session 对比 |
| 抽取召回率 | > 60% | > 70% | ground truth 对比 |
| 聚类一致性 | ≥ 80% | > 0.7 silhouette | 自动化 + ground truth |
| 召回准确率 (Recall@3) | > 70% | > 85%（priming + spreading） | benchmark query |
| 召回延迟 | < 100ms | < 100ms（保持） | 性能基准 |
| Memory Context 大小 | < 1500 tokens | < 1500 tokens | 自动测量 |
| 主动学习回答率 | — | > 60% | 跟踪回答率 |
| GNN 与规则一致率 | — | > 70% | 自动对比 |
