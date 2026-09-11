# Sonny 路线图

> 目标：个人知识资本的复利引擎。
>
> 你今天在项目 A 踩过的坑、在 Obsidian 里写下的洞察、在 OpenClaw 里积累的经验，
> 不应该仅仅服务于它们各自的上下文。它们应该形成一个跨项目、跨领域的复利池——
> 每一次新的经历都在旧知识的基础上产生杠杆。因果贝叶斯推断是实现这个目标的数学工具。

---

## 当前状态（截至 2026-07 · P6–P8 已实现）

**已完成：** 单项目记忆库闭环 + 概念关系边 + 跨项目分层 + 因果融合 + 结构迁移。

| 阶段 | 内容 | 状态 |
|------|------|------|
| P0–P4 | 存储/提取/聚类/召回/反馈闭环 | ✅ |
| P5-A | `concept_relation` 边 + 边级 Beta 生命周期 | ✅ |
| P6 | `lifecycle_scope`、`cross_project_count`、推广、可见性、别名合并 | ✅ |
| P7 | `causal_role`、do-充分统计量、异源证据权重、LinkEngine 接入 | ✅ |
| P8 | WL 结构指纹、跨 workspace 同构检测、迁移召回 notes | ✅ |

**当前定位：** 跨项目因果知识引擎（结构迁移为工程近似，非完整图同构推理机）。

### 关键入口

| 能力 | 代码 |
|------|------|
| 推广条件 | `models/scope.rs` · `pipeline/promote.rs` |
| 跨项目计数 | `ObservationStore::sync_cross_project_count` |
| 可见性 | `ConceptStore::list_visible_concepts` · `RecallEngine::with_elevated_visibility` |
| 因果角色 | `models/causal.rs` · LinkEngine `record_causal_evidence` |
| 结构迁移 | `pipeline/transfer.rs` · `transfer_notes_for_concepts` |

### 验收测试

- `tests/p6_layering_test.rs` — 跨项目计数 / 推广 / 可见性 / 别名合并
- `tests/p7_causal_fusion_test.rs` — Intervention→p_do、异源权重
- `tests/p8_transfer_test.rs` — 异名同构模块迁移 notes

---

## 后续可选增强（非本轮范围）

- Domain 权威来源（workspace metadata 自动推断 scope_key）
- Predictive Coding 提取（concept-growth Stage 2）
- Adversarial validation LLM
- sqlite-vec 加速
- GNN / 元学习（概念数 ≥ 500）

---

## 不变的设计原则

1. **贝叶斯因果推断是工具，知识复利是目标。**
2. **跨项目验证 > 单项目重复。**
3. **结构 > 名字。**
4. **异源证据不应该等权。**
5. **客户端是证据提供者，Sonny 是因果推断引擎。**
