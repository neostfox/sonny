# Infrastructure Layer Spec — store/

## Purpose

SQLite-backed persistence for all domain entities. Provides trait-based interfaces for dependency inversion and testability.

## Directory

`crates/memory-runtime/src/store/`

## Allowed Imports

- `crate::models::*` — domain types for mapping
- `crate::error::{MemoryError, MemoryResult}` — error handling
- `rusqlite` — SQLite bindings
- `std::sync::Mutex` — thread-safety for `Connection`

## Forbidden Imports

- `crate::pipeline` — store must not depend on application logic
- `crate::llm` — no LLM calls in store layer
- `tokio` — store is synchronous (Mutex-guarded)

## Patterns

### Connection wrapper with WAL mode

```rust
// crates/memory-runtime/src/store/connection.rs:7-19
pub struct Database {
    pub conn: Connection,
    pub has_vec: bool,
}
```

- WAL mode for concurrent read/write
- Foreign keys enforced
- Migrations run on every open

### Mutex-guarded connection per store

Each store struct owns a `Mutex<Connection>`:

```rust
// crates/memory-runtime/src/store/raw_memory_store.rs:10-12
pub struct SqliteRawMemoryStore {
    conn: Mutex<Connection>,
}
```

Lock is acquired per-method-call. No transaction spanning multiple methods.

### Versioned migrations with `user_version` PRAGMA

```rust
// crates/memory-runtime/src/store/migration.rs
const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../migrations/001_initial.sql")),
];
```

### Trait-based store interfaces

```rust
// crates/memory-runtime/src/store/traits.rs
pub trait ObservationStore: Send + Sync {
    fn insert(&self, obs: &Observation) -> MemoryResult<()>;
    fn insert_batch(&self, observations: &[Observation]) -> MemoryResult<()>;
    // ...
}
```

### Column string constants

```rust
// crates/memory-runtime/src/store/observation_store.rs:21-24
const OBS_COLUMNS: &str = "...";
```

### Row → domain type mapping helpers

```rust
fn row_to_observation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Observation> { ... }
```

### Parse functions for string → enum

```rust
fn parse_observation_status(s: &str) -> ObservationStatus { ... }
```

## Known Issues

### C1: 连接所有权模型 — 多 store 无法共享连接

`Database::open()` 返回拥有 `conn` 的结构体。每个 store 通过 `new(conn)` 获取它——这会 move 连接，`Database` 变空壳。

```rust
// sonny-cli/src/main.rs:67
let db = Database::open(&cli.db).expect("...");
let store = SqliteRawMemoryStore::new(db.conn);  // db.conn 被 move
```

后果：无法同时使用 `RawMemoryStore` + `ObservationStore`（extract pipeline 需要）。集成测试用 `std::mem::replace` 绕过。

**修复**: `Database` 持有 `Arc<Mutex<Connection>>`，store clone Arc。

### C2: find_by_entities 用 LIKE 搜索 JSON — 假阳性

```rust
// concept_store.rs:191
format!("related_entities_json LIKE '%' || ?{} || '%'", i + 3)
```

- `LIKE '%user%'` 匹配 `user_profile`、`superuser_config`
- 全表扫描，无法利用索引
**修复**: 实体→概念连接表，或 rusqlite JSON1 扩展。

### H1: update_status 接受裸 `&str`

```rust
// store/traits.rs:18
fn update_status(&self, observation_id: &str, status: &str) -> MemoryResult<()>;
```

拼写错误会静默写入无效状态到 DB。应接受 `ObservationStatus`。

### H2: 每次 insert 分配 17-23 个 Box<dyn ToSql>

`obs_params()` 和 `candidate_params()` 对每个字段 `Box::new(field.clone())`。rusqlite `params![]` 宏在栈上构建，零分配。

### H3: 所有 parse_* 状态函数默认值无日志

```rust
// observation_store.rs:192
_ => ObservationStatus::Candidate,  // 静默吞掉未知状态
```

DB 中损坏的状态字符串变成 Candidate，无任何诊断信息。

## Schema Design Gaps

### D1: 缺少 observation_candidate_link 表

设计 §5.3 要求 Observation 聚合成 ConceptCandidate。当前无关联表——Candidate 是一次性 insert 的扁平对象，无法增量生长。

**需要**:

```sql
CREATE TABLE observation_candidate_link (
    observation_id TEXT NOT NULL REFERENCES observation(observation_id),
    candidate_id   TEXT NOT NULL REFERENCES concept_candidate(candidate_id),
    linked_at      TEXT NOT NULL,
    PRIMARY KEY (observation_id, candidate_id)
);
```

### D9: 缺少 feedback 表

设计 §3.4 修正链路要求存储用户反馈。`FeedbackType` 和 `FeedbackResult` 模型存在，无 SQL 表。

**需要**:

```sql
CREATE TABLE feedback (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    feedback_type   TEXT NOT NULL,
    target_type     TEXT NOT NULL,  -- 'observation' | 'concept' | 'candidate'
    target_id       TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    user_input      TEXT,
    old_confidence  REAL,
    new_confidence  REAL,
    old_status      TEXT,
    new_status      TEXT,
    created_at      TEXT NOT NULL
);
```

## Anti-patterns

| Don't | Why | Instead |
|-------|-----|---------|
| Share a `Connection` across stores without Mutex | rusqlite `Connection` is not `Sync` | `Arc<Mutex<Connection>>` 共享 |
| Skip `run_migrations` on open | Schema may be outdated | Always call in `Database::open` |
| Use `unwrap()` on SQL results | Database errors must propagate | Use `?` with `MemoryResult` |
| Put SQL in model types | Violates layer boundary | Keep SQL in store files |
| Forget `INSERT OR IGNORE` for raw_memory | Duplicate session re-ingest | Use `INSERT OR IGNORE` on unique columns |
| 用 `LIKE` 搜索 JSON 字段 | 假阳性 + 全表扫描 | 使用连接表或 JSON1 扩展 |
| 解析状态时静默默认值 | 隐藏数据损坏 | 至少 `tracing::warn!()` |
| store trait 接受 `&str` 作为状态 | 无编译时验证 | 接受枚举类型 |

## Testing

- In-memory SQLite via `Database::open_in_memory()` or `Connection::open_in_memory()`
- Migration test: verify `user_version` after fresh DB
- Store CRUD tests: insert → get → list → update
- Batch insert tests for `insert_batch`
- **缺失**: 多 store 共享连接的并发测试
- **缺失**: batch insert 部分失败回滚测试
- **缺失**: LIKE 搜索假阳性测试
