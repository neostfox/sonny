# Research: Entity Normalization for Memory Runtime

- **Query**: Entity normalization approaches for a Rust-based memory system handling LLM-extracted observations with subject/predicate/object triples. Same entity may appear as "User Model", "user_model", "UserModel", "user model", "UM".
- **Scope**: Mixed (internal codebase analysis + external library evaluation)
- **Date**: 2026-05-16

## Findings

### Files Found

| File Path | Description |
|---|---|
| `crates/memory-runtime/src/entity/mod.rs` | Placeholder module: `// Entity normalization — implemented in Phase 1` |
| `crates/memory-runtime/src/migrations/001_initial.sql` | Contains `entity_alias` table schema already defined (lines 119-131) |
| `crates/memory-runtime/src/models/observation.rs` | Observation model with `subject_text`, `object_text` fields that need normalization |
| `crates/memory-runtime/src/store/traits.rs` | `ObservationStore` trait with `find_by_entity()` and `check_duplicate()` methods |
| `crates/memory-runtime/src/store/concept_store.rs` | Placeholder store implementation |
| `crates/memory-runtime/Cargo.toml` | Current dependencies (rusqlite, serde, chrono, ndarray, thiserror, etc.) |
| `.trellis/spec/memory-runtime/memory-model.md` | Full data schema spec including entity_alias table design |
| `.trellis/spec/memory-runtime/concept-growth.md` | Pipeline spec; entity names flow through subject_text/object_text |
| `.trellis/spec/memory-runtime/quality-control.md` | `are_semantically_equivalent()` uses `subject_text.contains()` currently |
| `.trellis/spec/memory-runtime/recall-design.md` | Entity recall channel (40% weight) depends on entity matching |
| `.trellis/spec/memory-runtime/roadmap.md` | Phase 1 task 1.3: entity normalization layer; Phase 4.5: alias active learning |

### Existing Database Schema

The `entity_alias` table is already defined in `001_initial.sql` (lines 119-131):

```sql
CREATE TABLE IF NOT EXISTS entity_alias (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_form  TEXT NOT NULL,
    alias_form      TEXT NOT NULL,
    workspace_id    TEXT,
    confirmed       BOOLEAN NOT NULL DEFAULT 0,
    co_occurrence_count INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    UNIQUE(canonical_form, alias_form)
);

CREATE INDEX IF NOT EXISTS idx_alias_canonical ON entity_alias(canonical_form);
CREATE INDEX IF NOT EXISTS idx_alias_alias ON entity_alias(alias_form);
```

Key design details:
- `canonical_form`: The normalized/primary entity name
- `alias_form`: An alternate spelling or casing of the same entity
- `workspace_id`: Nullable, allows workspace-specific aliases
- `confirmed`: Boolean flag; unconfirmed aliases are candidates (active learning in Phase 4)
- `co_occurrence_count`: Tracks how often alias and canonical form appear together
- Index on both `canonical_form` and `alias_form` for bidirectional lookup

### Entity Normalization Techniques

#### 1. Deterministic Normalization (Rule-Based)

This is the primary approach for Phase 1 MVP. A pipeline of string transformations applied in sequence to produce a canonical key for exact matching.

**Transformation pipeline** (ordered):

| Step | Operation | Example | Notes |
|------|-----------|---------|-------|
| 1 | Trim whitespace | `" User Model "` -> `"User Model"` | Always first |
| 2 | Unicode normalization (NFKC) | Fullwidth -> ASCII, compatibility decomposition | Critical for Chinese/mixed content |
| 3 | Case folding (lowercase) | `"UserModel"` -> `"usermodel"` | Use `.to_lowercase()`; NOT `.to_ascii_lowercase()` which skips Unicode |
| 4 | Separator normalization | `"user-model"`, `"user model"` -> `"user_model"` | Replace spaces and hyphens with underscores |
| 5 | Collapse repeated separators | `"user__model"` -> `"user_model"` | Prevents injection-style variation |
| 6 | Strip common suffixes | `"UserModelClass"` -> `"UserModel"` | Suffix list below |
| 7 | Strip trailing digits (optional) | `"user_model_v2"` -> `"user_model"` | Version suffixes |

**Suffix stripping rules** (deterministic, not regex):

Known suffixes to strip when they appear at the end of an identifier:
- English: `_table`, `_tbl`, `_entity`, `_model`, `_class`, `_type`, `_info`, `_data`, `_config`, `_obj`, `_mgr`, `_manager`, `_service`, `_handler`, `Impl`, `Class`, `Type`, `Model`, `Table`, `Entity`, `Info`, `Data`
- Do NOT strip if stripping would leave an empty string
- Only strip if the suffix is preceded by a separator or is in PascalCase boundary (e.g., `UserModel` -> `User`, strip `Model` suffix; but `Model` alone stays)

**Implementation approach**: A `CanonicalKey` struct that wraps a String and is produced by a pure function. The key is used for alias table lookups and duplicate detection, but the original `subject_text` is always preserved in the Observation record.

```rust
// Conceptual signature
fn canonical_key(raw: &str) -> String {
    let s = raw.trim();
    // NFKC normalization
    let s = unicode_normalize_nfkc(&s);
    // Lowercase (Unicode-aware)
    let s = s.to_lowercase();
    // Normalize separators
    let s = s.replace([' ', '-'], "_");
    // Collapse repeated underscores
    let s = collapse_repeated(&s, '_');
    // Strip known suffixes
    let s = strip_known_suffix(&s);
    s
}
```

#### 2. Unicode Normalization for Chinese/English Mixed Content

**Critical requirement**: The system handles Chinese and English entity names mixed together (e.g., "MASK 表", "POSMASK", "BOE 内网 DNS 排障").

Key considerations:
- Rust `str::to_lowercase()` is Unicode-aware and handles CJK characters correctly (they are already lowercase by definition)
- Unicode NFKC normalization is important for fullwidth/halfwidth character equivalence (e.g., fullwidth ASCII `Ａ` -> `A`)
- The `unicode-normalization` crate provides NFKC via `unicode_normalization::nfkc()` -- zero heavy dependencies
- Chinese characters do not have case, so case folding is a no-op for pure Chinese strings
- Mixed strings like "BOE 内网 DNS" become "boe 内网 dns" after lowercase -- the CJK portion is unchanged
- Separator normalization (space -> underscore) would make "boe 内网 dns" -> "boe_内网_dns" -- this is fine for canonical key purposes

**Crate**: `unicode-normalization` (MIT license, maintained by Rust Unicode group, no transitive dependencies beyond `smallvec`)

#### 3. Alias Table Lookup Strategy

The alias table lookup is a two-phase process:

**Phase A: Exact match via canonical key**
1. Compute `canonical_key(raw_entity)` for the incoming entity name
2. Query: `SELECT canonical_form FROM entity_alias WHERE alias_form = ?` with the canonical key
3. If found, use the `canonical_form` from the table
4. If not found, query: `SELECT canonical_form FROM entity_alias WHERE canonical_form = ?` with the canonical key
5. If not found, the canonical key IS the canonical form (new entity)

**Phase B: Fuzzy match (fallback, only when Phase A fails)**
1. Query all canonical forms from the entity's workspace
2. Compute string similarity between the incoming entity and each canonical form
3. If best match exceeds threshold (e.g., Levenshtein distance ratio > 0.8), return that canonical form
4. If no good match, create a new canonical form

**Performance**:
- Phase A: Single indexed SQLite query -- O(log n) with B-tree index, well under 0.1ms
- Phase B: Requires iteration over canonical forms -- O(n) but n is typically small per workspace (< 1000 entities)
- Total budget: < 1ms per entity is easily achievable with Phase A alone for the common case
- Phase B is a fallback that should rarely trigger if the deterministic normalization is well-tuned

#### 4. Alias Discovery (Automatic)

Aliases are discovered from two sources:

**Source 1: Co-occurrence in observations**
- When two different raw entity strings produce observations with the same predicate and overlapping object values across sessions, they are candidates for being aliases
- Increment `co_occurrence_count` when detected
- When `co_occurrence_count >= 3` and not yet confirmed, surface as an active learning question (Phase 4)

**Source 2: Canonical key collision**
- When two different raw strings produce the same canonical key, they are definitely the same entity
- Auto-insert into alias table with `confirmed = true`
- Example: "UserModel" and "user_model" both produce canonical key "usermodel" -> auto-alias

**Source 3: LLM-extracted type matching**
- The Observation Extractor already outputs `subject_type` (e.g., "database_table")
- When two entity names share the same `subject_type` AND appear in similar predicate contexts, they are alias candidates

### Rust Libraries for String Similarity / Fuzzy Matching

#### `strsim` (Recommended for MVP)

- **Crate**: `strsim` on crates.io
- **License**: MIT
- **Latest version**: 0.11.x (stable, well-maintained)
- **Size**: Minimal, no transitive dependencies
- **Algorithms provided**: Levenshtein distance, Damerau-Levenshtein distance, Jaro-Winkler similarity, Sorensen-Dice coefficient, Hamming distance, OSA distance
- **Performance**: Levenshtein distance on short strings (< 50 chars) is sub-microsecond
- **Unicode**: Handles Unicode correctly; Levenshtein operates on chars, not bytes
- **Relevance**: Perfect for entity name fuzzy matching where strings are typically short (5-50 chars)

```rust
use strsim::levenshtein;

fn similarity_ratio(a: &str, b: &str) -> f64 {
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 { return 1.0; }
    let dist = levenshtein(a, b);
    1.0 - (dist as f64 / max_len as f64)
}
```

**Why strsim for MVP**: Zero external dependencies, proven stability, covers the exact use case (short string similarity), sub-microsecond performance, Unicode-safe.

#### `rapidfuzz` (Alternative for Phase 4+)

- **Crate**: `rapidfuzz` on crates.io (Rust port of the Python rapidfuzz library)
- **License**: MIT
- **Algorithms**: Levenshtein, OSA, Damerau-Levenshtein, Jaro-Winkler, partial ratio, token sort ratio, token set ratio, extraction
- **Advantage**: More algorithms including `fuzz::ratio()` and `fuzz::partial_ratio()` which handle substring matching
- **Performance**: Optimized C implementation under the hood; very fast
- **Downside**: Heavier dependency than strsim (C FFI via cc crate)
- **When to use**: Phase 4 when entity normalization needs more sophisticated matching (partial ratio, token-based matching)

#### Other options (not recommended)

| Crate | Why Not |
|-------|---------|
| `fuzzy-matcher` | Unmaintained, uses `skim` scorer which is overkill |
| `regex-fuzzy` | Regex-based fuzzy matching is the wrong abstraction |
| Custom implementation | Unnecessary complexity when `strsim` covers the need |

### Regex vs Rule-Based for Suffix Stripping

**Verdict: Rule-based is better for this use case.**

**Reasons**:

1. **Predictability**: Suffix stripping must be deterministic and auditable. A fixed list of known suffixes with simple `str::ends_with()` checks is easier to reason about than regex patterns.

2. **Performance**: `str::ends_with()` on a list of 20 suffixes is faster than compiling and matching regex patterns, especially when called per entity. No regex compilation overhead.

3. **Chinese compatibility**: Regex character classes like `\w` have complex behavior with Unicode. A rule-based approach using `char::is_alphabetic()` or explicit character checks avoids subtle Unicode issues.

4. **Debuggability**: When a suffix is incorrectly stripped (or not stripped), it is trivial to find the rule in a static list. Regex patterns are harder to debug.

5. **Maintainability**: Adding a new suffix is a one-line change to a static array. Adding a new regex pattern requires understanding the interaction with existing patterns.

**Implementation pattern**:

```rust
const STRIPPABLE_SUFFIXES: &[&str] = &[
    "_table", "_tbl", "_entity", "_model", "_class",
    "_type", "_info", "_data", "_config", "_obj",
    "_mgr", "_manager", "_service", "_handler",
];

const PASCAL_SUFFIXES: &[&str] = &[
    "Impl", "Class", "Type", "Model", "Table",
    "Entity", "Info", "Data", "Object", "Service",
    "Manager", "Handler", "Config",
];

fn strip_known_suffix(s: &str) -> String {
    if s.is_empty() { return s.to_string(); }

    // Try underscore-separated suffixes (lowercase context)
    let lower = s.to_lowercase();
    for suffix in STRIPPABLE_SUFFIXES {
        if lower.ends_with(suffix) {
            let remaining = &s[..s.len() - suffix.len()];
            if !remaining.is_empty() {
                return remaining.to_string();
            }
        }
    }

    // Try PascalCase suffixes
    for suffix in PASCAL_SUFFIXES {
        if s.ends_with(suffix) {
            let remaining = &s[..s.len() - suffix.len()];
            if !remaining.is_empty() {
                return remaining.to_string();
            }
        }
    }

    s.to_string()
}
```

### Chinese/English Mixed Entity Handling

The system will encounter entity names like:
- "MASK 表" (English + Chinese)
- "POSMASK" (pure English)
- "用户模型" (pure Chinese)
- "BOE 内网 DNS 排障" (mixed phrase, likely as concept name not entity)
- "Spring Boot Mapper" (English multi-word)
- "user_model_v2" (English with version)

**Strategy per type**:

| Input Type | Example | Normalization | Notes |
|-----------|---------|---------------|-------|
| Pure English camelCase | `UserModel` | `usermodel` (lowercase) | Common in code |
| Pure English snake_case | `user_model` | `user_model` (already normalized) | Common in DB/code |
| Pure English with spaces | `User Model` | `user_model` | LLM output |
| Pure English abbreviation | `UM` | `um` | Keep as-is, too short to strip |
| Pure Chinese | `用户模型` | `用户模型` | No case folding effect; CJK chars are already lowercase |
| Mixed English+Chinese | `MASK 表` | `mask_表` | Lowercase English, spaces -> underscores |
| Mixed multi-word | `BOE 内网 DNS` | `boe_内网_dns` | Each token processed independently |

**Key insight**: For Chinese text, there is no case folding and no separator normalization needed within Chinese character sequences. The normalization primarily affects the ASCII/English portions. The canonical key preserves Chinese characters as-is.

### Performance Budget Analysis

Target: < 1ms per entity normalization.

| Operation | Estimated Time | Notes |
|-----------|---------------|-------|
| String transformations (trim, lowercase, replace) | ~1-5 microseconds | Pure string ops on short strings |
| Unicode NFKC normalization | ~5-10 microseconds | On short strings (20-50 chars) |
| SQLite alias table lookup (indexed) | ~50-100 microseconds | B-tree index lookup on SSD/in-memory |
| Fuzzy match fallback (if needed) | ~100-500 microseconds | strsim Levenshtein on ~100 candidates |
| **Total (happy path)** | **~60-120 microseconds** | Well under 1ms |
| **Total (with fuzzy fallback)** | **~200-600 microseconds** | Still under 1ms for reasonable entity counts |

### Integration Points in Codebase

The entity normalization layer needs to integrate at these points:

1. **`ObservationStore::insert()`** (`store/traits.rs:14`): Before inserting, normalize `subject_text` and `object_text`. Store original text in the observation record, but use normalized form for duplicate detection.

2. **`ObservationStore::check_duplicate()`** (`store/traits.rs:21`): Currently takes raw `subject`/`predicate`/`object`. Should normalize subject/object before comparison.

3. **`ObservationStore::find_by_entity()`** (`store/traits.rs:20`): Should resolve the input entity through the alias table first, then search by canonical form.

4. **`ConceptStore::find_by_entities()`** (`store/traits.rs:32`): Should normalize input entities before matching against concept's `related_entities_json`.

5. **`are_semantically_equivalent()`** (quality-control.md): Currently uses `str::contains()` for entity overlap. Should use canonical key comparison instead.

6. **Recall design** (`recall-design.md`): Entity recall channel extracts entities from queries and matches against concept entity lists. All entities should be normalized before matching.

### External References

- `unicode-normalization` crate: https://crates.io/crates/unicode-normalization -- NFKC normalization for fullwidth character handling
- `strsim` crate: https://crates.io/crates/strsim -- Levenshtein, Jaro-Winkler, Sorensen-Dice for short string similarity
- `rapidfuzz` crate: https://crates.io/crates/rapidfuzz -- More advanced fuzzy matching (Phase 4+)
- Unicode NFKC specification: https://unicode.org/reports/tr15/ -- Compatibility decomposition for normalization
- SQLite indexing performance: https://www.sqlite.org/queryplanner.html -- B-tree lookup is O(log N)

### Related Specs

- `.trellis/spec/memory-runtime/memory-model.md` -- Defines entity_alias table schema
- `.trellis/spec/memory-runtime/roadmap.md` -- Phase 1 task 1.3 (entity normalization), Phase 4 task 4.5 (alias active learning)
- `.trellis/spec/memory-runtime/quality-control.md` -- `are_semantically_equivalent()` needs canonical key comparison
- `.trellis/spec/memory-runtime/recall-design.md` -- Entity recall channel depends on entity matching

## Caveats / Not Found

- The `unicode-normalization` crate was evaluated based on known crate information, not live testing. It should be verified against the latest version for NFKC support.
- Fuzzy matching thresholds (Levenshtein ratio > 0.8) are suggested starting points. Actual thresholds should be calibrated against the test data from Phase 0 (task 0.1-0.4 in roadmap).
- Chinese abbreviation handling (e.g., "UM" for "User Model") cannot be solved by string normalization alone. These require the alias table and eventually active learning confirmation (Phase 4).
- The `entity/mod.rs` file is a placeholder with no implementation. All entity normalization code needs to be written.
- The `check_duplicate` method in `ObservationStore` currently takes raw strings. Its signature may need adjustment to accept canonical forms.
- No benchmark exists yet for entity normalization performance. The < 1ms budget should be validated with Criterion benchmarks once implemented.
- The suffix stripping list should be configurable (workspace-specific) rather than hard-coded, but MVP can start with a static list.
