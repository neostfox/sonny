# sonny 仓库代码审查报告

- 审查日期：2026-07-07
- 审查方式：全仓库逐文件人工审查（只读，未改动任何源码）+ 机械验证（cargo fmt / clippy / test）
- 范围：`crates/` 下全部 Rust 源码（src + tests）、SQL 迁移、CLI、fixtures、设计文档、corpus、`.gitignore`；`.git/ .ctl/ .claude/ .mindfs/ .omp/` 不在范围内
- 工作区状态：工作树干净；未跟踪文件仅 `.ctl/decisions.jsonl`（已被 `.gitignore:50` 忽略，最新提交 `52aa653 chore: ignore ctl decisions audit log`）

---

## 1. 总览

### 1.1 仓库概况

sonny 是一个 Rust workspace（resolver 2），实现跨项目的因果知识 / 记忆引擎：从会话文本抽取带证据的 Observation，聚类生成概念候选，维护概念/边级 Beta 后验置信度，支持 P6 分层提升（Project→Domain→Global）、P7 因果融合（do-统计量）、P8 结构类比迁移、P10 Dreaming 离线固化、P13 Compact Search（RRF 混合检索 + 双向图扩展）、P14 实体-属性时间线。

| 模块 | 文件数 | 代码行 | 说明 |
|---|---:|---:|---|
| `crates/memory-runtime/src` | 54 | 12,959 | 核心库：models(16) / store(11) / pipeline(12) / recall(2) / embed / llm / confidence / entity / feedback / text / config / error + 12 个 SQL 迁移 |
| `crates/memory-runtime/tests` | 7 | 1,800 | 集成/验收测试（P6/P7/P8/P10-12/P13/P14 + 主集成） |
| `crates/sonny-cli/src` | 1 | 695 | 单文件 CLI（clap）：init/ingest/extract/list/dream/consolidate/promote/recall-compact/feedback/seed/timeline/transfer |
| `crates/memory-test-fixtures/src` | 3 | 121 | mock LLM（正则匹配）+ 确定性 stub embedding |
| 迁移 SQL | 12 | — | `001_initial` → `012_p14_entity_timeline`，`user_version` 驱动 |
| 其他 | — | — | `docs/memory-runtime-design.md`（24.9KB 设计文档）、`examples/corpus/`（6 个种子 bundle，共 47 obs / 19 concepts / 12 relations）、`TODO.md`（P0–P14 全部 ✅） |

技术栈要点：SQLite（`rusqlite` bundled，WAL + 外键，`Arc<parking_lot::Mutex<Connection>>` 共享单连接）；OpenAI 兼容 HTTP 客户端（`reqwest`，`/chat/completions` + `/v1/embeddings`）；无 CI、无 `rustfmt.toml` / `clippy.toml`。

### 1.2 验证结果（本机实测，Windows / stable）

| 项目 | 结果 |
|---|---|
| `cargo fmt --all --check` | **FAIL**：96 个 diff hunk，涉及 29 个文件（feedback/mod.rs、models/{causal,persistence,relation,scope}、pipeline/{action_plan,cluster,corpus,dream,extract,link,promote,transfer}、recall/{compact,mod}、store/{concept_store,feedback_store,migration,relation_store,timeline_store,traits}、全部 7 个测试文件、sonny-cli/main.rs 等）。原因：代码实际按 >80 列书写，而仓库没有 `rustfmt.toml` 声明宽度，rustfmt 默认 80 列。 |
| `cargo clippy --all-targets` | **PASS**（无 error），12 条 warning：`timeline_store.rs:148` unused mut；`models/causal.rs:73` 可 derive Default；`compact.rs:128` / `compact.rs:363` / `traits.rs:187` too_many_arguments；`compact.rs:295` / `p10_12_quality_loop_test.rs:144` cloned_ref_to_slice_refs；`recall/mod.rs:179` needless_character_iteration；`concept_store.rs:272` vec_init_then_push；`relation_store.rs:252-256` if_same_then_else（两个分支代码相同）；`pipeline/p13 相关` 未使用 import；`main.rs:610` is_some 后不必要的 unwrap。 |
| `cargo test --workspace` | **PASS 191/191**：149 个单元/库测试 + 42 个集成测试（integration 14 + p6 7 + p7 4 + p8 5 + p10_12 7 + p13 3 + p14 2），0 失败。 |

### 1.3 总体评价

这是一个完成度相当高的个人规模（personal-scale）知识库原型：领域建模（Beta 证据、生命周期、作用域分层）严谨且注释充分，191 个测试覆盖了各 P 阶段的验收语义；store 层事务边界总体正确，feedback/observation 的原子性有专门设计与测试。主要问题集中在四类：**① 召回主路径上的 CJK 处理与死配置**（H1/H2）、**② 写路径（extract/dream/feedback）若干正确性缺口**（H3、M3-M5）、**③ 死代码/死依赖/文档漂移的持续累积**（见 §3 主题 1）、**④ 格式化基线缺失**（96 个 fmt hunk 无 CI 把关）。没有发现会在新建库上崩溃或静默损坏主链路数据的严重缺陷，因此本报告「严重」级为空；但有 5 个「高」级问题建议在下一轮修复中优先处理（§5）。

---

## 2. 审查发现

约定：每条含 `文件:行号` 锚点、证据、影响、建议修复。行号以当前工作树为准。

### 2.1 严重（阻塞主链路 / 数据损坏）

**（无）** — 主链路（ingest → extract → dedup → recall → feedback → dream → promote）在新建库上均能正确闭环，191/191 测试通过；未发现崩溃或静默数据损坏路径。

### 2.2 高

**H1. Compact Search 词法通道的 CJK token 过滤用字节长度**
- 位置：`crates/memory-runtime/src/recall/compact.rs:68-73`、`80-84`
- 证据：`for t in tokens { if t.len() >= 2 && d.contains(t) { score += 0.5 } }`。`str::len()` 是**字节**数：单个 CJK 字符 = 3 字节，`"机".len() == 3 >= 2` 通过过滤；而该过滤的本意是排除单字符（ASCII 标点/虚词）token。
- 影响：`recall-compact` 是 CLI 的主召回入口。对 CJK 查询，单字 token（中文里单字即词根很常见，如「查」「机」）会拿到 0.5/0.8 的词法加分，使本不该进入词法 Top-K 的概念被 RRF 融合进最终结果 → 中文查询下出现伪命中。另外对无空格 CJK 串，`split_whitespace()` 只产生整串 token，逐 token 加分分支基本与整串 `contains`（+2.0/+2.5）重复计数。
- 建议：改为 `t.chars().count() >= 2`；更彻底的做法是对 CJK 片段用 `text.rs` 已有的词汇切分（`extract_query_entities` 的 vocabulary 机制）替代空白切分。

**H2. `Settings.recall` / `Settings.confidence` / `Settings.clustering` 三段配置是死配置**
- 位置：`crates/memory-runtime/src/config.rs:46-54, 56-63, 136-147`；`crates/memory-runtime/src/recall/mod.rs:18-21, 54, 60-63`；`crates/sonny-cli/src/main.rs:421`
- 证据：`RecallConfig { semantic_weight: 0.6, entity_weight: 0.4, top_k, top_n, max_context_tokens: 1500, semantic_threshold: 0.4 }` 只被 `Settings::load()` 构造，全仓库无人读取；`recall/mod.rs` 硬编码 `SEMANTIC_WEIGHT=0.6 / ENTITY_WEIGHT=0.4 / SEMANTIC_THRESHOLD=0.0`，`RecallEngine::new()` 不接受任何配置（CLI 的 max_tokens 走命令行参数）。`ConfidenceConfig`（含 `decay_half_life_days`）同样无人读取——`recall/mod.rs:60-63` 的注释明写「wire from `ConfidenceConfig::decay_half_life_days` when ...」，但从未接线，`with_decay_half_life` builder 无调用方。`ClusterConfig` 同理（`ClusterEngine::default()` 硬编码相同数值）。
- 影响：配置是假象——用户/调用方以为可以调召回权重、语义阈值、遗忘半衰期，实际全部无效。且**默认值与硬编码不一致**（`semantic_threshold` 配置默认 0.4 vs 实际生效 0.0）：将来有人「顺手」把配置接上，召回行为会突然变化而无测试报警。
- 建议：三选一并加测试：(a) 让 `RecallEngine`/`ClusterEngine` 构造函数接受配置并由 CLI 注入；(b) 删除死配置段，只保留真正生效的字段；(c) 至少让硬编码常量 `const` 化到 `config.rs` 单一出处。

**H3. `extract_and_dedup` 的 Update/Merge 分支不做存活过滤，可能强化/顶替死行**
- 位置：`crates/memory-runtime/src/store/observation_store.rs:37-39`（`OBS_FIND_BY_ENTITY` 无 status 过滤）、`crates/memory-runtime/src/pipeline/extract.rs:230-315`、对照 `crates/memory-runtime/src/pipeline/action_plan.rs:77-96`（planner 用 `live()` 闭包过滤）
- 证据：planner 基于「存活行」决定 Add/Reinforce/Update/Merge；但执行分支里 `related.iter().find(|o| o.subject_text == ... && o.predicate == ...)`（Update，extract.rs:265-280）与 `related.iter().find(|o| subject+object 相同)`（Merge，extract.rs:282-298）都**没有**状态过滤，且 `related` 按 `created_at DESC` 排序 → 取到的是**最新一行（可能是死行）**。
- 影响：当 (subject, predicate) 下「最新行已死、更旧行存活」时（例如 v1 存活、v2 已被 v3 顶替而 v3 又死）：Update 分支会对死行 v3 再 `supersede`（no-op），插入新观察，**存活旧行 v1 不被顶替** → 同一 subject+predicate 出现两条活的不同 object 事实，冲突残留；Merge 分支则给死行累加证据——`observation_store.rs:53-54` 的注释明确说「bumping a dead row's evidence would be wrong」，执行分支却可能恰好这么做。
- 建议：执行分支复用 planner 的存活判定（抽一个 `is_live_obs()` 到 `models/status.rs` 或 `observation.rs` 共用）；或给 `OBS_FIND_BY_ENTITY` 增加与 `OBS_FIND_DUP_OBJ` 一致的 `status NOT IN ('superseded','rejected','deprecated')` 过滤，并同步修正 CLI `main.rs:587` 的用法预期。

**H4. 提升到 Domain 却不带 domain key：概念进入「Domain+NULL」陷阱态**
- 位置：`crates/memory-runtime/src/pipeline/promote.rs:50-57`；`crates/memory-runtime/src/models/scope.rs:128-159`；`crates/sonny-cli/src/main.rs:84-102（promote 参数定义）, 357-408（promote 执行）, 550-615（consolidate）`
- 证据：`evaluate_promotion` 在 cross_project≥2、conf≥0.75、sessions≥2 时返回 `Domain`（scope.rs:116-123）；`maybe_promote_concept` 对 Domain 目标取 `key = domain_key.map(...)`——CLI `promote`/`consolidate` 的 `--domain` 都是 `Option`，不传则 `scope_key = None`。此后 `is_visible`（scope.rs:139-142）：Domain 且 key=NULL → 对**所有**其他 workspace 不可见；而 `apply_promotion` 只升不降（scope.rs:149-151，同 rank 返回 false）→ **之后无法再通过 API 给同一概念补 domain key**（Domain→Domain 是 no-op），只能手工 SQL 或升到 Global。
- 影响：`sonny promote c123 --cross-project 2`（不传 --domain，完全合法的命令）把概念提升进一个「名字是 Domain、可见性和 Project 相同、且再也回不到 Domain 补 key」的中间态；后续每次 consolidate 的 `maybe_promote_concept` 对它反复空转。
- 建议：`maybe_promote_concept` 在 `target == Domain && domain_key.is_none()` 时返回「证据已达 Domain 但缺 key」的报告（不写入）或强制保持 Project 并记 warning；CLI 在 promote 到 Domain 时把 `--domain` 变为条件必填。

**H5. 未使用的 ML 依赖进了默认构建**
- 位置：`crates/memory-runtime/Cargo.toml:25-26, 37-39`
- 证据：`linfa = "0.7"` 与 `linfa-clustering = "0.7"` 是**非可选**依赖，全仓库 grep 零使用；candle 栈（`candle-transformers`/`candle-nn`/`tokenizers`）虽标 optional，但 `default = ["embedding"]` 且 `embedding` feature 恰好就是这三个 crate → **默认构建即编译整套 ML 栈**，而代码中无任何 `candle|tokenizers` 引用（grep 确认）。另 `strsim`、`pulldown-cmark` 也是零使用依赖。
- 影响：每次 `cargo build/test` 都多编译数十个 crate（candle/linfa 依赖树很大），显著拉长构建时间，并扩大供应链面；本地实验性依赖被固化成了正式构建配置。
- 建议：删除 `linfa`/`linfa-clustering`/`strsim`/`pulldown-cmark`；把 `embedding`（candle 栈）移出 default（`default = []`，需要本地推理时显式 `--features embedding`）；如保留请补 `#[cfg(feature)]` 使用点或 TODO 标注。

### 2.3 中

**M1. LLM 客户端错误被标成 Embedding 错误（5 处）**
- 位置：`crates/memory-runtime/src/llm/openai.rs:56, 93, 97, 105, 111`；对照 `crates/memory-runtime/src/error.rs`（存在 `MemoryError::LlmProvider` 变体但全仓库无使用点）
- 证据：LLM 的 HTTP 客户端构造、请求失败、非 2xx、JSON 解码、空补全共 5 处全部返回 `MemoryError::Embedding(...)`。
- 影响：调用方/日志无法区分「LLM 挂了」还是「embedding 服务挂了」；已有的正确错误变体被闲置。
- 建议：5 处改用 `MemoryError::LlmProvider`，并补一条断言错误类型的测试。

**M2. LLM 客户端：假健康检查、`max_tokens` 未发送、temperature 硬编码**
- 位置：`crates/memory-runtime/src/llm/openai.rs:114-116`（health_check 仅检查 api_key 非空）、请求体构造（无 `max_tokens` 字段）、temperature 固定 0.2；对照 `crates/memory-runtime/src/config.rs:20, 100`（`LlmConfig.max_tokens` 死字段）
- 影响：`health_check` 与 embedding 侧（真发请求）语义不对称，调用方会高估 LLM 可用性；抽取 prompt 很长，服务端默认 max_tokens 不足时 JSON 被截断，`repair_json` 救不回来（只处理围栏）；temperature 不可配。
- 建议：health_check 发一次最小 chat 请求（或文档注明为浅检查）；`ChatRequest` 增加 `max_tokens` 并从 `LlmConfig` 注入；temperature 配置化。

**M3. Supplement 反馈的实体合并会把整句 CJK 短语当成实体**
- 位置：`crates/memory-runtime/src/feedback/mod.rs:174, 356-378, 380-387`
- 证据：`merge_entities` 用 `split(|c| !(c.is_alphanumeric() || c=='_' || c=='-' || c > 0x7f))` 分词——CJK 字符（u32 > 0x7f）**不参与切分**，于是无空格的中文片段整体成为单个 token；只要 ≥2 字符且不在一个极小的停用词表（还有/补充/另外/加上/also/and/the/for/with/this/that/还要/以及）里就并入实体集。例：Supplement 触发文本「还有一个字段」→ 整个「还有一个字段」被当成一个实体写入 `related_entities_json`。
- 影响：中文反馈下概念实体集被整句短语污染 → 实体通道召回、`find_by_entities`、`reverse_expand` 的实体扩展全部被稀释，且污染经 `entity_concept` 索引（`update_concept` 逐字重同步）固化。
- 建议：对 CJK 片段复用 `text.rs` 的词汇/边界机制（先切词再过滤），或要求 Supplement 实体走与 extract 相同的 `canonical_key` + 白名单谓词；至少把「还有/这个/都是/相关」类高频黏着词补进停用表。

**M4. 反馈引擎读-写之间不是同一把锁（TOCTOU 丢更新）**
- 位置：`crates/memory-runtime/src/feedback/mod.rs`（读概念 α/β 的 lock 区间 ≈106-121 行，与写事务 ≈200-224 行分离）
- 证据：apply_feedback 先 `get_concept`（取锁-释放），分类/决策后再开事务写。两个并发反馈对同一概念都读到同一 (α,β)，各自 +delta 后先后写 → 后写覆盖先写，**丢一个增量**（feedback 账本两行都在，但概念后验只反映一次）。
- 影响：单人工具下概率低，但 CLI 可被 hook 并发调用（prehook/posthook 场景正是设计文档的目标用法）；账本与后验不一致会破坏「审计可回放」的承诺。
- 建议：把「读 α/β → 计算新值 → 写回 + 写账本」放进同一个事务（store 已有 `*_conn` 系列助手，模式现成），或用 SQL `UPDATE ... SET evidence_alpha = evidence_alpha + ?` 原子增量。

**M5. Dream 的 SoftenConflict 算了 α/β 却丢弃，且计算本身有循环 bug**
- 位置：`crates/memory-runtime/src/pipeline/dream.rs:214-230`（计算）、`344-356`（应用时 `alpha: _, beta: _` 直接丢弃）
- 证据：`plan_actions` 里 `beta = o.evidence_beta + 1.5` 在 `for o in live` 循环内被逐行**覆盖**（最终值 = 最后一行的 β+1.5，不是求和/最大）；随后应用阶段完全不用这对值，改为对每行 `update(&ConflictingEvidence)`。
- 影响：死计算 + 潜在误导：读代码的人会以为软化幅度由 plan 阶段决定；若将来有人「修复」应用阶段改用它，会继承循环 bug。
- 建议：删掉 `plan_actions` 中 SoftenConflict 的 alpha/beta 字段（或让应用阶段真正消费它，并把循环改成求和/取 max 的显式语义）。

**M6. 迁移 009 `DROP TABLE IF EXISTS feedback` 存在升级丢数据窗口**
- 位置：`crates/memory-runtime/src/migrations/009_p4b_p5a_foreign_keys.sql:17-18`；提交历史 `cd3b412`（008 入库）与 `f28ca5c`（009 入库）是两个独立提交
- 证据：009 的注释断言「empty at migration time」，这对全新库成立；但在 `cd3b412..f28ca5c` 之间构建并写入过 feedback 行的用户，升级时所有反馈行被 DROP 掉（concept_relation 走的是保数据的 rebuild，feedback 不是）。
- 影响：一次性升级数据丢失（账本不可再生）。
- 建议：既然迁移已发布，保留现状但补注释说明该窗口；若未来再做表重建，统一用 009 的 rename-copy-recreate 模式而不是 DROP。

**M7. `apply_bundle`（corpus 种子）非事务，且重复 seed 会向时间线追加版本**
- 位置：`crates/memory-runtime/src/pipeline/corpus.rs:107-267`
- 证据：逐行插入（每次调用单独取锁、autocommit），中途失败留下半个 bundle；对每个 seed observation 每次都 `record_from_observation`（corpus.rs:178-184 附近）→ **幂等的重复 seed 也会产生新的 timeline 版本**（旧版本变 superseded）。
- 影响：`seed` 命令重跑（脚本化初始化很常见）污染 P14 版本历史：同一事实出现 N 个「版本」。
- 建议：`apply_bundle` 包一层事务；timeline 记录改为「仅当 (entity, property, value) 的 active 版本不存在或值变化时 append」。

**M8. `created_at` 混用时间戳格式**
- 位置：`crates/memory-runtime/src/pipeline/ingest.rs:160-176`（JsonSessionParser 用 `session.date`，如 `"2026-05-16"`）、`ingest.rs:85-98`（journal 用 `**Date**` 原值）；其他路径写完整 RFC3339（含可选小数秒）
- 证据：同一列被所有 `ORDER BY created_at` 依赖；裸日期与 RFC3339 混排时，前缀关系使裸日期恒排在同日期时间戳之前（大致可接受），而 `chrono::to_rfc3339()` 的小数秒变体之间（`…00.123Z` vs `…00Z`）字典序与时间序在毫秒级不一致。
- 影响：排序在毫秒级/日期粒度上可能与真实时间顺序有偏差；`last_recalled_at` 衰减计算解析失败时回退 1.0（recency 不惩罚），掩盖了格式问题。
- 建议：入库前统一归一化为 `YYYY-MM-DDTHH:MM:SSZ`（固定无小数秒）；对解析失败的旧行保留回退行为。

**M9. 证据门禁只检查单条消息子串，跨消息引文被当幻觉丢弃**
- 位置：`crates/memory-runtime/src/pipeline/extract.rs:113-118`（`evidence_is_supported`）
- 证据：`raw_memories.iter().any(|m| m.content.contains(ev))`——prompt 允许 evidence 是「verbatim quote from the input」，而 input 是多条消息拼接；LLM 引用「user 说 X + assistant 确认 Y」的拼接引文时，任何单条消息都不包含完整引文 → 整条观察被丢弃。空白/标点归一化差异同样导致误杀。
- 影响：防幻觉门禁误杀合法证据（宁缺毋滥是设计取向，可接受），但对「用户确认助手提议」这类高价值信号（user_confirm 源）损失偏大。
- 建议：门禁改为对「整个 session 拼接文本」做子串检查，并对引文做空白归一化后再比较。

**M10. 聚类入口不按状态过滤，死观察参与聚类**
- 位置：`crates/memory-runtime/src/pipeline/cluster.rs:47-62`（`list_by_workspace(workspace_id, None)` 全量）
- 证据：superseded/rejected/deprecated 观察的 embedding 不会被删除（embedding 表无级联清理），于是死行也进入 HAC 聚类，可能凑出候选。
- 影响：候选概念可能由已死证据支撑；`build_candidates` 的置信度混合了死行后验。
- 建议：`cluster_workspace` 只取存活观察（同 H3 的 live 集合），或在 SQL 层 join observation 状态。

**M11. `consolidate` 每次都跑 `link_workspace`，边证据非幂等累积**
- 位置：`crates/sonny-cli/src/main.rs:550-615`（Consolidate：dream → link → promote）；`crates/memory-runtime/src/pipeline/link.rs:20-22`（文档自认「re-running re-accumulates」）、`96-140`（shared_entity/coclaim 每对每 entity 记一次证据）
- 证据：`record_evidence` 每次调用都 +1 证据并推进 lifecycle；没有任何「本批证据已记账」的幂等键（batch/obs id 不在 relation 表里）。CLI 里 `consolidate`（main.rs:550-615）每次都执行 link，且还有独立的 `sonny link` 命令（main.rs:490-508）可任意次重跑。
- 影响：每跑一次 `sonny consolidate`（或 `sonny link`），所有 shared_entity/shared_session/causal 边的 evidence_count 与 α 再涨一轮；反复运行会无依据地把边推到 confirmed（causal 边的 do-统计量倒是按 max 合并，不受影响）。
- 建议：给 `record_evidence` 增加幂等维度（如 relation 行记 last_batch_id/last_obs_id，重复跳过），或 link 改为「按 obs id 记账」的增量模式。

**M12. CLI `extract`：全 workspace 单次抽取 + 报告数字量纲错误**
- 位置：`crates/sonny-cli/src/main.rs:429-469`（Extract arm）、`443`（10k 上限）、`457-463`（逐条插入循环）、`464-468`（报告）
- 证据：(a) 无 `--session` 时取 `list_by_workspace(&workspace, 10_000)` 的**全部会话**塞进一次 `extract_and_dedup` → 一个 extraction_batch_id 横跨所有会话，P2-C 的 coclaim 边（「同一批次共称」）把无关会话的观察连成一片，直接污染聚类亲和度与 shared_session 边；10k 条消息进单个 prompt 也有 token 爆炸风险。(b) 第 465 行附近输出 `reinforced/updated in-place = {raws.len() - kept.len()}`——`raws.len()` 是**原始消息数**，`kept.len()` 是**抽取出的观察数**，两者相减没有语义（例：10 条消息抽出 5 条且全部新增，会显示「reinforced/updated in-place = 5」）。
- 影响：(a) 是数据质量隐患（coclaim 是聚类/建边的核心信号）；(b) 是误导性 CLI 输出。
- 建议：(a) 默认按 session 分组循环调用 `extract_and_dedup`（每会话一个 batch），`--all` 才允许跨会话；(b) 让 `extract_and_dedup` 返回 `extracted_total`，报告用 `extracted_total - kept.len()`。

**M13. `reverse_expand` 逐种子实体全量扫描 + 死数据结构**
- 位置：`crates/memory-runtime/src/recall/compact.rs:280-338`
- 证据：对每个种子实体 `e` 调一次 `find_by_entities`（OK），开启 elevated 时**每个 `e` 再调一次 `list_visible_concepts`（全量 SELECT：本 ws + 所有 domain/global 概念）**，然后在 Rust 里逐概念解析 `related_entities_json` 判断是否含 `e`——S 个实体 × V 个可见概念的重复全表扫描与重复 JSON 解析；另外 280-289 行构建的 `entity_owners` 在 334 行 `let _ = entity_owners;` 直接丢弃。
- 影响：`recall-compact --no-global=false`（默认开）时的主要性能热点；个人规模下尚可，概念量上千后可感知。死结构增加阅读噪音（clippy 已报 295 行 cloned_ref）。
- 建议：把「可见概念 → 实体索引」一次性建好（BTreeMap<entity, Vec<concept_id>>），种子实体查表；删除 `entity_owners`。

**M14. `find_structural_peers` 对每个候选概念重复拉取整个 workspace 数据**
- 位置：`crates/memory-runtime/src/pipeline/transfer.rs:264-307`
- 证据：循环内对每个 candidate（其他 workspace 的全部 Active 概念）都执行 `observations.list_by_workspace(candidate.workspace)` + `relations.list_by_workspace(...)` + `concepts.list_concepts(...)`——同一 workspace 被反复全量拉取 K 次（K = 该 ws 概念数）。
- 影响：transfer 是离线命令，量小时无感；workspace 概念多时 O(K×N) 全表扫描。
- 建议：按 workspace 缓存（HashMap<String, (obs, rels)>），每 ws 只查一次。

### 2.4 低

**L1. `extract` 批内所有观察共用 `raw_memories[0].memory_id`** — `extract.rs:141-142`。FK 指向的是「该会话第一条消息」，不是真正产出该观察的消息；溯源粒度停留在会话级。建议：prompt 让 LLM 回带消息序号，或文档明示 memory_id 只是会话代理。

**L2. Add 路径先 `sync_cross_project_count` 后插入，计数少当前 workspace** — `extract.rs:300-304` + `observation_store.rs:230-258`。新行以 count=1 落地，其他 workspace 的行被更新为「不含新 ws 的 distinct 数」；要等下一次跨项目 Add 才收敛。语义是「其他独立 workspace 数」，与列注释「how many distinct workspaces」差 1。建议：插入后补一次 sync，或文档明示语义。

**L3. `dream_workspace` 每次跑两遍全量 `list_by_workspace`** — `dream.rs:261, 273`。无功能问题，纯性能。建议合并为一次查询。

**L4. Dream 去重合并只给 +1 证据，不转移被并行的证据质量** — `dream.rs:322-343`（`let _ = dup;` 后按 `RepeatedOccurrence` 每重复行 +1）。被 supersede 行的 α/β 质量直接丢弃，keep 行只增 1。保守但偏损：两条 α=5 的重复合并后 α 仅 +1。建议：把 dup 行的 (α,β) 差值并入 keep 行（或至少在报告里披露丢弃量）。

**L5. `sync_entity_concepts` 的 DELETE+重建不包事务** — `concept_store.rs:588-600`。逐语句 autocommit，中途失败留下半重建的 `entity_concept`。建议：包 `unchecked_transaction`（单连接持锁，安全）。

**L6. 枚举解析严格/宽容不一致** — `concept_store.rs:549-578`（宽容：default + warn）、`observation_store.rs:456-478`（宽容）、`raw_memory_store.rs:137-148`（宽容）vs `feedback_store.rs:38-60` / `relation_store.rs:51-70`（严格：错误上抛，且注释专门解释了为什么严格）。同一代码库两种哲学，读者无法判断哪个表「未知值该报错」。建议：定一条规则（持久化不变量建议全严格）并统一。

**L7. Embedding 检索是纯暴力扫描，向量小端编码无版本头** — `embedding_store.rs:19-89`（每次查询加载 workspace+elevated 全部行、解码 BLOB、Rust 算余弦；`has_vec` 恒 false）、`92-98`（LE f32 流，无维度/魔数头，跨字节序机器拷贝 DB 会静默错）。个人规模可接受；建议 BLOB 前加 8 字节头（魔数+维度），为 sqlite-vec 替换留路。

**L8. 种子概念伪造召回统计** — `corpus.rs:205-220`（`recall_count: 1, successful_recall_count: 1`）。种子概念没有任何真实召回却声称有 1 次成功召回：抬升 Ebbinghaus 新鲜度与 `unique_session_count()` 提升代理（concept.rs:105-107）。建议：置 0，或加 `seeded` 标记。

**L9. Journal 解析的 `role` 是章节名而不是说话人** — `ingest.rs:204-227`（role = "summary"/"main changes"/"header"），而 extract prompt 假定 role ∈ {user, assistant}（`extract.rs:21-22`）；journal 内容会以 `[summary]: ...` 形式进抽取 prompt。功能上能跑，但语义错位。建议：journal 统一 role="journal"，prompt 增加该 role 的说明。

**L10. WL 指纹对外部端点泄漏实体名** — `transfer.rs:65-81`（`for_concept` 只要边的一端在实体集就收边，另一端可能在节点集外）、`transfer.rs:122-131`（WL 迭代对外部端点 `unwrap_or(&e.dst)` / `unwrap_or(&e.src)` 用**原始名**）→ 涉及外部实体的边使「名字无关」的指纹带上名字，两个同构模块因外部实体不同名而失配。建议：要么只收两端都在节点集内的边，要么把外部端点折叠成统一的 `*ext*` 标签。

**L11. `rank()` 对每个候选单独 `get_concept`，`load_ranked_concepts` 再取一次 Top5** — `recall/mod.rs:209-210, 253-261`。N 很小（≤SEMANTIC_TOP_K+实体命中），属轻微冗余读；建议 `rank` 直接返回带 concept 的评分或提供批量 `get_concepts`。

**L12. `Database.has_vec` 与 `EmbeddingConfig.prefer_vec` 均为死字段** — `connection.rs:12,19,23`（恒 false 且无人读）、`config.rs:36`。建议删除或实现 sqlite-vec 路径。

**L13. `entity_alias` 表是死 schema** — `migrations/001_initial.sql:119-131`：建表后全仓库无任何读写代码（`memory_item` 已在 003 删除，entity_alias 没有）。建议：下个迁移里 DROP 或标注保留原因。

**L14. timeline 状态宽容解析默认 `Expired`** — `timeline_store.rs:33`：未知状态被静默当 Expired（当前值消失且无告警），与 L6 的严格/宽容之争同理。建议至少 `warn!`。

**L15. 不存在的概念提升时报告 `from: Project`** — `promote.rs:36-44`：概念查不到时返回 `from: LifecycleScope::Project`（捏造值）+ `to: None`，CLI 会把 `Project → -` 打印成正常报告。建议：`from` 改为 `Option` 或在报告里显式 `missing: true`。

### 2.5 吹毛求疵

- **N1.** 死代码：`action_plan.rs:37-40` `ExistingMatch` 公开但无人用；`build_recall_envelope`/`envelope_prompt_block`（action_plan.rs:111-140）只有测试用；`corpus.rs:274-277` `_lifecycle_hint` 带 `#[allow(unused)]` 的哑函数；`pipeline/p13` 相关模块有未使用 import（clippy 已报）。
- **N2.** `cluster.rs:249-255`：`source_sessions_json` 字段实际存的是 **memory_id**（消息行 id），不是 session id，命名误导。
- **N3.** `relation_store.rs:252-256`：`if edge.causal_stats.p_do.is_none() && stats.p_do.is_some() { *new = *old } else if stats.p_do.is_some() { *new = *old }` —— 两分支相同，第一个条件冗余（clippy if_same_then_else）。
- **N4.** `extract.rs:271`：`superseded_by = format!("update:{}", new_id)`（及 dream 的 `"dream-merge:"` 前缀）——provenance 字段里塞非观察 id 的哨兵串，将来按 id join 会踩坑；建议用独立列或约定前缀文档。
- **N5.** `models/recall.rs:120`：`RecallBudget::for_intent` 预留 250 token 的「头部」，但 `to_prompt` 从不核算这 250；各段百分比之和恰为 100，预算实际只花在段落上。
- **N6.** `Settings`/`LlmConfig` derive `Debug` 且含 `api_key`（config.rs:3,15）——当前无人打印 Settings，暂无泄漏；建议 api_key 字段用自定义 Debug 打码，防止将来 `println!("{:?}", settings)`。
- **N7.** `.gitignore:41` 忽略了 `Cargo.lock`——对含二进制（sonny-cli）的 workspace，Rust 社区惯例是提交 lock 以保证可复现构建。
- **N8.** 设计文档漂移：`docs/memory-runtime-design.md` §11（CLI）描述的是 `recall / review / prehook / posthook / distill-session`，实际 CLI 是 `recall-compact / feedback / consolidate / seed / timeline / transfer / dream / link / promote / reextract`；§10 的 DDL 也是 001 时代快照。文档作为「原始设计」保留没问题，但建议加一行「实现已演进，以代码与 TODO.md 为准」。
- **N9.** `main.rs:239, 435`：`timeout_secs.max(5)` / `.max(10)` 静默抬高用户配置的超时下限，行为与配置不符（配 1s 实际 5s）。
- **N10.** `models/concept.rs:105-107`：`unique_session_count() = max(connection_count, recall_count)` 是文档化的近似代理，但名字里「session」会误导（连接数≠会话数）；提升决策依赖它（scope.rs:92）。建议改名 `activity_proxy`。
- **N11.** `ingest.rs:247-252`：`use std::sync::LazyLock;` 写在 impl 块中间（风格）。
- **N12.** `tests/p8_transfer_test.rs` 里自环用例的注释「will be rejected as a self-loop — use a second concept instead」——测试先故意造自环再断言报错，setup 绕了一圈，可简化为直接两个概念。
- **N13.** `transfer.rs:400-416`：`fingerprint_bag` 用 `split('|').skip(2)` 解析指纹——仅当 `rounds >= 1`（标签被 `short_hash` 换成无 `|` 的 16 位 hex）时成立；`rounds == 0` 时初始标签形如 `d1|out:causes:1`（含 `|`），解析会错位（静默丢失，不报错）。默认 `DEFAULT_WL_ROUNDS = 2`，实际不触发。
- **N14.** `feedback/mod.rs:283-313`：分类表顺序是 Negate → Confirm → Supplement → Correct → Preference，因此「对，应该是 X」（确认+纠正混合）会被归为 **Confirm**（+2α），纠正动作（supersede）丢失。注释只说明了「negate 先于 confirm」，Confirm/Correct 的先后是隐式决定，建议补注释或让 Correct 优先于 Confirm（「应该是」语义更具体）。

---

## 3. 跨模块主题

1. **死代码 / 死依赖 / 死配置的持续累积**（H2、H5、L7、L12、L13、N1）：`Settings.recall/confidence/clustering`、`linfa`×2、candle 栈进 default feature、`strsim`/`pulldown-cmark`、`has_vec`/`prefer_vec`、`entity_alias` 表、`ExistingMatch`、envelope 函数……每类都有 2-5 处。项目演进很快（P0→P14），旧接口/旧依赖没有被清理流程兜住。建议：引入 `cargo udeps`（或定期 `cargo hack` 检查）+ 一条「pub API 必须有使用点或 `#[doc(hidden)]` 标注」的 lint 纪律。
2. **「存活」状态定义分散且不一致**（H3、M10 的根因）：`observation_store` 的 SQL NOT IN 列表（2 处）、`action_plan` 的 `live()` 闭包、`link.rs`/`dream.rs`/`transfer.rs` 各自的 `is_live`（排除 Disputed）、recall `rank()` 的语义通道（**包含** Disputed，实体通道仅 Active）——同一概念至少 5 处定义，排除集不同（Disputed 的取舍尤其分裂）。建议：在 `models/status.rs` 提供 `ObservationStatus::is_live()` 单一实现，各处引用，并对「Disputed 可见性」写一条设计注释。
3. **事务边界不一致**（M4、M7、L5、H3 的连锁）：observation/feedback/relation/timeline 的核心写路径都有专门的事务设计与测试（这是本仓库做得最好的部分之一），但 entity_concept 重建、corpus seed、CLI extract 循环、extract_and_dedup 的多语句变更没有同等保护。建议：给 store 增加「组合操作」原语（如 `upsert_concept_with_entities`），而不是在 pipeline 层手工拼接多句写。
4. **`created_at` 格式混用**（M8）：RFC3339、裸日期、测试里的 `"t0"`/`"t"` 同列共存，所有排序/衰减都依赖它。个人工具影响小，但这是全库共享的隐式契约，值得一次性归一。
5. **性能热点全部是「全表扫描 + Rust 里算」**（M13、M14、L7、L11）：embedding 暴力余弦、reverse_expand 的逐实体全量 list、transfer 的逐候选重复拉取、rank 的逐候选 get。全部在文档里以「personal-scale」自洽，且都有注释——不是缺陷，但集中在一处说明：检索路径没有统一的「一次加载、多次使用」纪律。
6. **严格解析 vs 宽容解析**（L6、L14）：feedback/relation 两个 store 有专门注释解释「为什么严格」，其余 store 宽容 + warn。建议统一为严格（这些表的枚举值都是代码自己写进去的，未知值即 bug）。

---

## 4. fmt / clippy / test 明细

- **fmt**：`cargo fmt --all --check` 失败，96 个 hunk / 29 个文件。仓库无 `rustfmt.toml`，代码实际书写宽度约 100 列，rustfmt 默认 80 列导致大面积「假 diff」。建议二选一：加 `rustfmt.toml`（`max_width = 100`，与现状一致）或跑一次 `cargo fmt --all` 提交；并加 CI 跑 `--check`。
- **clippy**：0 error / 12 warning，清单见 §1.2 表格。其中 `relation_store.rs:252-256` 的 if_same_then_else 是真冗余（N3），其余为风格/签名类。
- **test**：191/191 通过（149 库内 + 42 集成）。测试质量总体好：P4-B 反馈闭环、P2-D reextract 原子性、P6 提升门槛/别名解析、P7 因果权重、P10 去重幂等（二次 dream 零动作）、P14 时间线唯一 active 都有端到端断言。缺口主要在**并发**（无锁/事务边界的并发测试，见 M4）与**CJK 边界**（H1 场景无测试覆盖）。

---

## 5. 修复优先级建议

| 优先级 | 条目 | 理由 |
|---|---|---|
| **P0（先修）** | H3（extract 分支存活过滤）、M6（009 丢数据窗口的后续防护）、M12b（CLI 报告量纲） | 正确性：H3 会在真实使用中静默产生重复活事实；M12b 是用户直接看到的错误数字 |
| **P1（本周）** | H1（CJK 字节长度）、H2（死配置接线或删除）、H4（Domain 无 key 陷阱）、M5（dream 死计算）、M3（CJK 实体污染） | 主链路质量：H1/H4/M3 都直接决定中文用户的召回/提升体验；H2 是配置语义的谎言 |
| **P2（本迭代）** | H5（依赖瘦身）、M1/M2（LLM 客户端）、M4（反馈原子性）、M7（seed 事务+时间线幂等）、M8（时间戳归一）、M9（证据门禁）、M10（聚类存活过滤）、M11（link 幂等）、M12a（按 session 抽取）、M13/M14（检索路径全表扫描） | 架构卫生与性能：H5 立竿见影缩短构建；M11/M12a 影响数据质量积累 |
| **P3（随手）** | L1-L15、N1-N14 + fmt 基线（rustfmt.toml 或一次全量 fmt）+ CI | 低风险清理；fmt 先定基线，否则每次 PR 都会被 96 个 hunk 淹没 |

---

*报告完。修复时请逐条对照 §2 的 `文件:行号` 锚点；本审查未改动任何源码。*
