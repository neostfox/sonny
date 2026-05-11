# Memory Runtime — Quality Control & Confidence

## Confidence Model: Beta-Bernoulli

### Overview

Every observation, memory item, concept candidate, and concept maintains a Beta(alpha, beta) distribution:

- **alpha**: accumulated positive evidence
- **beta**: accumulated negative evidence
- **confidence** (materialized): alpha / (alpha + beta)
- **Initial prior**: Beta(1, 1) → confidence = 0.5

### Why Beta-Bernoulli

- Zero additional dependencies (pure Python)
- Sequential: each new observation naturally updates the prior
- Interpretable: alpha = "supporting evidence count", beta = "contradicting evidence count"
- Auditable: users can see exactly "N supporting, M contradicting → confidence X"
- Conservative: uniform prior means no observation is trusted by default

### Evidence Weight Table

| Evidence Type | Alpha Update | Beta Update | Explanation |
|--------------|-------------|-------------|-------------|
| User explicit confirmation | +2.0 | | Strong positive |
| File/project evidence | +1.5 | | Objective evidence |
| Repeated occurrence (no conflict) | +1.0 | | Weak positive |
| Cross ≥3 sessions consistent | +2.0 | | Auto-confirmation level |
| Cross 2 sessions consistent | +1.0 | | Strong signal |
| Human review confirm | +2.0 | | Strong positive |
| Recall not corrected by user | +0.3 | | Implicit positive |
| User negation | | +3.0 | Strong negative |
| New conflicting evidence | | +2.0 | Medium negative |
| Recall corrected by user | | +1.5 | Implicit negative |
| Internal conflict detected | | +1.0 | Self-check discovery |
| Long inactivity (>30 days) | | +0.5 × (days/30) | Gradual decay |
| Assistant speculation only | +0 | | No alpha boost |

### Implementation

```python
class BetaConfidence:
    def __init__(self, alpha: float = 1.0, beta: float = 1.0):
        self.alpha = alpha
        self.beta = beta

    @property
    def confidence(self) -> float:
        return self.alpha / (self.alpha + self.beta)

    def update(self, evidence_type: str):
        weights = EVIDENCE_WEIGHTS[evidence_type]
        self.alpha += weights.get('alpha', 0)
        self.beta += weights.get('beta', 0)

    def display(self) -> str:
        support = self.alpha - 1  # subtract prior
        contradict = self.beta - 1
        return f"{support:.0f} supporting, {contradict:.0f} contradicting (confidence: {self.confidence:.2f})"
```

## Concept Vitality Model

Vitality is a composite score that determines whether a concept should be promoted, demoted, or maintained.

### Vitality Dimensions

```python
def compute_vitality(concept) -> float:
    now = datetime.now()

    # 1. Time decay (90-day half-life)
    days = (now - concept.last_recalled_at).days
    time_decay = math.exp(-days / 90)

    # 2. Recall success rate
    success_rate = (
        concept.successful_recall_count / concept.recall_count
        if concept.recall_count > 0 else 0.5
    )

    # 3. Source diversity (5-session cap)
    diversity = min(1.0, concept.unique_session_count / 5)

    # 4. Network connectivity (10-connection cap)
    connectivity = min(1.0, concept.connection_count / 10)

    # 5. Bayesian confidence
    bayesian = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta)

    return (
        0.30 * bayesian +
        0.25 * success_rate +
        0.20 * diversity +
        0.15 * time_decay +
        0.10 * connectivity
    )
```

### Vitality Thresholds

| Vitality Range | Action | Status |
|---------------|--------|--------|
| > 0.80 | Auto-confirm if sessions ≥ 3 and no conflicts | candidate → auto_confirmed |
| 0.40 - 0.80 | Maintain as candidate | candidate (no change) |
| 0.20 - 0.40 | Warning — at risk | candidate (flagged for review) |
| < 0.40 (age > 30d) | Auto-demote | candidate → deprecated |
| Conflicts ≥ Support | Mark disputed | any → disputed |

## Cross-Session Consistency

### Auto-Confirmation Algorithm

```python
def check_auto_confirmation(concept) -> AutoConfirmResult:
    """
    Determine if a concept can be auto-confirmed without human review.
    ALL conditions must be true.
    """
    vitality = compute_vitality(concept)
    bayesian_confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta)

    conditions = {
        'vitality_sufficient': vitality > 0.80,
        'enough_sessions': concept.unique_session_count >= 3,
        'no_conflicts': concept.conflicts == 0,
        'bayesian_confident': bayesian_confidence > 0.75,
    }

    return AutoConfirmResult(
        can_confirm=all(conditions.values()),
        conditions=conditions,
        vitality=vitality,
        confidence=bayesian_confidence
    )
```

### Semantic Equivalence Detection

Determine if two observations express the same fact:

```python
def are_semantically_equivalent(obs_a, obs_b, embedding_sim) -> bool:
    # Hard match: identical entities + predicate
    if (obs_a.subject_text == obs_b.subject_text and
        obs_a.predicate == obs_b.predicate and
        obs_a.object_text == obs_b.object_text):
        return True

    # Soft match: entity overlap + high similarity
    entity_overlap = (
        obs_a.subject_text in obs_b.subject_text or
        obs_b.subject_text in obs_a.subject_text
    )
    if entity_overlap and embedding_sim > 0.85:
        return True

    return False

def are_conflicting(obs_a, obs_b) -> bool:
    """Same entity, opposite predicates."""
    if obs_a.subject_text != obs_b.subject_text:
        return False
    return is_negation_pair(obs_a.predicate, obs_b.predicate)
```

### Cross-Session Scoring

| Pattern | Score | Action |
|---------|-------|--------|
| ≥3 independent sessions, 100% consistent | +2.0 | Auto-confirmation candidate |
| 2 independent sessions, consistent | +1.0 | Strong signal |
| 1 session, user-provided | +0.5 | Moderate signal |
| Only assistant speculation | +0 | No boost |
| 1 conflict among consistent reports | -1.0 | Flag for review |
| ≥2 conflicts | -2.0 | Mark as disputed |

## Memory Reconsolidation

When a concept is recalled, it enters a **labile** (unstable) state — analogous to how recalling a memory in the brain makes it temporarily modifiable. After the recall window closes, the concept is re-consolidated (possibly modified).

### Labile State

```python
def mark_labile(concept_id: str, timeout_hours: float = 1.0):
    """
    Mark concept as labile on recall.
    timeout_hours: how long the concept stays modifiable (default: 1 hour).
    """
    concept = get_concept(concept_id)
    concept.status = 'labile'
    concept.labile_since = now()
    concept.labile_timeout = now() + timedelta(hours=timeout_hours)
```

### Reconsolidation

```python
def reconsolidate(concept):
    """
    Re-consolidate a labile concept.
    Triggered by: labile timeout, user confirmation, or user correction.
    """
    if concept.status != 'labile':
        return

    if concept.pending_corrections:
        # User provided corrections during labile window
        apply_corrections(concept)
        concept.evidence_alpha += 0.3  # reward for successful correction integration
    else:
        # No corrections — recall was successful
        concept.successful_recall_count += 1
        concept.evidence_alpha += 0.1  # weak positive: successful recall strengthens memory

    concept.status = 'active'
    concept.last_consolidated_at = now()
```

### Labile Window Behavior

| Event | During Labile Window | Action |
|-------|---------------------|--------|
| User confirms | Immediately re-consolidate | alpha += 2.0, status → active |
| User corrects | Apply correction, re-consolidate | alpha += 0.3, apply fix, status → active |
| User supplements | Add entities, re-consolidate | alpha += 0.5, extend concept, status → active |
| User negates | Create rejected_hypothesis, re-consolidate | beta += 3.0, status → active |
| Timeout (1h) | Auto re-consolidate | alpha += 0.1 (successful implicit recall), status → active |

**Why**: In the brain, recalled memories are rebuilt each time. This creates a natural window for correction. If the concept survives recall without correction, the successful retrieval strengthens it slightly (retrieval practice effect).

## Feedback-Driven Revision

### Feedback Classification

```python
def classify_feedback(feedback_text: str) -> str:
    """
    Classify user feedback type.
    Returns: 'confirm' | 'negate' | 'supplement' | 'correct' | 'preference'
    """
    negate_keywords = ['不对', '错了', '不是', '不正确', 'no', 'wrong', '不是这个']
    confirm_keywords = ['对', '没错', '正确', '是的', 'yes', 'right', '就是这个']
    supplement_keywords = ['还有', '补充', '另外', '加上', 'also', 'and']
    correct_keywords = ['应该是', '其实是', '实际上是', 'actually', 'should be']

    if any(kw in feedback_text for kw in negate_keywords):
        return 'negate'
    if any(kw in feedback_text for kw in confirm_keywords):
        return 'confirm'
    if any(kw in feedback_text for kw in supplement_keywords):
        return 'supplement'
    if any(kw in feedback_text for kw in correct_keywords):
        return 'correct'
    return 'general'
```

### Revision Actions

| Feedback Type | Alpha | Beta | Additional Action |
|--------------|-------|------|-------------------|
| `confirm` | +2.0 | | |
| `negate` | | +3.0 | Create rejected_hypothesis |
| `supplement` | +0.5 | | Add new entities to concept |
| `correct` | | +2.0 (old) | Create new observation, reject old |
| `preference` | +1.0 | | Store as user_preference_memory |

### Confidence Status After Revision

```python
def apply_revision(concept, feedback_type, feedback_text):
    # 1. Update alpha/beta
    concept.evidence_alpha += REVISION_WEIGHTS[feedback_type]['alpha']
    concept.evidence_beta += REVISION_WEIGHTS[feedback_type]['beta']

    # 2. Recompute confidence
    concept.confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta)

    # 3. Check status transitions
    if concept.confidence < 0.40 and concept.status in ('confirmed', 'auto_confirmed'):
        concept.status = 'deprecated'
    elif concept.confidence < 0.40 and concept.status == 'candidate':
        concept.status = 'deprecated' if days_since(concept.created_at) > 30 else concept.status

    # 4. Special actions
    if feedback_type == 'negate':
        create_rejected_hypothesis(concept, feedback_text)
    elif feedback_type == 'correct':
        reject_old_and_create_new(concept, feedback_text)
    elif feedback_type == 'supplement':
        add_entities_from_feedback(concept, feedback_text)
```

## Time Decay

### Decay Schedule

```python
def apply_time_decay(concept, now: datetime):
    """
    Apply gentle time decay to inactive concepts.
    Runs during periodic consolidation.
    """
    days_since_recall = (now - concept.last_recalled_at).days

    if days_since_recall > 30:
        decay_amount = 0.5 * (days_since_recall / 30)
        concept.evidence_beta += decay_amount
        concept.confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta)
```

**Half-life**: ~90 days (concept loses ~50% vitality if never recalled).
**Full decay threshold**: 180 days without recall → candidate for deprecated status.

## Quality Checklist

Before any concept enters `confirmed` or `auto_confirmed`:
- [ ] Has evidence with source_type NOT only "assistant_guess"
- [ ] confidence > 0.75 (bayesian)
- [ ] At least 1 user-provided or file-evidence source
- [ ] No unresolved conflicts
- [ ] Evidence count ≥ 3
- [ ] Unique session count ≥ 3 (for auto_confirmed)
- [ ] Passed adversarial validation (if enabled)

## Active Learning

### Overview

The system proactively identifies the most valuable questions to ask the user. Not random — targeted at the concepts where human judgment adds the most information gain.

### Signature

```python
class ActiveLearner:
    def get_questions(
        self,
        workspace_id: str,
        max_questions: int = 3
    ) -> list[Question]:
        """
        Generate prioritized questions for user confirmation.
        Returns at most max_questions, sorted by priority.
        """
```

### Question Types and Priority

| Type | Priority | Trigger | Example Question |
|------|----------|---------|-----------------|
| `near_confirm` | 0.9 | confidence in (0.65, 0.75) | "以下观察多次出现，可以确认吗？" |
| `conflict_resolve` | 0.8 | status = disputed | "发现矛盾信息，哪个正确？" |
| `alias_confirm` | 0.6 | unconfirmed entity alias | "A 和 B 是同一个东西吗？" |
| `stale_review` | 0.5 | age > 14d, confidence < 0.5 | "这个概念仍然相关吗？" |

### Question Timing

| Timing | Behavior | Max Questions |
|--------|----------|---------------|
| Session start | "开始前确认 N 个记忆" | 3 |
| Task completion | "任务完成了，顺便确认" | 2 |
| Idle accumulation | Background, remind when ≥5 accumulated | Notify only |

### Implementation

```python
@dataclass
class Question:
    type: str               # 'near_confirm' | 'conflict_resolve' | 'alias_confirm' | 'stale_review'
    priority: float         # 0.0 - 1.0
    concept_id: str | None
    text: str               # Natural language question for user
    options: list[str]      # Suggested answers
    evidence_summary: str   # Why we're asking

def get_questions(workspace_id, max_questions=3):
    candidates = []

    # Near-confirm: concept almost meets auto-confirmation threshold
    for concept in get_concepts(workspace_id, status='candidate'):
        if 0.65 < concept.confidence < 0.75:
            candidates.append(Question(
                type='near_confirm', priority=0.9,
                concept_id=concept.id,
                text=f"以下信息多次出现，可以确认吗？\n{concept.known_facts[0]}",
                options=['确认', '否定', '不确定'],
                evidence_summary=f"{concept.evidence_count} 条证据来自 {concept.unique_session_count} 个 session"
            ))

    # Conflict resolution
    for concept in get_concepts(workspace_id, status='disputed'):
        candidates.append(Question(
            type='conflict_resolve', priority=0.8,
            concept_id=concept.id,
            text=f"发现矛盾信息：\nA: {concept.conflicts[0]}\nB: {concept.conflicts[1]}\n哪个正确？",
            options=['A 正确', 'B 正确', '都不对', '都对（不同上下文）'],
            evidence_summary=f"{len(concept.conflicts)} 处矛盾"
        ))

    # Entity alias
    for alias in get_unconfirmed_aliases(workspace_id):
        candidates.append(Question(
            type='alias_confirm', priority=0.6,
            text=f'"{alias.term_a}" 和 "{alias.term_b}" 是同一个东西吗？',
            options=['是', '不是', '相关但不同'],
            evidence_summary=f"在 {alias.co_occurrence_count} 个 session 中共同出现"
        ))

    # Stale review
    for concept in get_concepts(workspace_id, status='candidate'):
        days = days_since(concept.created_at)
        if days > 14 and concept.confidence < 0.5:
            candidates.append(Question(
                type='stale_review', priority=0.5,
                concept_id=concept.id,
                text=f"这个概念已存在 {days} 天但置信度较低，仍然相关吗？\n{concept.summary}",
                options=['仍然相关', '已过时', '合并到其他概念'],
                evidence_summary=f"置信度 {concept.confidence:.2f}，{concept.recall_count} 次召回"
            ))

    candidates.sort(key=lambda q: -q.priority)
    return candidates[:max_questions]
```

## Adversarial Validation

### Overview

A second LLM (Validator) challenges each extracted observation. This addresses the core risk: LLM extraction quality is the system's biggest uncertainty.

### Signature

```python
class AdversarialValidator:
    def validate(
        self,
        observations: list[Observation],
        raw_text: str
    ) -> list[ValidatedObservation]:
        """
        Validate extracted observations against source text.
        Returns validated observations with verdicts.
        """
```

### Validation Criteria

Each observation is challenged on five dimensions:

1. **Evidence clarity**: Is the evidence_text a clear, unambiguous support?
2. **Over-speculation**: Does the observation go beyond what the evidence supports?
3. **Triple accuracy**: Are subject/predicate/object correctly identified?
4. **Confidence calibration**: Is the stated confidence appropriate?
5. **Source attribution**: Is user vs assistant content correctly distinguished?

### Verdicts

| Verdict | Action | Confidence Adjustment |
|---------|--------|----------------------|
| `accepted` | Observation passes | None |
| `adjusted` | Partially correct | × 0.7 |
| `rejected` | Fundamentally flawed | status → disputed |

### Implementation

```python
@dataclass
class ValidationChallenge:
    accepted: bool
    partial: bool
    reason: str
    criteria_failed: list[str]

def validate_observation(obs, raw_text, validator_llm):
    challenge = validator_llm.challenge(
        observation=obs,
        source_text=raw_text,
        criteria=[
            "是否有明确的证据支持？",
            "是否过度推测？",
            "subject/predicate/object 是否准确？",
            "置信度是否合理？",
            "是否混淆了用户事实和助手推测？"
        ]
    )
    return challenge
```

### Cost Control Strategies

| Strategy | Description | When to Use |
|----------|-------------|-------------|
| **Sampling** | Only validate obs with surprise_score > 0.5 | Default mode |
| **Caching** | Reuse validation prompts for similar session structures | Repeated session types |
| **Self-consistency** | Same LLM extracts 3×, keep consistent results | No validator LLM available |
| **Skip small sessions** | Only validate sessions with >10 observations | Cost-sensitive environments |

### Quality Gate

Adversarial validation runs as a quality gate between Extract and Embed stages:

```
RawMemory → [Extractor LLM] → Observations → [Validator LLM] → ValidatedObs → [Embed]
                                      ↑                              |
                                      └── rejected → disputed ────────┘
```
