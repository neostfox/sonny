# Sonny 路线图

> 目标：个人知识资本的复利引擎。
>
> 你今天在项目 A 踩过的坑、在 Obsidian 里写下的洞察、在 OpenClaw 里积累的经验，
> 不应该仅仅服务于它们各自的上下文。它们应该形成一个跨项目、跨领域的复利池——
> 每一次新的经历都在旧知识的基础上产生杠杆。因果贝叶斯推断是实现这个目标的数学工具。

---

## 当前状态

| 阶段 | 内容 | 状态 |
|------|------|------|
| P0–P4 | 存储/提取/聚类/召回/反馈闭环 | ✅ |
| P5-A | `concept_relation` 边 + 边级 Beta 生命周期 | ✅ |
| P6 | 概念分层 / 跨项目计数 / 推广 / 可见性 / 别名合并 | ✅ |
| P7 | 因果角色 / do-充分统计量 / 异源证据权重 | ✅ |
| P8 | WL 结构指纹 / 跨 workspace 迁移 notes | ✅ |
| P10 | Dreaming 实体邻域离线巩固 | 本阶段 |
| P11 | 隐式反馈 + 持久性三档 | 本阶段 |
| P12 | Add 五操作 + Recall-aware 提取 | 本阶段 |

**定位：** 跨项目因果知识引擎。吸收 MindMemOS 的质量闭环（Dreaming / 隐式反馈 / Action Plan），不吸收其重型基础设施。

### 关键入口

| 能力 | 代码 |
|------|------|
| 推广条件 | `models/scope.rs` · `pipeline/promote.rs` |
| 跨项目计数 | `ObservationStore::sync_cross_project_count` |
| 可见性 | `list_visible_concepts` · `RecallEngine::with_elevated_visibility` |
| 因果角色 | `models/causal.rs` · LinkEngine |
| 结构迁移 | `pipeline/transfer.rs` |
| Dreaming | `pipeline/dream.rs` |
| 持久性分类 | `models/persistence.rs` |
| Add Action Plan | `pipeline/action_plan.rs` |

### 验收测试

- `p6_layering_test` / `p7_causal_fusion_test` / `p8_transfer_test`
- `p10_dreaming_test` / `p11_persistence_test` / `p12_action_plan_test`

---

## P10：Dreaming（实体邻域离线巩固）

**来源：** MindMemOS Dreaming — 不全局比对，按实体邻域 detect → act；标记 consolidated。

| ID | 任务 |
|----|------|
| P10-A | `pipeline/dream.rs`：按实体邻域聚类未巩固 Observation |
| P10-B | Detect：重复三元组、对谓词冲突（has/not_has）、低价值（assistant_guess） |
| P10-C | Act：合并重复（α 累加）、归档低价值、冲突降 β、标记 consolidated |

**验收：** 重复观测合并为一条高 α；assistant_guess 低证据可归档；已 consolidated 不再处理。

---

## P11：隐式反馈 + 持久性三档

**来源：** MindMemOS Feedback — 纠正往往对 Agent 说，不是对记忆系统说。

| ID | 任务 |
|----|------|
| P11-A | `PersistenceScope`: TaskTemporary / ScenarioSpecific / LongTerm |
| P11-B | 关键词分类：「这次先这样」→ temp；「以后都」→ long；默认 scenario |
| P11-C | 纠正信号检测（不对/错了/应该是）+ 仅 LongTerm/Scenario 才写入 durable |

**验收：** TaskTemporary 纠正不产生新 Observation；LongTerm 进入 ledger。

---

## P12：Add 五操作 + Recall-aware 提取

**来源：** MindMemOS MindVanilla — 提取时看旧记忆；写入前显式动作规划。

| ID | 任务 |
|----|------|
| P12-A | `MemoryAction`: Add / Reinforce / Update / Merge / Skip |
| P12-B | `plan_memory_action(existing, candidate)` 确定性规则 |
| P12-C | `build_recall_envelope(raw, related_memories)` 注入提取上下文 |

**验收：** 同三元组 → Reinforce（非重复新行）；对象变更 → Update（supersede）；无匹配 → Add。

---

## P13–P14（后续）

| 阶段 | 内容 | 状态 |
|------|------|------|
| P13 | Compact Search：多步 agentic + RRF + 双向边遍历 | 待做 |
| P14 | Entity-Property 时间线（版本史 + 过期） | 待做 |

### 可选增强

- Domain 权威来源、Predictive Coding、Adversarial LLM、sqlite-vec、Skill 轨迹演化

---

## 不变的设计原则

1. **贝叶斯因果推断是工具，知识复利是目标。**
2. **跨项目验证 > 单项目重复。**
3. **结构 > 名字。**
4. **异源证据不应该等权。**
5. **客户端是证据提供者，Sonny 是因果推断引擎。** — Dreaming/Action Plan 在引擎内；Skill 演化留给客户端。
