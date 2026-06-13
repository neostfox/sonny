# Quality Guidelines

## Forbidden Patterns

### `unwrap()` in library code

```rust
// ❌ Forbidden in src/ (outside #[cfg(test)])
let conn = self.conn.lock().unwrap();
```

The `Mutex::lock().unwrap()` pattern is the one exception — it only panics on poison, which indicates a logic bug. All other `unwrap()` calls must be in `#[cfg(test)]`.

### `todo!()` or `unimplemented()` in non-stub code

Stubs (`recall/mod.rs`, `feedback/mod.rs`) may have placeholder comments. Active modules must not contain `todo!()` macros.

### `panic!()` for recoverable errors

All recoverable error conditions must return `MemoryError`. Only truly impossible states may panic (e.g., regex `unwrap()` on compile-time constants).

### Domain types importing infrastructure

`models/` must not import from `store/`, `llm/`, `embed/`, or `pipeline/`. Check with `cargo check` after adding imports.

### `Box<dyn Error>` instead of `MemoryError`

Never return `Box<dyn std::error::Error>`. Always use `MemoryResult<T>`.

## Required Patterns

### Derive standard traits on public types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation { ... }
```

### `Send + Sync` on all store and provider traits

```rust
pub trait ObservationStore: Send + Sync { ... }
```

### Column constants for SQL queries

```rust
const OBS_COLUMNS: &str = "observation_id, workspace_id, ...";
```

### Foreign key enforcement in SQLite

```rust
conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
```

Always enabled. Every open must set these pragmas.

### `INSERT OR IGNORE` for idempotent raw_memory ingestion

```rust
"INSERT OR IGNORE INTO raw_memory ..."
```

## Known Issues (from Code Review)

### 🔴 Critical

| ID | Issue | File | Impact |
|----|-------|------|--------|
| C1 | 连接所有权：每个 store move Connection，多 store 无法共享 | `connection.rs`, CLI `main.rs` | extract pipeline 无法同时使用两个 store |
| C2 | `find_by_entities` 用 `LIKE '%?%'` 搜索 JSON | `concept_store.rs:191` | 假阳性：搜索 "user" 匹配 "user_profile" |
| C3 | `dirs_home()` 用 `$HOME`，Windows 上不存在 | `config.rs:148` | Windows 上默认路径 `/tmp` |

### 🟠 High

| ID | Issue | File | Impact |
|----|-------|------|--------|
| H1 | `update_status` 接受裸 `&str` 不是枚举 | `store/traits.rs:18` | 拼写错误静默写入无效状态 |
| H2 | `obs_params` / `candidate_params` 每次 17-23 个 `Box<dyn ToSql>` 堆分配 | `observation_store.rs:26`, `concept_store.rs:34` | 批量插入时大量分配 |
| H3 | 所有 `parse_*` 状态函数静默默认值，无日志 | `observation_store.rs:192`, `concept_store.rs:306,317,329` | DB 损坏状态被隐藏 |
| H4 | `extract_session_id` 每次调用编译 Regex | `ingest.rs:188` | 不必要的性能开销 |
| H5 | `update_recall_stats` 不检查受影响行数 | `concept_store.rs:209` | 不存在的 concept_id 静默成功 |

### 🟡 Medium

| ID | Issue | File | Impact |
|----|-------|------|--------|
| M1 | `list_by_workspace` 无 LIMIT | `observation_store.rs:91` | 大 workspace OOM |
| M2 | `MemoryContext.token_count` 不生效 | `models/recall.rs:13` | 无截断，超 LLM token 限制 |
| M3 | `insert_batch` 用 `unchecked_transaction()` | `observation_store.rs:60` | 不验证 autocommit 状态 |
| M4 | `ConceptCandidate.status` 是 `ObservationStatus` | `models/concept.rs:22` | 无意义状态变体 |
| M5 | `MemoryItem` 表和模型存在但无任何代码 | `models/memory_item.rs` | 死代码 |
| M6 | `repair_json` 只处理 markdown fence | `extract.rs:139` | LLM 其他畸形输出未处理 |
| M7 | `open_in_memory()` 设置 WAL PRAGMA | `connection.rs:22` | 无害但误导 |

## Naming Conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Crate name | kebab-case | `memory-runtime`, `sonny-cli` |
| Module file | snake_case | `raw_memory_store.rs` |
| Struct / Enum | PascalCase | `Observation`, `ObservationStatus` |
| Enum variant | PascalCase | `FastStored`, `AutoConfirmed` |
| Function / method | snake_case | `extract_observations`, `canonical_key` |
| Constant | SCREAMING_SNAKE | `EMBEDDING_DIM`, `OBS_COLUMNS` |
| Type alias | PascalCase | `MemoryResult<T>` |
| SQL table | snake_case | `raw_memory`, `concept_candidate` |
| SQL column | snake_case | `observation_id`, `workspace_id` |

## Build Commands

| Command | Purpose |
|---------|---------|
| `cargo check` | Fast type-check without codegen |
| `cargo test` | Run all tests (unit + integration) |
| `cargo test -p memory-runtime` | Test single crate |
| `cargo clippy -- -D warnings` | Lint with zero warnings |
| `cargo build` | Full build |
| `cargo run -p sonny-cli -- --help` | Run CLI |
