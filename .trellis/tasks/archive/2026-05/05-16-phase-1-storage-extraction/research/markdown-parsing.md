# Research: Rust Markdown Parsing for Session Ingest

- **Query**: Rust libraries and patterns for parsing markdown session/conversation files into structured RawMemory records
- **Scope**: Mixed (internal codebase analysis + external library research)
- **Date**: 2026-05-16

## Findings

### 1. Rust Markdown Parsing Libraries

Three main options exist in the Rust ecosystem:

#### pulldown-cmark (v0.13.3)

| Attribute | Value |
|---|---|
| Downloads | 93.8M total, 27.3M recent |
| Repository | https://github.com/raphlinus/pulldown-cmark |
| Approach | Pull parser (iterator-based) for CommonMark |
| Zero-copy | Yes (borrows from input) |
| GFM support | Limited (tables via feature flag) |
| AST output | No built-in AST; event stream only |
| License | MIT |

**API pattern** - Event stream iterator:

```rust
use pulldown_cmark::{Parser, Event, Tag, Options};

fn parse_markdown(input: &str) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);

    for (event, range) in Parser::new_ext(input, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                println!("Heading level {:?}", level);
            }
            Event::Start(Tag::Paragraph) => {
                // collect text until Event::End(Tag::Paragraph)
            }
            Event::Text(text) => {
                println!("Text at {:?}: {}", range, text);
            }
            Event::Code(code) => {
                println!("Inline code: {}", code);
            }
            _ => {}
        }
    }
}
```

**Key trait**: Returns an iterator of `Event<'a>` where each event is `Start(tag)`, `End(tag)`, `Text(str)`, `Code(str)`, `Html(str)`, etc. The `into_offset_iter()` variant provides byte ranges into the source, which is critical for mapping parsed content back to source locations.

**Strengths**:
- Most popular Rust markdown parser by far (93M+ downloads)
- Zero-allocation, zero-copy design (borrows from input string)
- `into_offset_iter()` gives byte offsets for source mapping
- Lightweight dependency tree
- Battle-tested (used by mdbook, docs.rs, Zola, etc.)

**Weaknesses**:
- No built-in AST -- caller must build their own tree from events
- GFM support is partial (tables work, but no autolinks, strikethrough, task lists by default)
- Does not produce a DOM; event stream requires manual state management for nested structures

#### comrak (v0.52.0)

| Attribute | Value |
|---|---|
| Downloads | 5.0M total, 1.7M recent |
| Repository | https://github.com/kivikakk/comrak |
| Approach | AST-based, 100% CommonMark + GFM compatible |
| Zero-copy | No (allocates AST nodes) |
| GFM support | Full (tables, strikethrough, task lists, autolinks) |
| AST output | Yes (concrete AST with node types) |
| License | BSD-2-Clause |

**API pattern** - AST traversal:

```rust
use comrak::{Arena, Options, parse_document, nodes::NodeValue};

fn parse_markdown(input: &str) {
    let arena = Arena::new();
    let opts = Options::default();

    let root = parse_document(&arena, input, &opts);

    fn walk_node(node: &comrak::nodes::AstNode, depth: usize) {
        match &node.data.borrow().value {
            NodeValue::Heading(heading) => {
                println!("{}Heading level {}", "  ".repeat(depth), heading.level);
            }
            NodeValue::Paragraph => {
                println!("{}Paragraph", "  ".repeat(depth));
            }
            NodeValue::Text(text) => {
                println!("{}Text: {}", "  ".repeat(depth), text);
            }
            NodeValue::CodeBlock(code) => {
                println!("{}Code block ({}): {}",
                    "  ".repeat(depth), code.info, code.literal);
            }
            _ => {}
        }
        for child in node.children() {
            walk_node(child, depth + 1);
        }
    }

    walk_node(root, 0);
}
```

**Key trait**: Uses an arena allocator (`Arena`) for zero-cost node allocation. Returns a tree of `AstNode` that can be traversed parent-to-children. Each node has `sourcepos` (line/column) for source mapping.

**Strengths**:
- Full GFM compatibility (this matters for our Trellis journals which use tables, bold, code spans)
- Built-in AST with parent-child traversal
- Source position tracking on each node
- Arena allocation pattern is efficient for short-lived parses
- Used by GitHub's own Rust tooling

**Weaknesses**:
- Allocates AST nodes (not zero-copy)
- Slower compile times than pulldown-cmark
- Heavier dependency tree

#### markdown (v1.0.0)

| Attribute | Value |
|---|---|
| Downloads | 6.5M total, 1.4M recent |
| Repository | https://github.com/wooorm/markdown-rs |
| Approach | CommonMark compliant with ASTs and extensions |
| Zero-copy | No |
| GFM support | Via extensions |
| AST output | Yes |
| License | MIT |

**Strengths**:
- Clean AST output
- Extension system for custom syntax

**Weaknesses**:
- From the JS ecosystem (wooorm), less idiomatic Rust API
- Smaller Rust community adoption
- Fewer reverse dependencies in the Rust ecosystem

### 2. Comparison Matrix for Our Use Case

| Criterion | pulldown-cmark | comrak | markdown |
|---|---|---|---|
| Popularity (downloads) | 93.8M | 5.0M | 6.5M |
| API style | Event stream | AST tree | AST tree |
| GFM tables | Feature flag | Built-in | Extension |
| Source position | offset_iter | sourcepos on nodes | Via AST |
| Zero-copy | Yes | No (arena) | No |
| Dependency weight | Light | Medium | Medium |
| Best for our case | Manual tree building | Direct tree traversal | Direct tree traversal |

### 3. Recommendation

**pulldown-cmark** is the recommended choice for this project. Reasons:

1. **Our markdown is semi-structured, not complex**: Trellis journals and Claude conversations use a predictable pattern of headings, bold key-value pairs, and paragraphs. We do not need a full AST -- we need to detect section boundaries (headings) and collect text within sections.

2. **Source mapping is critical**: The `into_offset_iter()` API provides byte ranges into the original source, which we need for `source_ref` fields in `RawMemory` (referencing line numbers or byte ranges in the original file).

3. **Minimal dependency weight**: The project already has a large dependency tree (candle-transformers, linfa, rusqlite). Adding a lightweight parser avoids compounding compile times.

4. **Pattern match for our use case**: Our parsing approach is a single-pass state machine over heading-delimited sections. This maps naturally to pulldown-cmark's event stream, where we track "current heading level" and "current section content" as we iterate events.

5. **Ecosystem ubiquity**: 93M+ downloads, used by mdbook (the standard Rust documentation tool). Any Rust developer will recognize the API.

The one downside is that we need to build our own section-extraction logic from the event stream. But since our formats are well-defined and simple, this is a small amount of code (estimated ~150-200 lines per parser).

### 4. Existing Format Analysis

#### Trellis Journal Format

Found at `.trellis/workspace/shaobolua/journal-1.md`. Structure:

```markdown
# Journal - shaobolua (Part 1)

> AI development session journal
> Started: 2026-05-10

---

## Session 1: Migrate Memory Runtime spec from Python to Rust

**Date**: 2026-05-12
**Task**: Migrate Memory Runtime spec from Python to Rust
**Branch**: `main`

### Summary

Migrated all 6 spec files + plan file from Python to Rust tech stack...

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `334ecc4` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete

---

## Session 2: Architecture design and Rust workspace scaffold

**Date**: 2026-05-16
...
```

**Key structural patterns**:

1. File-level: `# Title` + blockquote metadata (`> Started: YYYY-MM-DD`)
2. Session delimiter: `## Session N: <title>`
3. Session metadata: `**Key**: Value` pairs (Date, Task, Branch)
4. Session sections: `### Heading` for Summary, Main Changes, Git Commits, Testing, Status, Next Steps
5. Tables: `| col | col |` for git commits
6. Checkboxes: `- [OK]` or `- [ ]` for testing items
7. Session separator: `---` horizontal rule between sessions

**Mapping to RawMemory**:

| Trellis field | RawMemory field |
|---|---|
| Session heading (`## Session N`) | `session_id` |
| Date (`**Date**: ...`) | `created_at` |
| Workspace (from filename/path) | `workspace_id` |
| Section content | `content` |
| File path | `source_ref` |
| Fixed value | `source_type` = `SourceType::TrellisJournal` |
| Section role (Summary/Changes/etc) | `role` (custom: "summary", "changes", "commits", etc.) |

#### Claude Code Conversation Format

Claude Code conversations in markdown follow a pattern of alternating user/assistant turns:

```markdown
# Conversation Title

**User**: First message content here...

**Assistant**: Response content here...

**User**: Second message...

**Assistant**: Second response...
```

Or with tool use blocks:

```markdown
**Assistant**: Let me look at that file.

<tool_call name="Read">
<parameter name="file_path">/path/to/file</parameter>
</tool_call**

<tool_result>
file contents here
</tool_result>
```

**Mapping to RawMemory**:

| Conversation field | RawMemory field |
|---|---|
| Conversation file | `session_id` (derived from filename) |
| `**User**:` blocks | `role` = "user" |
| `**Assistant**:` blocks | `role` = "assistant" |
| Message content | `content` |
| File path | `source_ref` |
| Fixed value | `source_type` = `SourceType::SessionFile` |

### 5. Parsing Strategy

#### Approach: State Machine over pulldown-cmark Events

For both formats, the parsing strategy is a state machine that tracks:

1. **Current section context** (which heading level we're under)
2. **Accumulated text content** for the current section
3. **Metadata extraction** from bold key-value pairs

```rust
use pulldown_cmark::{Parser, Event, Tag, Options};

/// State for parsing a Trellis journal
struct JournalParserState {
    current_session_id: Option<String>,
    current_date: Option<String>,
    current_heading_level: u8,
    current_section: String,  // "summary", "changes", "commits", etc.
    content_buffer: String,
}

/// Parse a Trellis journal into RawMemory records
fn parse_trellis_journal(input: &str, workspace_id: &str, file_path: &str) -> Vec<RawMemory> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);

    let mut memories = Vec::new();
    let mut state = JournalParserState {
        current_session_id: None,
        current_date: None,
        current_heading_level: 0,
        current_section: String::new(),
        content_buffer: String::new(),
    };

    for event in Parser::new_ext(input, opts) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Flush accumulated content as a RawMemory record
                if !state.content_buffer.trim().is_empty() {
                    if let Some(session_id) = &state.current_session_id {
                        memories.push(RawMemory {
                            memory_id: uuid::Uuid::new_v4().to_string(),
                            workspace_id: workspace_id.to_string(),
                            session_id: session_id.clone(),
                            role: state.current_section.clone(),
                            content: state.content_buffer.trim().to_string(),
                            source_type: SourceType::TrellisJournal,
                            source_ref: file_path.to_string(),
                            created_at: state.current_date.clone()
                                .unwrap_or_default(),
                        });
                    }
                    state.content_buffer.clear();
                }

                state.current_heading_level = level as u8;

                if level == 2 {
                    // ## Session N: Title  -- new session boundary
                    // Session title will come as the next Text event
                } else if level == 3 {
                    // ### Summary, ### Main Changes, etc.
                    // Section name will come as the next Text event
                }
            }
            Event::Text(text) => {
                // Depending on context, this is either:
                // - A heading label (under a Heading Start)
                // - Body text (under a Paragraph)
                // - Metadata value (after a **Key**: pattern)
                state.content_buffer.push_str(&text);
            }
            Event::Code(code) => {
                // Inline code -- preserve in content
                state.content_buffer.push('`');
                state.content_buffer.push_str(&code);
                state.content_buffer.push('`');
            }
            Event::Start(Tag::Strong) => {
                // Bold text: could be metadata key (**Date**: ...)
                // Need to track whether next text is a metadata key
            }
            Event::End(Tag::Paragraph) => {
                state.content_buffer.push('\n');
            }
            _ => {}
        }
    }

    // Flush final section
    // ... (same flush logic as above)

    memories
}
```

#### Approach: Metadata Extraction via Regex on Sections

For the `**Key**: Value` pattern in journals, use the `regex` crate on the accumulated section text:

```rust
use regex::Regex;

fn extract_metadata(section_text: &str) -> HashMap<String, String> {
    let re = Regex::new(r"\*\*(\w+)\*\*:\s*(.+?)(?:\n|$)").unwrap();
    re.captures_iter(section_text)
        .filter_map(|cap| {
            Some((cap[1].to_string(), cap[2].trim().to_string()))
        })
        .collect()
}
```

#### Approach: Trait-Based Parser Abstraction

Each format gets its own parser implementing a common trait:

```rust
pub trait SessionParser: Send + Sync {
    /// Parse a file into RawMemory records.
    fn parse(&self, content: &str, workspace_id: &str, source_ref: &str)
        -> Result<Vec<RawMemory>, MemoryError>;

    /// Detect if this parser can handle the given content.
    fn can_parse(&self, content: &str, filename: &str) -> bool;
}

pub struct TrellisJournalParser;
pub struct ClaudeConversationParser;
pub struct JsonSessionParser;
```

The `can_parse` method enables automatic format detection:

```rust
impl SessionParser for TrellisJournalParser {
    fn can_parse(&self, content: &str, filename: &str) -> bool {
        // Heuristic: starts with "# Journal" or path contains ".trellis/workspace/"
        content.starts_with("# Journal") || filename.contains(".trellis/workspace/")
    }
    // ...
}
```

### 6. Files Found

| File Path | Description |
|---|---|
| `.trellis/workspace/shaobolua/journal-1.md` | Example Trellis journal with 2 sessions (73 lines) |
| `.trellis/workspace/shaobolua/index.md` | Workspace index with session history table |
| `.trellis/tasks/05-16-phase-1-storage-extraction/prd.md` | Phase 1 PRD with session ingest requirements |
| `.trellis/spec/memory-runtime/memory-model.md` | RawMemory schema and source_type enum |
| `.trellis/spec/memory-runtime/roadmap.md` | Roadmap with task 1.2 (session ingest) definition |
| `crates/memory-runtime/src/models/raw_memory.rs` | RawMemory struct with SourceType enum |
| `crates/memory-runtime/src/store/traits.rs` | RawMemoryStore trait definition |
| `crates/memory-runtime/src/pipeline/mod.rs` | Empty pipeline module (placeholder) |
| `crates/memory-runtime/src/lib.rs` | Crate root with module declarations |
| `Cargo.toml` | Workspace Cargo.toml with dependency list |
| `crates/memory-runtime/Cargo.toml` | Memory-runtime crate dependencies |

### 7. Related Specs

- `.trellis/spec/memory-runtime/memory-model.md` -- Defines RawMemory schema and SourceType enum (TrellisJournal, SessionFile, etc.)
- `.trellis/spec/memory-runtime/roadmap.md` -- Phase 1 task 1.2 requires session ingest supporting at least 2 formats
- `.trellis/tasks/05-16-phase-1-storage-extraction/prd.md` -- Open Question: "Session ingest 的 markdown 格式具体是什么样的？"

### 8. Dependency to Add

For `crates/memory-runtime/Cargo.toml`:

```toml
pulldown-cmark = "0.13"
regex = "1"
```

Or at workspace level in `Cargo.toml`:

```toml
[workspace.dependencies]
pulldown-cmark = "0.13"
regex = "1"
```

### 9. Caveats / Not Found

- **No example Claude Code conversation exports found** in the current project. The journal format is the only concrete markdown example available. Claude conversation export format was described from documentation knowledge, not from a real file in this repo.
- **Exact conversation markdown format may vary** depending on how Claude Code exports are generated. The format described above is based on common patterns but should be validated against actual exports.
- **The `regex` crate** is suggested for metadata extraction (`**Key**: Value` patterns). An alternative is to parse these inline during pulldown-cmark event iteration by tracking Strong+Text+Colon sequences, but regex is simpler and more robust for this semi-structured pattern.
- **pulldown-cmark's ENABLE_TABLES feature flag** is needed to correctly parse the Git Commits table in Trellis journals. Without it, table rows would be parsed as loose text.
- **Source position tracking**: When using `Parser::new_ext()`, source positions require `into_offset_iter()` instead of plain iteration. This adds a small amount of complexity but is necessary for `source_ref` line number tracking.
