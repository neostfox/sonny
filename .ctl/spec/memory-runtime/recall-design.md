# Memory Runtime — Recall Design

## Recall Goal

Recall is NOT "find similar text". Recall is:

> Find the concept matching the current problem, and expand the most useful context from that concept.

## Dual-Channel Recall

### Channel 1: Semantic Similarity (Embedding)

Embed the user query and search for nearest concept/concept_candidate vectors.

```rust
fn semantic_recall(
    query: &str,
    workspace_id: &str,
    top_k: usize,
) -> Result<Vec<(String, f64)>, MemoryError> {
    // Embed query, search vec_embedding for nearest concepts.
    // Returns Vec<(concept_id, similarity_score)>.
    let query_embedding = embed(query)?;
    let results = vector_search(&query_embedding, workspace_id, top_k)?;
    Ok(results.iter().map(|r| (r.source_id.clone(), r.distance)).collect())
}
```

**Weight**: 60% of final score.

### Channel 2: Entity Exact Match

Extract entities from the query using lightweight LLM call, then match against concept entity lists.

```rust
fn entity_recall(
    query: &str,
    workspace_id: &str,
) -> Result<Vec<(String, f64)>, MemoryError> {
    // Extract entities from query, match against concept.related_entities.
    // Returns Vec<(concept_id, overlap_ratio)>.
    let query_entities = llm_extract_entities(query)?;
    let concepts = get_concepts_by_entities(&query_entities, workspace_id)?;
    let mut results = Vec::new();
    for concept in &concepts {
        let overlap = concept.entities.intersection(&query_entities).count() as f64
            / concept.entities.len() as f64;
        results.push((concept.id.clone(), overlap));
    }
    Ok(results)
}
```

**Weight**: 40% of final score.

### Merged Score

```rust
fn recall(
    query: &str,
    workspace_id: &str,
    top_n: usize,
) -> Result<Vec<(String, f64)>, MemoryError> {
    // Dual-channel recall with merged scoring.
    let semantic_results = semantic_recall(query, workspace_id, 10)?;
    let entity_results = entity_recall(query, workspace_id)?;

    let mut scores: HashMap<String, f64> = HashMap::new();
    for (cid, sim) in &semantic_results {
        *scores.entry(cid.clone()).or_insert(0.0) += sim * 0.6;
    }
    for (cid, overlap) in &entity_results {
        *scores.entry(cid.clone()).or_insert(0.0) += overlap * 0.4;
    }

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Ok(ranked.into_iter().take(top_n).collect())
}
```

## Spreading Activation

When a concept is activated, partially activate related concepts through the concept network.

```rust
fn spread_activation(
    seed_concept_id: &str,
    concept_network: &ConceptGraph,
    max_depth: usize,
    decay_rate: f64,
) -> HashMap<String, f64> {
    // Spread activation from seed concept through network.
    // Returns {concept_id: activation_strength}.
    let mut activation: HashMap<String, f64> = HashMap::new();
    activation.insert(seed_concept_id.to_string(), 1.0);
    let mut frontier: HashSet<String> = [seed_concept_id.to_string()].into_iter().collect();

    for _ in 0..max_depth {
        let mut next_frontier: HashSet<String> = HashSet::new();
        for concept_id in &frontier {
            let current = *activation.get(concept_id).unwrap_or(&0.0);
            if current < 0.05 { continue; }
            for (neighbor_id, edge_strength) in concept_network.neighbors(concept_id) {
                let spread = current * edge_strength * decay_rate;
                let entry = activation.entry(neighbor_id.clone()).or_insert(0.0);
                *entry = (*entry).max(spread);
                if spread > 0.05 {
                    next_frontier.insert(neighbor_id);
                }
            }
        }
        frontier = next_frontier;
    }

    activation
}
```

### Enhanced Recall with Spreading

```rust
fn recall_with_spreading(
    query: &str,
    workspace_id: &str,
) -> Result<MemoryContext, MemoryError> {
    // Full recall: direct match + spreading activation.
    // 1. Direct matches (dual-channel)
    let direct = recall(query, workspace_id, 3)?;

    // 2. Spread from each direct match
    let mut all_activations: HashMap<String, f64> = HashMap::new();
    let network = get_network(workspace_id)?;
    for (concept_id, score) in &direct {
        let activations = spread_activation(concept_id, &network, 2, 0.5);
        for (aid, strength) in &activations {
            let combined = if aid == concept_id { *score } else { strength * score };
            let entry = all_activations.entry(aid.clone()).or_insert(0.0);
            *entry = (*entry).max(combined);
        }
    }

    // 3. Rank and select
    let mut ranked: Vec<_> = all_activations.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top5: Vec<_> = ranked.into_iter().take(5).collect();
    build_context(&top5, workspace_id)
}
```

## Intent Classification

Before recall, classify user intent to determine recall strategy.

### Phase 3 (MVP): Keyword Matching `[MVP]`

硬编码关键词匹配，覆盖 5 种核心意图。足以验证 pipeline 端到端，但覆盖率有限（无法处理中英混合变体表达、新增意图需改代码重新编译）。

```rust
const INTENT_TEMPLATES: &[(&str, &[&str])] = &[
    ("continue_investigation", &["继续看", "接着查", "还差", "接下来"]),
    ("verify_fact",            &["确认", "是不是", "有没有"]),
    ("correct_mistake",        &["不对", "错了", "不是这个", "其实是"]),
    ("add_knowledge",          &["补充", "还有", "另外", "加上"]),
    ("review_history",         &["之前", "上次", "历史", "原来"]),
];

fn classify_intent(query: &str) -> &str {
    for (intent, keywords) in INTENT_TEMPLATES {
        if keywords.iter().any(|kw| query.contains(kw)) {
            return intent;
        }
    }
    "general_query"
}
```

### Intent-driven Recall Priority

| Intent | Priority Content | Special Action |
|--------|-----------------|----------------|
| `continue_investigation` | task_state + open_questions | Resume from last checkpoint |
| `verify_fact` | known_facts + evidence | Show evidence sources |
| `correct_mistake` | rejected_hypotheses + current facts | Trigger Bayesian negation update |
| `add_knowledge` | concept entities + open_questions | Create new observation |
| `review_history` | concept summary + all facts | Full concept dump |
| `general_query` | ranked by activation score | Standard recall |

### Phase 7 (Enhancement): LLM Intent Classification `[Enhancement]`

用 LLM 替代硬编码关键词分类意图。**不做独立调用**——合并到 Memory Context 生成步骤中，由同一个 LLM 调用同时输出意图标注 + Memory Context。零额外延迟和成本。

**升级理由**：
- 硬编码关键词无法覆盖中英混合自然表达（如 "let me check 这个上次排查到哪了"）
- 新增意图无需改代码，LLM 自然泛化
- 已有 LLM 调用（Memory Context 生成），合并后零边际成本

**实现方式**：在 Memory Context 生成的 system prompt 中加入意图分类指令，LLM 输出结构化 JSON 包含 `intent` 字段和 `memory_context` 字段。

**激活阈值**：Phase 7，与高级召回机制（spreading activation、sparse activation、contextual priming）一同引入。

## Memory Context Generation

### Signature

```rust
fn build_context(
    ranked_concepts: &[(String, f64)],
    workspace_id: &str,
    intent: &str,
    max_tokens: usize,
) -> Result<MemoryContext, MemoryError> {
    // Build Memory Context from ranked concepts.
    // Budget: concept(300) + facts(400) + rejected(250) + prefs(200) + task(250) + evidence(100)
}
```

### Output Format

```
[Memory Context]
Workspace: <workspace>
Current Concept: <concept_name> (confidence: <score>)
Intent: <intent_type>

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

### Token Budget Allocation

| Section | Tokens | Content |
|---------|--------|---------|
| Current Concept | 300 | Name, definition, summary |
| Known Facts | 400 | Sorted by confidence, top facts |
| Rejected Hypotheses | 250 | Failed approaches |
| User Preferences | 200 | Relevant preferences |
| Task State | 250 | Open questions, next actions |
| Evidence Summary | 100 | Source counts, session refs |

### Fact Selection Logic

Within each section, facts are selected by:
1. Effective confidence — observations ranked by `effective_confidence` (`extraction_confidence × fact_confidence`); concepts/candidates by Beta-posterior `confidence` (`evidence_alpha / (evidence_alpha + evidence_beta)`). Higher first.
2. Source authority — user_confirmed > file_evidence > repeated > assistant_only
3. Recency — last_recalled_at
4. Relevance to intent — intent-driven priority

## Recall Execution Flow

```
Recent Context (last 3-5 messages)
  ↓
[Priming] pre-activate weakly related concepts (threshold=0.4, boost=20%)
  ↓
User Query
  ↓
Intent Classification (keyword matching)
  ↓
[Channel 1] Embed query → vector search → top-10 candidates (60%)
[Channel 2] Extract entities → entity match → candidates (40%)
  ↓
Merge scores + priming bonus → top-3 direct matches
  ↓
Spreading Activation (depth=2, decay=0.5) from each direct match
  ↓
[Sparse Activation] keep only top-5% of activated concepts, suppress rest
  ↓
[Hierarchy Expansion] expand children (50% score) + parent (30% score)
  ↓
Combine direct + spread + hierarchy scores → rank top-5
  ↓
Select content by intent-driven priority
  ↓
Compress to Memory Context (≤1500 tokens)
  ↓
Inject into Claude Code
  ↓
[Reconsolidation] mark recalled concepts as 'labile'
```

## Contextual Priming

Pre-activate concepts from recent conversation context before formal recall.

```rust
struct ContextualPrimer;

impl ContextualPrimer {
    /// Neuroscience: prior exposure to a stimulus facilitates subsequent processing.
    /// Recent conversation context pre-activates related concepts.

    fn prime(
        &self,
        recent_messages: &[String],
        workspace_id: &str,
    ) -> Result<HashMap<String, f64>, MemoryError> {
        // Extract context from last 3-5 messages, weakly activate related concepts.
        // Does NOT produce Memory Context — only updates activation baseline.
        let context: String = recent_messages.iter().rev().take(5).cloned()
            .collect::<Vec<_>>().join(" ");
        let context_embedding = embed(&context)?;

        // Weak threshold (0.4) — broader than recall's matching threshold
        let weak_matches = vector_search(&context_embedding, workspace_id, 0.4)?;

        let priming_map: HashMap<String, f64> = weak_matches.iter()
            .map(|(cid, sim)| (cid.clone(), sim * 0.2))
            .collect();
        Ok(priming_map)
    }

    fn recall_with_priming(
        &self,
        query: &str,
        recent_messages: &[String],
        workspace_id: &str,
    ) -> Result<Vec<(String, f64)>, MemoryError> {
        let priming = self.prime(recent_messages, workspace_id)?;
        let direct = recall(query, workspace_id, 5)?;

        // Primed concepts get 20% bonus
        let mut final_scores: HashMap<String, f64> = HashMap::new();
        for (cid, score) in &direct {
            let bonus = priming.get(cid).unwrap_or(&0.0);
            final_scores.insert(cid.clone(), score + bonus);
        }

        let mut ranked: Vec<_> = final_scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(ranked)
    }
}
```

## Sparse Activation

After spreading activation, suppress all but the top concepts. Brain analogy: only ~2% of neurons fire at any time.

```rust
fn sparse_activation(
    activation_map: &mut HashMap<String, f64>,
    sparsity: f64,
    concept_network: Option<&ConceptGraph>,
) -> HashMap<String, f64> {
    // Keep only top-K% activated concepts. Suppress the rest.
    // Competitive inhibition: strong activations suppress weaker neighbors.
    let mut sorted: Vec<_> = activation_map.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
    let keep_count = (sorted.len() as f64 * sparsity).max(1.0) as usize;

    let active: HashMap<String, f64> = sorted.iter()
        .take(keep_count)
        .map(|(k, v)| ((*k).clone(), *v))
        .collect();

    // Competitive inhibition
    if let Some(network) = concept_network {
        for (concept_id, _) in &active {
            for (neighbor_id, edge_strength) in network.neighbors(concept_id) {
                if !active.contains_key(neighbor_id) && activation_map.contains_key(neighbor_id) {
                    let entry = activation_map.get_mut(neighbor_id).unwrap();
                    *entry *= 1.0 - edge_strength * 0.3;
                }
            }
        }
    }

    active
}
```

## Hierarchy Expansion (Chunking)

When a concept is matched, expand to children and parent for zoom-in/zoom-out context.

```rust
fn recall_with_hierarchy(
    direct_matches: &[(String, f64)],
    workspace_id: &str,
) -> Result<Vec<(String, f64)>, MemoryError> {
    // Expand matched concepts along hierarchy:
    // - Children get 50% of parent's score (zoom in)
    // - Parent gets 30% of child's score (zoom out)
    let mut expanded: Vec<(String, f64)> = direct_matches.to_vec();
    let concept_ids: HashSet<&str> = direct_matches.iter().map(|(cid, _)| cid.as_str()).collect();

    for (concept_id, score) in direct_matches {
        // Expand children (more specific)
        let children = get_children(concept_id, workspace_id)?;
        for child in children {
            if !concept_ids.contains(child.id.as_str()) {
                expanded.push((child.id.clone(), score * 0.5));
            }
        }

        // Expand parent (more abstract)
        if let Some(parent) = get_parent(concept_id, workspace_id)? {
            if !concept_ids.contains(parent.id.as_str()) {
                expanded.push((parent.id.clone(), score * 0.3));
            }
        }
    }

    expanded.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Ok(expanded)
}
```

## Post-Recall Tracking

After each recall, record only the *attempt* (`record_recall`):

- `recall_count += 1`
- `last_recalled_at = now`

Success/failure is resolved **later, by explicit feedback** (`record_recall_outcome`, P4-B closed loop) — retrieval itself is never a success:

- Explicit positive feedback (Confirm/Supplement/Preference): `successful_recall_count += 1`
- Explicit negative feedback (Negate/Correct): `failed_recall_count += 1` + trigger Bayesian revision
- No feedback (silence): neither counter moves — a neutral, unresolved attempt (see quality-control.md §Concept Vitality for why silence ≠ failure)
