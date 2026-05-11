# Memory Runtime — Recall Design

## Recall Goal

Recall is NOT "find similar text". Recall is:

> Find the concept matching the current problem, and expand the most useful context from that concept.

## Dual-Channel Recall

### Channel 1: Semantic Similarity (Embedding)

Embed the user query and search for nearest concept/concept_candidate vectors.

```python
def semantic_recall(query: str, workspace_id: str, top_k: int = 10) -> list[tuple[str, float]]:
    """
    Embed query, search vec_embedding for nearest concepts.
    Returns [(concept_id, similarity_score)].
    """
    query_embedding = embed(query)
    results = vector_search(query_embedding, workspace_id, top_k=top_k)
    return [(r.source_id, r.distance) for r in results]
```

**Weight**: 60% of final score.

### Channel 2: Entity Exact Match

Extract entities from the query using lightweight LLM call, then match against concept entity lists.

```python
def entity_recall(query: str, workspace_id: str) -> list[tuple[str, float]]:
    """
    Extract entities from query, match against concept.related_entities.
    Returns [(concept_id, overlap_ratio)].
    """
    query_entities = llm_extract_entities(query)
    concepts = get_concepts_by_entities(query_entities, workspace_id)
    results = []
    for concept in concepts:
        overlap = len(concept.entities & set(query_entities)) / len(concept.entities)
        results.append((concept.id, overlap))
    return results
```

**Weight**: 40% of final score.

### Merged Score

```python
def recall(query: str, workspace_id: str, top_n: int = 5) -> list[tuple[str, float]]:
    """
    Dual-channel recall with merged scoring.
    """
    semantic_results = semantic_recall(query, workspace_id, top_k=10)
    entity_results = entity_recall(query, workspace_id)

    scores = {}
    for cid, sim in semantic_results:
        scores[cid] = scores.get(cid, 0) + sim * 0.6
    for cid, overlap in entity_results:
        scores[cid] = scores.get(cid, 0) + overlap * 0.4

    return sorted(scores.items(), key=lambda x: -x[1])[:top_n]
```

## Spreading Activation

When a concept is activated, partially activate related concepts through the concept network.

```python
def spread_activation(
    seed_concept_id: str,
    concept_network: Graph,
    max_depth: int = 2,
    decay_rate: float = 0.5
) -> dict[str, float]:
    """
    Spread activation from seed concept through network.
    Returns {concept_id: activation_strength}.
    """
    activation = {seed_concept_id: 1.0}
    frontier = {seed_concept_id}

    for depth in range(max_depth):
        next_frontier = set()
        for concept_id in frontier:
            current = activation[concept_id]
            if current < 0.05:
                continue
            for neighbor_id, edge_strength in concept_network.neighbors(concept_id):
                spread = current * edge_strength * decay_rate
                activation[neighbor_id] = max(activation.get(neighbor_id, 0), spread)
                if spread > 0.05:
                    next_frontier.add(neighbor_id)
        frontier = next_frontier

    return activation
```

### Enhanced Recall with Spreading

```python
def recall_with_spreading(query: str, workspace_id: str) -> MemoryContext:
    """
    Full recall: direct match + spreading activation.
    """
    # 1. Direct matches (dual-channel)
    direct = recall(query, workspace_id, top_n=3)

    # 2. Spread from each direct match
    all_activations = {}
    for concept_id, score in direct:
        activations = spread_activation(concept_id, get_network(workspace_id))
        for aid, strength in activations.items():
            combined = score if aid == concept_id else strength * score
            all_activations[aid] = max(all_activations.get(aid, 0), combined)

    # 3. Rank and select
    ranked = sorted(all_activations.items(), key=lambda x: -x[1])[:5]
    return build_context(ranked, workspace_id)
```

## Intent Classification

Before recall, classify user intent to determine recall strategy.

```python
INTENT_TEMPLATES = {
    'continue_investigation': ['继续看', '接着查', '还差', '接下来'],
    'verify_fact':            ['确认', '是不是', '有没有'],
    'correct_mistake':        ['不对', '错了', '不是这个', '其实是'],
    'add_knowledge':          ['补充', '还有', '另外', '加上'],
    'review_history':         ['之前', '上次', '历史', '原来'],
}

def classify_intent(query: str) -> str:
    for intent, keywords in INTENT_TEMPLATES.items():
        if any(kw in query for kw in keywords):
            return intent
    return 'general_query'
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

## Memory Context Generation

### Signature

```python
def build_context(
    ranked_concepts: list[tuple[str, float]],
    workspace_id: str,
    intent: str = 'general_query',
    max_tokens: int = 1500
) -> MemoryContext:
    """
    Build Memory Context from ranked concepts.
    Budget: concept(300) + facts(400) + rejected(250) + prefs(200) + task(250) + evidence(100)
    """
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
1. Confidence (alpha / (alpha + beta)) — higher first
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

```python
class ContextualPrimer:
    """
    Neuroscience: prior exposure to a stimulus facilitates subsequent processing.
    Recent conversation context pre-activates related concepts.
    """

    def prime(self, recent_messages: list[str], workspace_id: str) -> dict[str, float]:
        """
        Extract context from last 3-5 messages, weakly activate related concepts.
        Does NOT produce Memory Context — only updates activation baseline.
        """
        context_text = " ".join(recent_messages[-5:])
        context_embedding = embed(context_text)

        # Weak threshold (0.4) — broader than recall's matching threshold
        weak_matches = vector_search(context_embedding, workspace_id, threshold=0.4)

        priming_map = {}
        for concept_id, similarity in weak_matches:
            priming_map[concept_id] = similarity * 0.2  # weak activation
        return priming_map

    def recall_with_priming(self, query, recent_messages, workspace_id):
        priming = self.prime(recent_messages, workspace_id)
        direct = recall(query, workspace_id, top_n=5)

        # Primed concepts get 20% bonus
        final_scores = {}
        for cid, score in direct:
            final_scores[cid] = score
            if cid in priming:
                final_scores[cid] += priming[cid]
        return sorted(final_scores.items(), key=lambda x: -x[1])
```

## Sparse Activation

After spreading activation, suppress all but the top concepts. Brain analogy: only ~2% of neurons fire at any time.

```python
def sparse_activation(
    activation_map: dict[str, float],
    sparsity: float = 0.05,
    concept_network: Graph = None
) -> dict[str, float]:
    """
    Keep only top-K% activated concepts. Suppress the rest.
    Competitive inhibition: strong activations suppress weaker neighbors.
    """
    sorted_acts = sorted(activation_map.items(), key=lambda x: -x[1])
    keep_count = max(1, int(len(sorted_acts) * sparsity))

    active = dict(sorted_acts[:keep_count])

    # Competitive inhibition
    if concept_network:
        for concept_id, strength in active.items():
            for neighbor_id, edge_strength in concept_network.neighbors(concept_id):
                if neighbor_id in activation_map and neighbor_id not in active:
                    activation_map[neighbor_id] *= (1 - edge_strength * 0.3)

    return active
```

## Hierarchy Expansion (Chunking)

When a concept is matched, expand to children and parent for zoom-in/zoom-out context.

```python
def recall_with_hierarchy(
    direct_matches: list[tuple[str, float]],
    workspace_id: str
) -> list[tuple[str, float]]:
    """
    Expand matched concepts along hierarchy:
    - Children get 50% of parent's score (zoom in)
    - Parent gets 30% of child's score (zoom out)
    """
    expanded = list(direct_matches)
    concept_ids = {cid for cid, _ in direct_matches}

    for concept_id, score in direct_matches:
        # Expand children (more specific)
        children = get_children(concept_id, workspace_id)
        for child in children:
            if child.id not in concept_ids:
                expanded.append((child.id, score * 0.5))

        # Expand parent (more abstract)
        parent = get_parent(concept_id, workspace_id)
        if parent and parent.id not in concept_ids:
            expanded.append((parent.id, score * 0.3))

    return sorted(expanded, key=lambda x: -x[1])
```

## Post-Recall Tracking

After each recall, update concept vitality metrics:

- `recall_count += 1`
- `last_recalled_at = now`
- If user continues without correction: `successful_recall_count += 1`
- If user corrects: `failed_recall_count += 1` + trigger Bayesian revision
