# Memory Runtime 技术设计文档

> 目标：指导实现一个面向 Coding Agent 的长期记忆系统。该系统的核心不是保存历史文本，而是从历史经历中生长出概念网络，并在未来任务中精准召回。

---

## 1. 核心定位

Memory Runtime 的核心理念：

> 记忆是概念的原料，概念是记忆的压缩索引。

系统要完成的不是"把历史对话存起来"，而是让历史经历逐渐沉淀为：

```text
Observation → Concept Candidate → Concept → Memory Context
```

其中：

```text
session 是原料
observation 是片段
concept 是压缩
relation 是理解
recall 是激活
feedback 是修正
```

### 1.1 系统目标

Memory Runtime 要让 Claude Code / Codex / OpenCode 等 Coding Agent 具备以下能力：

1. 记住项目架构。
2. 记住历史 Bug 修复路径。
3. 记住用户偏好和项目约束。
4. 记住长期任务状态。
5. 从历史 session 中自动提炼概念。
6. 在未来任务中基于概念召回上下文。
7. 根据用户反馈修正记忆和概念边界。

### 1.2 非目标

第一版不做：

1. 不做复杂知识图谱平台。
2. 不做 GNN。
3. 不做全格式 Parser 系统。
4. 不做全量历史文本注入。
5. 不把助手推测直接当成事实。
6. 不做通用个人知识库。
7. 不做复杂企业权限系统。

---

## 2. 核心原则

### 2.1 只体现流程，不体现讨论过程

技术文档只描述最终系统流程，不记录设计讨论、观点演化、取舍过程。

文档中不应出现：

```text
我们之前讨论过……
我们认为……
为什么从 Parser 转向 Observation……
```

文档应只描述：

```text
系统如何输入
系统如何提取
系统如何生成概念
系统如何召回
系统如何修正
任务如何组织
Claude Code 如何实现
```

### 2.2 概念生长优先

系统重点不是 Parser、NER、RAG 或 Graph，而是概念生长流程。

核心流程：

```text
历史经历
  ↓
Observation
  ↓
概念候选
  ↓
概念确认
  ↓
概念召回
  ↓
反馈修正
```

### 2.3 用户输入按文本经历处理

用户输入作为原始经历处理，重点提取：

```text
用户目标
涉及对象
已知事实
否定信息
用户纠正
用户偏好
任务状态
```

不要求用户输入是完整代码、完整 SQL、完整日志或完整配置。

### 2.4 记忆必须可追溯

每条记忆必须有证据来源。

证据至少包括：

```text
source_type
source_id
session_id
message_range
evidence_text
created_at
confidence
status
```

没有证据的记忆不能进入 confirmed 状态。

### 2.5 助手推测默认不是事实

助手回答中的推测只能进入 candidate 状态。只有以下情况可以升级：

```text
用户明确确认
文件/项目证据支持
多次重复出现且无冲突
人工 review 确认
```

### 2.6 否定信息是高价值记忆

被用户否定、质疑、纠正的内容必须保存。

例如：

```text
某个关联字段可能不对
某个根因不是实际原因
某个方案已经失败
某个表不包含某字段
```

这类信息可以防止 Coding Agent 重复犯错。

---

## 3. 最终系统流程

### 3.1 总体流程

```text
历史 Session / 当前对话 / Journal / Task 记录
        ↓
Memory Ingest
        ↓
Observation Extractor
        ↓
Session Distiller
        ↓
Concept Growth Engine
        ↓
Memory Store
        ↓
Memory Retriever
        ↓
Memory Context
        ↓
Claude Code / Coding Agent
        ↓
User Feedback
        ↓
Memory Revision
```

### 3.2 实时链路

用户当前提问时：

```text
User Prompt
  ↓
识别当前 workspace / repo / task
  ↓
召回相关概念
  ↓
展开概念下的事实、约束、偏好、被否定方案
  ↓
生成 Memory Context
  ↓
注入 Claude Code
```

### 3.3 离线链路

空闲时处理历史 session：

```text
Historical Session
  ↓
Topic Segmentation
  ↓
Observation Extraction
  ↓
Memory Type Classification
  ↓
Concept Candidate Building
  ↓
Deduplication
  ↓
Conflict Detection
  ↓
Concept Promotion / Review
```

### 3.4 修正链路

用户反馈后：

```text
User Feedback
  ↓
判断是确认、否定、补充、修正还是偏好
  ↓
更新相关 observation / memory / concept
  ↓
调整 confidence
  ↓
必要时生成 rejected_hypothesis
  ↓
更新未来召回结果
```

---

## 4. 核心对象模型

### 4.1 Raw Memory

原始经历。

来源包括：

```text
用户消息
助手回答
历史 session
journal 记录
task 记录
项目说明
Bug 记录
任务总结
```

Raw Memory 只负责保存证据，不直接作为长期记忆使用。

### 4.2 Observation

Observation 是从 Raw Memory 中提取出的事实片段。

示例：

```json
{
  "subject": "POSMASK",
  "predicate": "not_has_field",
  "object": "MACHINE",
  "evidence_text": "POSMASK 没有机器字段",
  "extraction_confidence": 0.70,
  "fact_confidence": 0.92,
  "status": "candidate"
}
```

Observation 的 confidence 分两个独立维度：

- `extraction_confidence`：提取时由 `source_type` 决定的来源可信度，提取后固定不变（FileEvidence 0.9、UserConfirm 0.85、UserNegation 0.8、UserMessage 0.7、AssistantGuess 0.3）。
- `fact_confidence`：由证据累积的 Beta 分布后验（`evidence_alpha / (evidence_alpha + evidence_beta)`），随反馈和时间动态更新。

召回排序使用 `effective_confidence = extraction_confidence × fact_confidence`：来源不可信的推测即使被反复"未纠正"也无法累积到高置信，来源可信但被证据反驳的事实也会被压低。

Observation 可以表达：

```text
A 包含 B
A 不包含 B
A 依赖 B
A 可能关联 B
A 被用户否定
A 是用户偏好
A 是任务状态
A 是 Bug 根因
A 是架构事实
```

### 4.3 Memory Type（Observation 属性）

记忆类型不作为独立实体持久化，也没有独立的 pipeline 中转层。每条 Observation 在提取时通过 `memory_type_candidate` 字段标注所属类型，类型特定的结构化细节存入 `observation_detail_json`。

概念生长直接基于 Observation 聚类（Observe → Extract → Cluster），不经过独立的 Memory Item 阶段。

类型包括：

```text
architecture_memory
bug_fix_memory
troubleshooting_memory
data_asset_memory
user_preference_memory
task_state_memory
rejected_hypothesis_memory
decision_memory
project_context_memory
```

### 4.4 Concept Candidate

Concept Candidate 是从一组 Observation 中生长出的候选概念。

示例：

```json
{
  "name": "MASK 与机器关系排查",
  "source_terms": [
    "MASK",
    "POSMASK",
    "MASKSPECNAME",
    "MACHINE",
    "POSMACHINE"
  ],
  "summary": "围绕 MASK 与机器关系、POSMASK 字段和候选关联路径的长期排查主题。",
  "known_facts": [
    "POSMASK 不包含机器字段"
  ],
  "rejected_hypotheses": [
    "某些字段作为直接关联路径可能不准确"
  ],
  "open_questions": [
    "正确中间表或关联路径是什么"
  ],
  "confidence": 0.78,
  "status": "candidate"
}
```

### 4.5 Concept

Concept 是经过确认、合并、升权后的稳定概念。

Concept 应包含：

```text
name
type
definition
workspace
related_entities
known_facts
constraints
rejected_hypotheses
open_questions
evidence
confidence
status
```

### 4.6 Memory Context

Memory Context 是最终注入 Claude Code 的上下文。

它不是历史摘要，而是围绕当前任务激活的概念内容。

格式：

```text
[Memory Context]
Workspace: <workspace>
Current Concept: <concept>

User Preferences:
- ...

Known Facts:
- ...

Rejected / Doubtful Hypotheses:
- ...

Current Task State:
- ...

Relevant Entities:
- ...

Instructions:
- Do not treat candidate hypotheses as confirmed facts.
- Respect rejected hypotheses and user preferences.
```

---

## 5. 概念生长流程

### 5.1 Observe

从历史 session、当前对话、journal、task 文件中提取原始观察。

输入：

```text
用户问题
用户补充
用户否定
助手回答
任务总结
Bug 修复记录
架构讨论
```

输出：Observation。

### 5.2 Extract

从 Observation 中提取：

```text
实体
事实
约束
偏好
任务状态
被否定假设
开放问题
解决方案
```

### 5.3 Cluster

将相关 Observation 聚合成候选主题。

聚合依据：

```text
实体重叠
任务连续性
用户表达相似
问题类型相似
workspace 相同
repo 相同
时间邻近
```

### 5.4 Name

为候选主题生成概念名称。

命名要求：

```text
贴近用户真实表达
能表达长期主题
不要过度抽象
不要只用单个技术词
```

好例子：

```text
MASK 与机器关系排查
BOE 内网 DNS 排障
Maven HTTP Nexus 阻断
Spring Boot Mapper 启动失败
FP 系统数据治理与排产逻辑
```

坏例子：

```text
数据库
网络
错误
项目
代码
```

### 5.5 Link

把概念与以下对象连接：

```text
相关实体
已知事实
被否定假设
开放问题
用户偏好
任务状态
证据片段
```

### 5.6 Validate

根据证据提升或降低 `fact_confidence`。`extraction_confidence` 在提取时由 `source_type` 固定，Validate 阶段不修改。

升权条件（增加 `evidence_alpha`）：

```text
用户确认
多次重复出现
任务持续进行
文件证据支持
人工 review 确认
```

降权条件（增加 `evidence_beta`）：

```text
用户否定
新证据冲突
长期未使用
被标记过期
只来自助手推测
```

Concept 与 ConceptCandidate 没有 extraction 维度（它们不是被提取的，而是从 Observation 生长出来的），只使用 `fact_confidence`。

### 5.7 Promote

Concept Candidate 满足条件后升级为 Concept。

升级条件：

```text
有多个 observation 支持
有明确证据
有稳定实体集合
有可复用价值
无严重冲突
```

### 5.8 Use

用户再次提问时，优先命中 Concept。

```text
当前问题
  ↓
概念匹配
  ↓
展开已知事实 / 被否定方案 / 任务状态 / 用户偏好
  ↓
生成 Memory Context
```

### 5.9 Revise

用户反馈会修改概念边界。

例如：

```text
用户说"不是这个原因" → 降低对应 bug_fix 置信度
用户说"对，就是这个" → 升级为 confirmed
用户说"这个字段不对" → 生成 rejected_hypothesis
用户补充新字段 → 更新 concept 相关实体
```

---

## 6. 记忆类型定义

下列各类型的 JSON 形状描述 Observation 在该 `memory_type_candidate` 下的 `observation_detail_json` 结构，不是独立持久化实体。

### 6.1 Architecture Memory

用于保存项目结构和模块关系。

```json
{
  "type": "architecture_memory",
  "title": "服务依赖关系",
  "content": "某 Listener 依赖某 Service，Service 依赖某 Mapper。",
  "entities": [],
  "relations": [],
  "evidence": [],
  "status": "candidate"
}
```

### 6.2 Bug Fix Memory

用于保存 Bug、现象、根因、修复方案。

```json
{
  "type": "bug_fix_memory",
  "error_signature": "...",
  "symptoms": [],
  "root_cause": "...",
  "solution": "...",
  "rejected_causes": [],
  "evidence": [],
  "status": "candidate"
}
```

### 6.3 Troubleshooting Memory

用于保存排障路径。

```json
{
  "type": "troubleshooting_memory",
  "problem": "...",
  "symptoms": [],
  "diagnosis_path": [],
  "result": "...",
  "evidence": [],
  "status": "candidate"
}
```

### 6.4 Data Asset Memory

用于保存表、字段、指标、数据源、字段含义。

```json
{
  "type": "data_asset_memory",
  "entity": "...",
  "fields": [],
  "known_facts": [],
  "negative_facts": [],
  "evidence": [],
  "status": "candidate"
}
```

### 6.5 User Preference Memory

用于保存用户长期偏好。

```json
{
  "type": "user_preference_memory",
  "scope": "...",
  "content": "...",
  "evidence": [],
  "confidence": 0.0,
  "status": "candidate"
}
```

### 6.6 Rejected Hypothesis Memory

用于保存失败方案和被否定路径。

```json
{
  "type": "rejected_hypothesis_memory",
  "task": "...",
  "hypothesis": "...",
  "reason": "...",
  "evidence": [],
  "confidence": 0.0,
  "status": "active"
}
```

### 6.7 Task State Memory

用于保存长期任务进展。

```json
{
  "type": "task_state_memory",
  "task": "...",
  "status": "in_progress",
  "known": [],
  "open_questions": [],
  "next_actions": [],
  "evidence": []
}
```

---

## 7. Spec 组织方式

项目规范与任务记录由 `.ctl/` 管理。Memory Runtime 的领域设计 spec 在 `.ctl/spec/memory-runtime/`，通用 backend 工程规范在 `.ctl/spec/backend/`，任务记录在 `.ctl/tasks/`。

### 7.1 目录结构

```text
.ctl/
  spec/
    memory-runtime/
      index.md
      principles.md
      concept-growth.md
      memory-model.md
      recall-design.md
      quality-control.md
    backend/
      roadmap.md
      design-reference.md
      directory-structure.md
      ...

  tasks/
    <task-id>/
      task.json
      events.jsonl
```

### 7.2 principles.md

记录不可违反的原则：

```text
1. Memory Runtime 的核心是概念生长，不是文本存储。
2. 用户输入作为经历处理。
3. 每条记忆必须有证据。
4. 助手推测不能直接成为 confirmed memory。
5. 被否定假设必须保存。
6. 召回优先概念，而不是原始历史文本。
7. Memory Context 必须短、准、可操作。
```

### 7.3 concept-growth.md

记录概念生长流程：

```text
Observe → Extract → Cluster → Name → Link → Validate → Promote → Use → Revise
```

### 7.4 memory-model.md

记录对象模型：

```text
RawMemory
Observation
ConceptCandidate
Concept
MemoryContext
Evidence
```

### 7.5 recall-design.md

记录召回流程：

```text
当前问题 → workspace/repo/task 识别 → Concept Match → Memory Context Pack → Claude Code
```

### 7.6 quality-control.md

记录质量控制：

```text
candidate / confirmed / rejected / deprecated / conflicted
confidence
evidence
user feedback
review
```

---

## 8. Claude Code 实现约束

Claude Code 负责实现 Memory Runtime，但必须遵循项目 spec（`.ctl/spec/`）。

### 8.1 实现前必须读取

```text
.ctl/spec/memory-runtime/principles.md
.ctl/spec/memory-runtime/concept-growth.md
.ctl/spec/memory-runtime/memory-model.md
.ctl/spec/memory-runtime/recall-design.md
.ctl/spec/memory-runtime/quality-control.md
.ctl/spec/backend/roadmap.md
```

### 8.2 不允许做的事

```text
1. 不允许把历史 session 直接全量注入模型。
2. 不允许没有 evidence 就写入长期记忆。
3. 不允许把助手推测直接设为 confirmed。
4. 不允许丢弃用户否定信息。
5. 不允许绕过 Concept 直接做纯文本召回作为核心逻辑。
6. 不允许生成过长 Memory Context。
```

### 8.3 优先实现模块

```text
1. 本地 Memory Store
2. Session Ingest
3. Observation Extractor
4. Session Distiller
5. Concept Candidate Builder
6. Concept Review
7. Memory Retriever
8. Claude Code Pre-hook
9. Claude Code Post-hook
```

---

## 9. MVP 范围

### 9.1 MVP 输入

```text
历史 session markdown/json
journal 记录
task/prd 记录
用户手动输入片段
```

### 9.2 MVP 输出

```text
候选概念列表
概念详情
相关事实
被否定假设
开放问题
用户偏好
任务状态
Memory Context
```

### 9.3 MVP 不做

```text
复杂代码索引
复杂文件解析
数据库直连
团队权限
图数据库
MCP 工具完整实现
```

---

## 10. 数据模型

MVP 使用 SQLite 即可。

### 10.1 raw_memory

```sql
CREATE TABLE raw_memory (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id       TEXT UNIQUE NOT NULL,
    workspace_id    TEXT,
    session_id      TEXT,
    role            TEXT,
    content         TEXT NOT NULL,
    source_type     TEXT,
    source_ref      TEXT,
    created_at      TEXT,
    metadata_json   TEXT
);
```

### 10.2 observation

```sql
CREATE TABLE observation (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    observation_id  TEXT UNIQUE NOT NULL,
    workspace_id    TEXT,
    memory_id       TEXT,
    subject_text    TEXT NOT NULL,
    subject_type    TEXT,
    predicate       TEXT NOT NULL,
    object_text     TEXT,
    object_type     TEXT,
    evidence_text   TEXT,
    memory_type_candidate   TEXT,
    observation_detail_json TEXT,
    extraction_confidence  REAL NOT NULL DEFAULT 0.5,
    evidence_alpha         REAL NOT NULL DEFAULT 1.0,
    evidence_beta          REAL NOT NULL DEFAULT 1.0,
    status          TEXT DEFAULT 'candidate',
    created_at      TEXT,
    metadata_json   TEXT
);
```

### 10.3 memory_item（已移除）

独立的 `memory_item` 表移除。记忆类型作为 Observation 的属性持久化：`observation.memory_type_candidate` 标注类型，`observation.observation_detail_json` 存储类型特定结构。参见 §4.3 与 §10.2。

### 10.4 concept_candidate

```sql
CREATE TABLE concept_candidate (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    candidate_id             TEXT UNIQUE NOT NULL,
    workspace_id             TEXT,
    name                     TEXT NOT NULL,
    summary                  TEXT,
    source_terms_json        TEXT,
    source_sessions_json     TEXT,
    source_observations_json TEXT,
    known_facts_json         TEXT,
    rejected_hypotheses_json TEXT,
    open_questions_json      TEXT,
    evidence_json            TEXT,
    evidence_count           INTEGER DEFAULT 0,
    confidence               REAL DEFAULT 0.5,
    status                   TEXT DEFAULT 'candidate',
    created_at               TEXT,
    updated_at               TEXT
);
```

### 10.5 concept

```sql
CREATE TABLE concept (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_id              TEXT UNIQUE NOT NULL,
    workspace_id            TEXT,
    name                    TEXT NOT NULL,
    concept_type            TEXT,
    definition              TEXT,
    related_entities_json   TEXT,
    known_facts_json        TEXT,
    rejected_hypotheses_json TEXT,
    open_questions_json     TEXT,
    evidence_json           TEXT,
    confidence              REAL DEFAULT 0.5,
    status                  TEXT DEFAULT 'active',
    created_at              TEXT,
    updated_at              TEXT
);
```

---

## 11. CLI 设计

### 11.1 ingest-session

导入历史 session。

```bash
memory ingest-session --file session.md --workspace fp-cim
```

### 11.2 distill-session

蒸馏历史 session。

```bash
memory distill-session --session session_001
```

### 11.3 list-concepts

查看候选概念。

```bash
memory list-concepts --workspace fp-cim
```

### 11.4 show-concept

查看概念详情。

```bash
memory show-concept concept_mask_machine_relation
```

### 11.5 recall

根据问题生成 Memory Context。

```bash
memory recall --workspace fp-cim --query "继续看 MASK 和机器关系"
```

### 11.6 review

确认、拒绝、修改、合并概念。

```bash
memory review --candidate cc_mask_machine_relation --confirm
memory review --candidate cc_old_hypothesis --reject
```

### 11.7 prehook

Claude Code 调用前执行。

```bash
memory prehook --workspace fp-cim --query "$USER_PROMPT"
```

### 11.8 posthook

Claude Code 调用后执行。

```bash
memory posthook --workspace fp-cim --prompt "$USER_PROMPT" --answer "$AGENT_ANSWER"
```

---

## 12. LLM 抽取规范

### 12.1 Observation Extractor

任务：从原始经历中提取 Observation。

输出字段：

```text
subject
predicate
object
evidence_text
confidence
status
memory_type_candidate
```

约束：

```text
只抽取有证据的信息
区分用户提供、用户确认、用户否定、助手推测
否定信息必须保留
输出 JSON
```

### 12.2 Session Distiller

任务：从完整 session 中提取高价值长期记忆。

重点提取：

```text
架构事实
Bug 修复路径
排障路径
数据资产
用户偏好
任务状态
被否定假设
开放问题
```

约束：

```text
助手推测不能直接确认
用户否定要降权或记录为 rejected_hypothesis
每条记忆必须有 evidence
```

### 12.3 Concept Candidate Builder

任务：从 Observation 中生成候选概念。

候选概念必须包括：

```text
name
summary
source_terms
known_facts
rejected_hypotheses
open_questions
evidence
confidence
status
```

---

## 13. Recall 设计

### 13.1 召回目标

Recall 的目标不是"找相似文本"，而是：

```text
找到当前问题对应的概念，并展开该概念下最有用的上下文。
```

### 13.2 召回步骤

```text
1. 识别 workspace / repo / task
2. 抽取当前问题中的关键实体和意图
3. 匹配 Concept / Concept Candidate
4. 展开相关 facts / rejected hypotheses / preferences / task state
5. 压缩为 Memory Context
6. 注入 Claude Code
```

### 13.3 召回排序

优先级：

```text
当前 workspace
当前 repo
当前 task
概念匹配
用户确认事实
被否定假设
用户偏好
最近任务状态
历史 session 证据
```

置信度信号使用 `effective_confidence = extraction_confidence × fact_confidence`，防止来源不可信的推测通过反复召回累积到高排序。

### 13.4 Memory Context 限制

默认不超过 1500 tokens。

建议分配：

```text
当前概念：300
已知事实：400
被否定假设：250
用户偏好：200
任务状态：250
证据摘要：100
```

---

## 14. 验收标准

### 14.1 基础验收

系统必须做到：

```text
1. 能导入历史 session。
2. 能提取 Observation。
3. 能生成候选概念。
4. 能展示概念详情。
5. 能保留被否定假设。
6. 能基于用户问题召回 Memory Context。
7. 能区分用户确认和助手推测。
8. 能根据用户反馈调整记忆状态。
```

### 14.2 概念生长验收

给定多段围绕同一长期问题的 session，系统必须生成一个稳定候选概念，而不是生成一堆零散摘要。

输出应包含：

```text
概念名称
概念定义
相关实体
已知事实
被否定假设
开放问题
证据
置信度
```

### 14.3 召回验收

当用户输入：

```text
继续看某个长期问题
```

系统应召回对应概念，而不是返回一堆历史对话片段。

### 14.4 防错验收

如果某个方案只由助手提出、用户没有确认，则不能进入 confirmed。

如果用户明确否定某个方案，系统必须生成 rejected_hypothesis。

---

## 15. 给 Claude Code 的初始任务 Prompt

```text
你现在要实现 Memory Runtime MVP。

请先阅读：
- .ctl/spec/memory-runtime/principles.md
- .ctl/spec/memory-runtime/concept-growth.md
- .ctl/spec/memory-runtime/memory-model.md
- .ctl/spec/memory-runtime/recall-design.md
- .ctl/spec/memory-runtime/quality-control.md
- .ctl/spec/backend/roadmap.md

第一阶段目标：
1. 创建本地 SQLite 数据模型。
2. 实现 session ingest。
3. 实现 Observation Extractor 接口和 mock provider。
4. 实现 Session Distiller 基础流程。
5. 实现 Concept Candidate Builder。
6. 实现 memory recall 命令。
7. 能根据 query 输出 Memory Context。

约束：
- 核心是概念生长，不是文本存储。
- 每条记忆必须有 evidence。
- 助手推测不能直接 confirmed。
- 被否定假设必须保存。
- 召回优先概念，不优先原始历史文本。
- Memory Context 必须短、准、可操作。

请先给出实现计划，然后开始编码。
```

---

## 16. CLAUDE.md 建议内容

```text
# Claude Code Instructions for Memory Runtime

This project implements an Agent Memory Runtime.

Core principle:
Memory is not stored text. Memory is concept growth from historical observations.

You must follow these rules:

1. The system grows concepts from historical sessions and observations.
2. Every memory must have evidence.
3. Assistant guesses must not become confirmed memory without user confirmation or source evidence.
4. Negative feedback and rejected hypotheses are valuable memory.
5. Recall should prioritize concepts and task state over raw historical text.
6. Keep Memory Context short, relevant, and scoped to workspace/repo/task.
7. Do not inject large raw session history into prompts.

Primary flow:
RawMemory → Observation → ConceptCandidate → Concept → MemoryContext

Primary modules:
- raw memory store
- observation extractor
- session distiller
- concept growth engine
- memory retriever
- prehook/posthook integration
```

---

## 17. 最终闭环

Memory Runtime 的最小可行闭环：

```text
1. 导入历史 session。
2. 提取 Observation。
3. 蒸馏长期记忆。
4. 生成候选概念。
5. 用户 review / 系统升权。
6. 未来提问时召回概念。
7. Claude Code 根据 Memory Context 执行任务。
8. 用户反馈继续修正概念。
```

最终产品价值：

```text
让历史经历自动生长成用户、项目、团队自己的概念网络。
```

核心闭环：

```text
经历 → 观察 → 概念 → 召回 → 行动 → 反馈 → 修正
```
