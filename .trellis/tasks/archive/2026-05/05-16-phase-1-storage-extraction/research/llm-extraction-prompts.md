# Research: LLM Prompt Design for Structured Observation Extraction

- **Query**: How to design LLM prompts that extract (subject, predicate, object, evidence_text, source_type) tuples from conversations for a memory system
- **Scope**: Mixed (internal project specs + external domain knowledge)
- **Date**: 2026-05-16

## Findings

### 1. Knowledge Graph Construction Prompts (Triple Extraction)

Knowledge graph construction from unstructured text using LLMs typically follows these patterns:

#### Standard Triples Pattern

The most common approach is instructing the LLM to extract (subject, predicate, object) triples with a fixed schema. Key design elements:

- **Role definition**: Assign the LLM the role of "information extraction expert" or "knowledge graph builder"
- **Schema specification**: Provide an explicit schema with field names, types, allowed values, and descriptions
- **Entity typing**: Include `subject_type` and `object_type` fields to categorize entities (e.g., "database_table", "service", "framework", "preference")
- **Negation predicates**: Use explicit negation predicates (e.g., "not_has_field", "does_not_use") rather than boolean flags, because negation is first-class information in the knowledge graph
- **Confidence scoring**: Ask the LLM to self-assess confidence per extraction, though calibration is poor without adversarial validation

#### OpenIE vs ClosedIE

- **OpenIE** (Open Information Extraction): Let the LLM freely choose predicates. More flexible but harder to normalize downstream. Best when the domain is unknown.
- **ClosedIE** (Closed Information Extraction): Provide a controlled predicate vocabulary. More consistent, easier to merge/deduplicate. Best when the domain is well-known.

**Recommendation for this project**: Use a **hybrid approach** -- provide a suggested predicate vocabulary as guidance but allow the LLM to create new predicates when needed. The entity normalization layer (Phase 1, task 1.3) will handle consistency downstream.

#### Predicate Vocabulary (Derived from Project Specs)

Based on the project's memory types and observation fields, a suggested predicate vocabulary:

| Category | Predicates | Example |
|----------|-----------|---------|
| Structure | `has_field`, `not_has_field`, `has_table`, `has_module` | POSMASK not_has_field 机器 |
| Dependency | `depends_on`, `uses`, `called_by` | auth middleware uses SQLite |
| Preference | `prefers`, `dislikes`, `avoids` | User prefers TDD approach |
| Cause | `is_root_cause`, `causes`, `fixes` | Missing migration is_root_cause startup failure |
| Status | `has_status`, `is_state_of` | Task has_status blocked |
| Knowledge | `has_value`, `is_type`, `belongs_to` | email field is_type VARCHAR(255) |
| Negation | `not_has_field`, `does_not_use`, `is_not` | POSMASK is_not root table |

### 2. Prompt Design for Reliable JSON Output

#### Core Techniques

1. **JSON Schema in Prompt**: Include the exact JSON schema the LLM must follow. Specify required vs optional fields, allowed enum values, and field descriptions.

2. **Output Format Instruction**: End the prompt with explicit output format instruction, e.g., "Output ONLY valid JSON matching the schema above. No markdown, no explanation."

3. **Response Format Enforcement**:
   - For Claude API: Use `response_format` with `type: "json"` or prefill the assistant message with `{`
   - For OpenAI API: Use `response_format: { type: "json_object" }` (requires "JSON" in the prompt)
   - In Rust: The `LlmProvider::complete_json<T>()` method (at `crates/memory-runtime/src/llm/traits.rs:10-16`) already handles JSON deserialization with `serde_json::from_str()` and returns `MemoryError::LlmInvalidJson` on failure

4. **Structured Output Prompt Template** (adapted for this project):

```
You are an observation extraction system. Your task is to extract structured
knowledge observations from conversation messages.

## Input
- Conversation messages between a user and a coding assistant
- Each message has a role: "user" or "assistant"
- Messages may contain Chinese and English mixed content

## Extraction Rules
1. Extract facts, preferences, technical decisions, and negative information
2. Each observation MUST have a clear evidence_text (verbatim quote from the input)
3. Distinguish user-stated facts from assistant guesses
4. Preserve negative information ("X does NOT have Y") as separate observations
5. Do NOT extract greetings, pleasantries, or meta-discussion
6. Do NOT speculate beyond what the evidence explicitly supports

## Output Schema
Output a JSON object with this structure:
{
  "observations": [
    {
      "subject_text": "entity name (e.g., 'POSMASK table', 'auth middleware')",
      "subject_type": "entity type or null (e.g., 'database_table', 'service', 'framework')",
      "predicate": "relationship (e.g., 'has_field', 'uses', 'prefers', 'is_root_cause')",
      "object_text": "target entity or value (e.g., 'machine_code field', 'SQLite')",
      "object_type": "target type or null",
      "evidence_text": "exact verbatim quote supporting this observation",
      "source_type": "one of: user_message | user_confirm | user_negation | assistant_guess | file_evidence",
      "confidence": 0.0-1.0
    }
  ]
}

## Source Type Rules
- user_message: The user stated this directly
- user_confirm: The user confirmed something the assistant suggested
- user_negation: The user denied or corrected something
- assistant_guess: The assistant inferred or speculated this
- file_evidence: Derived from file/project content

## Output ONLY valid JSON. No markdown fences, no explanation.
```

5. **JSON Repair Strategy**: The `LlmProvider::complete_json<T>()` method currently fails on `serde_json::from_str()` errors. A production extraction prompt should include a JSON repair fallback:
   - Strip markdown code fences (```json ... ```)
   - Strip leading/trailing whitespace and newlines
   - Attempt to parse after repair before returning `LlmInvalidJson`

#### Key Insight from Project Specs

The project's `concept-growth.md` (line 120-123) specifies these constraints for extraction:
- "Only extract information with clear evidence"
- "Distinguish user-provided vs user-confirmed vs user-negated vs assistant-speculated"
- "Negative information MUST be preserved"
- "Output as structured JSON"

### 3. How Other Memory Systems Handle Extraction

#### mem0 (formerly EmbedChain)

mem0 extracts memories from conversations using a two-step process:

1. **Extraction prompt**: Given a conversation, extract discrete "memories" as natural language statements (not triples). Each memory is a self-contained fact like "User prefers dark mode" or "The project uses PostgreSQL."

2. **Deduplication**: When a new memory is extracted, mem0 embeds it and checks for semantic similarity with existing memories. If similar enough (cosine similarity > threshold), it updates the existing memory rather than creating a duplicate.

3. **Storage**: Memories are stored as (id, text, embedding, metadata). No subject/predicate/object decomposition -- mem0 works at the statement level, not the triple level.

**Key difference from this project**: mem0 does not decompose into triples. It stores whole statements. This project's approach (subject/predicate/object) provides more structure for concept growth and clustering.

#### MemGPT / Letta

MemGPT (now Letta) uses a different memory architecture:

1. **Memory tiers**: Core memory (always in context), archival memory (searchable via embeddings), recall memory (conversation history).

2. **Extraction**: MemGPT does NOT use a separate extraction step. Instead, the LLM agent itself manages memory via tool calls (`memorize`, `recall`, `archival_insert`). The agent decides what to store.

3. **Structured format**: Core memory is stored as a JSON block that the agent can modify in-place. Archival memory is unstructured text with embeddings.

4. **No triple decomposition**: Like mem0, Letta stores statements, not triples.

**Key difference from this project**: This project uses a separate offline extraction pipeline rather than relying on the agent to manage memory inline.

#### Microsoft GraphRAG

GraphRAG extracts entities and relationships from text into a knowledge graph:

1. **Entity extraction**: Identify named entities with types and descriptions
2. **Relationship extraction**: Extract relationships as (source, target, description, strength)
3. **Community detection**: Cluster entities into hierarchical communities
4. **Summarization**: Generate community-level summaries for retrieval

**Relevant pattern**: GraphRAG's relationship extraction is closest to this project's observation extraction. It uses a structured prompt that asks for (source_entity, target_entity, relationship_description, relationship_strength).

#### Key Takeaway for This Project

This project's triple-based extraction (subject/predicate/object) is more structured than mem0 or Letta but less structured than a full knowledge graph (like GraphRAG's entity-relationship model). This is a good middle ground:
- Structured enough for clustering and concept growth
- Simple enough for reliable LLM extraction
- Flexible enough to handle diverse technical conversations

### 4. Few-Shot Examples in Extraction Prompts

#### Best Practices

1. **3-5 examples are optimal**: Research shows 3-5 diverse examples provide the best balance of guidance and flexibility. Too many examples cause the LLM to overfit to the examples.

2. **Cover edge cases in examples**: Include examples for:
   - Simple factual statement (straightforward extraction)
   - Negative statement ("X does NOT have Y")
   - User preference extraction
   - Assistant speculation (must be labeled as such)
   - Mixed Chinese/English content
   - Multi-sentence input where the evidence spans multiple sentences
   - Input with NO extractable observations (empty result)

3. **Example diversity**: Vary entity types, predicates, and source types across examples.

4. **Show the reasoning**: For each example, show a brief chain-of-thought before the JSON output. This improves extraction accuracy. However, the reasoning should be stripped before JSON parsing.

5. **Order examples from simple to complex**: Start with the most straightforward case, then progressively introduce complexity.

#### Recommended Few-Shot Examples for This Project

**Example 1 -- Simple user fact (Chinese)**:
```
Input messages:
[user]: POSMASK 表没有机器字段

Output:
{
  "observations": [
    {
      "subject_text": "POSMASK",
      "subject_type": "database_table",
      "predicate": "not_has_field",
      "object_text": "机器字段",
      "object_type": "field",
      "evidence_text": "POSMASK 表没有机器字段",
      "source_type": "user_message",
      "confidence": 0.9
    }
  ]
}
```

**Example 2 -- User preference (mixed Chinese/English)**:
```
Input messages:
[assistant]: Should we use SQLite or PostgreSQL for the store layer?
[user]: 用 SQLite 吧，这个项目不需要分布式

Output:
{
  "observations": [
    {
      "subject_text": "store layer",
      "subject_type": "component",
      "predicate": "uses",
      "object_text": "SQLite",
      "object_type": "database",
      "evidence_text": "用 SQLite 吧，这个项目不需要分布式",
      "source_type": "user_message",
      "confidence": 0.9
    },
    {
      "subject_text": "project",
      "subject_type": null,
      "predicate": "does_not_require",
      "object_text": "distributed architecture",
      "object_type": "architecture",
      "evidence_text": "这个项目不需要分布式",
      "source_type": "user_message",
      "confidence": 0.85
    }
  ]
}
```

**Example 3 -- Assistant speculation**:
```
Input messages:
[assistant]: Based on the error log, the root cause is likely a missing database migration.
[user]: 嗯继续看

Output:
{
  "observations": [
    {
      "subject_text": "database migration",
      "subject_type": "component",
      "predicate": "is_potential_root_cause",
      "object_text": "error in log",
      "object_type": "error",
      "evidence_text": "the root cause is likely a missing database migration",
      "source_type": "assistant_guess",
      "confidence": 0.5
    }
  ]
}
```

Note: "嗯继续看" is too vague to extract as user confirmation. The assistant's statement remains `assistant_guess` rather than being upgraded to `user_confirm`.

**Example 4 -- User negation/correction**:
```
Input messages:
[assistant]: So POSMASK has a machine_code field?
[user]: 不对，POSMASK 没有机器字段，那是 MASKSPECNAME 的字段

Output:
{
  "observations": [
    {
      "subject_text": "POSMASK",
      "subject_type": "database_table",
      "predicate": "not_has_field",
      "object_text": "机器字段",
      "object_type": "field",
      "evidence_text": "POSMASK 没有机器字段",
      "source_type": "user_negation",
      "confidence": 0.95
    },
    {
      "subject_text": "MASKSPECNAME",
      "subject_type": "database_table",
      "predicate": "has_field",
      "object_text": "机器字段",
      "object_type": "field",
      "evidence_text": "那是 MASKSPECNAME 的字段",
      "source_type": "user_message",
      "confidence": 0.9
    }
  ]
}
```

**Example 5 -- No extractable content**:
```
Input messages:
[user]: 好的，谢谢
[assistant]: 不客气，有问题随时问。

Output:
{
  "observations": []
}
```

### 5. Handling Chinese + English Mixed Content

#### Challenges

1. **Entity boundary detection**: Chinese has no spaces between words, making it harder to identify entity boundaries in mixed text like "POSMASK表没有机器字段" (no space between POSMASK and 表).

2. **Code-switching**: Users frequently switch between Chinese and English mid-sentence: "用 SQLite 吧" (Chinese + English), "这个 project 的 auth module" (mixed).

3. **Evidence text fidelity**: The `evidence_text` field must preserve the original mixed-language text verbatim. No translation.

4. **Predicate language**: Should predicates be in English (machine-readable) or Chinese (matching user expression)?
   - **Decision**: Predicates should be in English for consistency and downstream processing. The entity names (subject/object) should preserve the user's exact expression.

5. **Entity normalization**: "POSMASK table", "POSMASK表", "POSMASK" are the same entity. The entity normalization layer (Phase 1, task 1.3) must handle this.

#### Prompt Techniques for Mixed-Language Extraction

1. **Language-aware instructions**: Explicitly tell the LLM that input may contain mixed Chinese/English and that it should handle both naturally.

2. **Preserve original language**: Instruct the LLM to keep `subject_text`, `object_text`, and `evidence_text` in the original language. Never translate.

3. **English predicates**: All `predicate` values should be in English for normalization. Provide the predicate vocabulary in English.

4. **Mixed-language examples**: Include few-shot examples that contain both Chinese and English, as shown in the examples above.

5. **Bilingual entity types**: `subject_type` and `object_type` should use English type labels (database_table, service, etc.) since these are system-internal, not user-facing.

6. **Embedding model consideration**: The project uses `BAAI/bge-small-zh-v1.5` which is optimized for Chinese text but handles English well. The embedding input is `subject_text + " " + predicate + " " + object_text` which will naturally contain mixed language. This model handles mixed input well.

#### Specific Prompt Instruction for Mixed Content

Add to the system prompt:

```
## Language Handling
- Input may contain mixed Chinese and English text
- Preserve the original language in subject_text, object_text, and evidence_text
- Do NOT translate any text
- Predicates must always be in English
- Entity types (subject_type, object_type) must always be in English
- Entity names should preserve the user's exact expression (e.g., "POSMASK表" not "POSMASK table")
```

### Files Found

| File Path | Description |
|---|---|
| `crates/memory-runtime/src/llm/traits.rs` | LlmProvider trait with `complete()` and `complete_json<T>()` methods |
| `crates/memory-runtime/src/models/observation.rs` | Observation struct with all target fields |
| `crates/memory-runtime/src/models/raw_memory.rs` | RawMemory struct (input to extraction) |
| `crates/memory-runtime/src/models/evidence.rs` | Evidence struct used in evidence_json |
| `crates/memory-runtime/src/models/status.rs` | ObservationStatus and ObservationSourceType enums |
| `crates/memory-runtime/src/models/recall.rs` | MemoryContext with `to_prompt()` method |
| `crates/memory-runtime/src/pipeline/mod.rs` | Pipeline module (placeholder, not yet implemented) |
| `crates/memory-test-fixtures/src/mock_llm.rs` | MockLlmProvider for testing |
| `crates/memory-runtime/src/entity/mod.rs` | Entity normalization module (placeholder) |
| `crates/memory-runtime/src/error.rs` | MemoryError enum including LlmInvalidJson variant |
| `.trellis/spec/memory-runtime/concept-growth.md` | Full pipeline spec including Stage 2: Extract |
| `.trellis/spec/memory-runtime/principles.md` | Design decisions including "No Separate NER Pipeline" |
| `.trellis/spec/memory-runtime/quality-control.md` | Adversarial validation spec with 5 criteria |
| `.trellis/spec/memory-runtime/memory-model.md` | Observation table schema with all fields |

### Code Patterns

#### Current LLM Provider Interface (llm/traits.rs)

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> MemoryResult<String>;
    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str, system: Option<&str>) -> MemoryResult<T>;
    async fn health_check(&self) -> MemoryResult<bool>;
    fn name(&self) -> &str;
}
```

Key pattern: The `complete_json<T>()` method handles raw response -> JSON deserialization. The `system` parameter is `Option<&str>`, allowing a system prompt to be passed separately from the user prompt.

#### Observation Model (models/observation.rs)

The target extraction output matches the Observation struct:
- `subject_text: String` (NOT NULL)
- `subject_type: Option<String>`
- `predicate: String` (NOT NULL)
- `object_text: Option<String>`
- `evidence_text: Option<String>`
- `source_type: ObservationSourceType` (enum: UserMessage, UserConfirm, UserNegation, AssistantGuess, FileEvidence)
- `confidence: f64` (default 0.5)

#### MemoryError for JSON (error.rs)

```rust
LlmInvalidJson {
    source: serde_json::Error,
    raw: String,  // raw response preserved for debugging
}
```

This captures the raw LLM output when JSON parsing fails, enabling retry/repair strategies.

### External References

- **mem0** (github.com/mem0ai/mem0) -- Extracts memories as natural language statements, stores with embeddings. No triple decomposition. Uses semantic similarity for deduplication.
- **Letta/MemGPT** (github.com/letta-ai/letta) -- LLM agent manages memory via tool calls. No separate extraction pipeline. Three-tier memory (core, archival, recall).
- **Microsoft GraphRAG** (github.com/microsoft/graphrag) -- Extracts (entity, relationship, entity) triples for knowledge graph construction. Uses community detection and hierarchical summarization. Closest architectural analogy to this project's observation extraction.
- **BAAI/bge-small-zh-v1.5** -- Chinese-optimized embedding model (512-dim, ~90MB). Handles mixed Chinese/English input well. Chosen for this project's embedding layer.

### Related Specs

- `.trellis/spec/memory-runtime/concept-growth.md` -- Stage 2: Extract defines the extraction pipeline signature, output fields, and constraints
- `.trellis/spec/memory-runtime/principles.md` -- "No Separate NER Pipeline" decision explains why LLM handles entity extraction; "Adversarial Validation" decision defines quality gate
- `.trellis/spec/memory-runtime/quality-control.md` -- Adversarial validation with 5 criteria (evidence clarity, over-speculation, triple accuracy, confidence calibration, source attribution)
- `.trellis/spec/memory-runtime/roadmap.md` -- Phase 1 task 1.4 defines "LLM Observation Extractor" deliverable

### Extraction Prompt Architecture Summary

Based on the project specs and domain knowledge, the extraction prompt should be structured as:

```
[System Prompt]
  - Role definition (observation extraction expert)
  - Language handling rules (Chinese + English mixed)
  - Extraction constraints (evidence required, no speculation, preserve negation)
  - Source type classification rules
  - JSON schema specification
  - Few-shot examples (5 examples covering edge cases)

[User Prompt]
  - RawMemory records formatted as conversation turns
  - Each turn prefixed with [user] or [assistant]
  - Request for extraction

[Expected Output]
  - JSON array of observations matching the Observation struct
```

The `LlmProvider` trait already supports this split via `complete(prompt, system)` where `system` contains the static extraction instructions and `prompt` contains the dynamic conversation content.

## Caveats / Not Found

- **No existing extraction prompt in codebase**: The `pipeline/mod.rs` is a placeholder. No extraction prompt templates exist yet in the codebase.
- **External search restricted**: Could not access GitHub or web search APIs during this research session. Findings about mem0, Letta, and GraphRAG are based on established domain knowledge rather than live code inspection.
- **JSON repair not implemented**: The current `complete_json<T>()` method fails on malformed JSON without repair attempts. A production extraction pipeline should add a JSON repair layer (strip markdown fences, handle trailing commas, etc.).
- **Entity normalization not implemented**: The `entity/mod.rs` is a placeholder. Extraction prompt quality depends heavily on consistent entity naming, which the normalization layer must provide.
- **Predicate vocabulary not finalized**: The suggested predicate vocabulary above is derived from the spec's examples but has not been validated against real session data. This should be refined during Phase 1 implementation with actual test sessions.
