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

```rust
struct BetaConfidence {
    alpha: f32,
    beta: f32,
}

impl BetaConfidence {
    fn new() -> Self {
        Self { alpha: 1.0, beta: 1.0 }
    }

    fn confidence(&self) -> f32 {
        self.alpha / (self.alpha + self.beta)
    }

    fn update(&mut self, evidence_type: &str) {
        let weights = EVIDENCE_WEIGHTS.get(evidence_type)
            .expect("unknown evidence type");
        self.alpha += weights.alpha;
        self.beta += weights.beta;
    }

    fn display(&self) -> String {
        let support = self.alpha - 1.0; // subtract prior
        let contradict = self.beta - 1.0;
        format!("{:.0} supporting, {:.0} contradicting (confidence: {:.2})",
            support, contradict, self.confidence())
    }
}
```

## Concept Vitality Model

Vitality is a composite score that determines whether a concept should be promoted, demoted, or maintained.

### Vitality Dimensions

```rust
fn compute_vitality(concept: &Concept) -> f32 {
    // 1. Time decay (90-day half-life)
    let days = days_since(&concept.last_recalled_at);
    let time_decay = (-days as f32 / 90.0).exp();

    // 2. Recall success rate
    let success_rate = if concept.recall_count > 0 {
        concept.successful_recall_count as f32 / concept.recall_count as f32
    } else { 0.5 };

    // 3. Source diversity (5-session cap)
    let diversity = (concept.unique_session_count as f32 / 5.0).min(1.0);

    // 4. Network connectivity (10-connection cap)
    let connectivity = (concept.connection_count as f32 / 10.0).min(1.0);

    // 5. Bayesian confidence
    let bayesian = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    0.30 * bayesian +
    0.25 * success_rate +
    0.20 * diversity +
    0.15 * time_decay +
    0.10 * connectivity
}
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

```rust
struct AutoConfirmResult {
    can_confirm: bool,
    vitality_sufficient: bool,
    enough_sessions: bool,
    no_conflicts: bool,
    bayesian_confident: bool,
    vitality: f32,
    confidence: f32,
}

fn check_auto_confirmation(concept: &Concept) -> AutoConfirmResult {
    // Determine if a concept can be auto-confirmed without human review.
    // ALL conditions must be true.
    let vitality = compute_vitality(concept);
    let bayesian_confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    let vitality_sufficient = vitality > 0.80;
    let enough_sessions = concept.unique_session_count >= 3;
    let no_conflicts = concept.conflicts == 0;
    let bayesian_confident = bayesian_confidence > 0.75;

    AutoConfirmResult {
        can_confirm: vitality_sufficient && enough_sessions && no_conflicts && bayesian_confident,
        vitality_sufficient,
        enough_sessions,
        no_conflicts,
        bayesian_confident,
        vitality,
        confidence: bayesian_confidence,
    }
}
```

### Semantic Equivalence Detection

Determine if two observations express the same fact:

```rust
fn are_semantically_equivalent(obs_a: &Observation, obs_b: &Observation, embedding_sim: f32) -> bool {
    // Hard match: identical entities + predicate
    if obs_a.subject_text == obs_b.subject_text &&
       obs_a.predicate == obs_b.predicate &&
       obs_a.object_text == obs_b.object_text {
        return true;
    }

    // Soft match: entity overlap + high similarity
    let entity_overlap = obs_a.subject_text.contains(&obs_b.subject_text) ||
                         obs_b.subject_text.contains(&obs_a.subject_text);
    if entity_overlap && embedding_sim > 0.85 {
        return true;
    }

    false
}

fn are_conflicting(obs_a: &Observation, obs_b: &Observation) -> bool {
    // Same entity, opposite predicates.
    if obs_a.subject_text != obs_b.subject_text { return false; }
    is_negation_pair(&obs_a.predicate, &obs_b.predicate)
}
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

```rust
fn mark_labile(concept_id: &str, timeout_hours: f32) -> Result<(), MemoryError> {
    // Mark concept as labile on recall.
    // timeout_hours: how long the concept stays modifiable (default: 1 hour).
    let mut concept = get_concept(concept_id)?;
    concept.status = ConceptStatus::Labile;
    concept.labile_since = Some(Utc::now());
    concept.labile_timeout = Some(Utc::now() + Duration::hours(timeout_hours as i64));
    update_concept(&concept)
}
```

### Reconsolidation

```rust
fn reconsolidate(concept: &mut Concept) -> Result<(), MemoryError> {
    // Re-consolidate a labile concept.
    // Triggered by: labile timeout, user confirmation, or user correction.
    if concept.status != ConceptStatus::Labile { return Ok(()); }

    if !concept.pending_corrections.is_empty() {
        // User provided corrections during labile window
        apply_corrections(concept)?;
        concept.evidence_alpha += 0.3; // reward for successful correction integration
    } else {
        // No corrections — recall was successful
        concept.successful_recall_count += 1;
        concept.evidence_alpha += 0.1; // weak positive: successful recall strengthens memory
    }

    concept.status = ConceptStatus::Active;
    concept.last_consolidated_at = Some(Utc::now());
    update_concept(concept)
}
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

```rust
fn classify_feedback(feedback_text: &str) -> &str {
    // Classify user feedback type.
    // Returns: "confirm" | "negate" | "supplement" | "correct" | "preference" | "general"
    let negate_keywords = ["不对", "错了", "不是", "不正确", "no", "wrong", "不是这个"];
    let confirm_keywords = ["对", "没错", "正确", "是的", "yes", "right", "就是这个"];
    let supplement_keywords = ["还有", "补充", "另外", "加上", "also", "and"];
    let correct_keywords = ["应该是", "其实是", "实际上是", "actually", "should be"];

    if negate_keywords.iter().any(|kw| feedback_text.contains(kw)) { return "negate"; }
    if confirm_keywords.iter().any(|kw| feedback_text.contains(kw)) { return "confirm"; }
    if supplement_keywords.iter().any(|kw| feedback_text.contains(kw)) { return "supplement"; }
    if correct_keywords.iter().any(|kw| feedback_text.contains(kw)) { return "correct"; }
    "general"
}
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

```rust
fn apply_revision(
    concept: &mut Concept,
    feedback_type: &str,
    feedback_text: &str,
) -> Result<(), MemoryError> {
    // 1. Update alpha/beta
    let weights = REVISION_WEIGHTS.get(feedback_type)
        .expect("unknown feedback type");
    concept.evidence_alpha += weights.alpha;
    concept.evidence_beta += weights.beta;

    // 2. Recompute confidence
    concept.confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    // 3. Check status transitions
    if concept.confidence < 0.40 &&
       (concept.status == ConceptStatus::Confirmed || concept.status == ConceptStatus::AutoConfirmed) {
        concept.status = ConceptStatus::Deprecated;
    } else if concept.confidence < 0.40 && concept.status == ConceptStatus::Candidate {
        if days_since(&concept.created_at) > 30 {
            concept.status = ConceptStatus::Deprecated;
        }
    }

    // 4. Special actions
    match feedback_type {
        "negate" => create_rejected_hypothesis(concept, feedback_text)?,
        "correct" => reject_old_and_create_new(concept, feedback_text)?,
        "supplement" => add_entities_from_feedback(concept, feedback_text)?,
        _ => {}
    }

    update_concept(concept)
}
```

## Time Decay

### Decay Schedule

```rust
fn apply_time_decay(concept: &mut Concept, now: &DateTime<Utc>) {
    // Apply gentle time decay to inactive concepts.
    // Runs during periodic consolidation.
    let days_since_recall = (*now - concept.last_recalled_at).num_days();

    if days_since_recall > 30 {
        let decay_amount = 0.5 * (days_since_recall as f32 / 30.0);
        concept.evidence_beta += decay_amount;
        concept.confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);
    }
}
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

```rust
struct ActiveLearner;

impl ActiveLearner {
    fn get_questions(
        &self,
        workspace_id: &str,
        max_questions: usize,
    ) -> Result<Vec<Question>, MemoryError> {
        // Generate prioritized questions for user confirmation.
        // Returns at most max_questions, sorted by priority.
    }
}
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

```rust
struct Question {
    q_type: QuestionType,   // NearConfirm | ConflictResolve | AliasConfirm | StaleReview
    priority: f32,          // 0.0 - 1.0
    concept_id: Option<String>,
    text: String,           // Natural language question for user
    options: Vec<String>,   // Suggested answers
    evidence_summary: String, // Why we're asking
}

fn get_questions(workspace_id: &str, max_questions: usize) -> Result<Vec<Question>, MemoryError> {
    let mut candidates = Vec::new();

    // Near-confirm: concept almost meets auto-confirmation threshold
    for concept in get_concepts(workspace_id, ConceptStatus::Candidate)? {
        if concept.confidence > 0.65 && concept.confidence < 0.75 {
            candidates.push(Question {
                q_type: QuestionType::NearConfirm,
                priority: 0.9,
                concept_id: Some(concept.id.clone()),
                text: format!("以下信息多次出现，可以确认吗？\n{}", concept.known_facts[0]),
                options: vec!["确认".into(), "否定".into(), "不确定".into()],
                evidence_summary: format!("{} 条证据来自 {} 个 session",
                    concept.evidence_count, concept.unique_session_count),
            });
        }
    }

    // Conflict resolution
    for concept in get_concepts(workspace_id, ConceptStatus::Disputed)? {
        candidates.push(Question {
            q_type: QuestionType::ConflictResolve,
            priority: 0.8,
            concept_id: Some(concept.id.clone()),
            text: format!("发现矛盾信息：\nA: {}\nB: {}\n哪个正确？",
                concept.conflicts[0], concept.conflicts[1]),
            options: vec!["A 正确".into(), "B 正确".into(), "都不对".into(), "都对（不同上下文）".into()],
            evidence_summary: format!("{} 处矛盾", concept.conflicts.len()),
        });
    }

    // Entity alias
    for alias in get_unconfirmed_aliases(workspace_id)? {
        candidates.push(Question {
            q_type: QuestionType::AliasConfirm,
            priority: 0.6,
            concept_id: None,
            text: format!("\"{}\" 和 \"{}\" 是同一个东西吗？", alias.term_a, alias.term_b),
            options: vec!["是".into(), "不是".into(), "相关但不同".into()],
            evidence_summary: format!("在 {} 个 session 中共同出现", alias.co_occurrence_count),
        });
    }

    // Stale review
    for concept in get_concepts(workspace_id, ConceptStatus::Candidate)? {
        let days = days_since(&concept.created_at);
        if days > 14 && concept.confidence < 0.5 {
            candidates.push(Question {
                q_type: QuestionType::StaleReview,
                priority: 0.5,
                concept_id: Some(concept.id.clone()),
                text: format!("这个概念已存在 {} 天但置信度较低，仍然相关吗？\n{}", days, concept.summary),
                options: vec!["仍然相关".into(), "已过时".into(), "合并到其他概念".into()],
                evidence_summary: format!("置信度 {:.2}，{} 次召回", concept.confidence, concept.recall_count),
            });
        }
    }

    candidates.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap());
    Ok(candidates.into_iter().take(max_questions).collect())
}
```

## Adversarial Validation

### Overview

A second LLM (Validator) challenges each extracted observation. This addresses the core risk: LLM extraction quality is the system's biggest uncertainty.

### Signature

```rust
struct AdversarialValidator;

impl AdversarialValidator {
    fn validate(
        &self,
        observations: &[Observation],
        raw_text: &str,
    ) -> Result<Vec<ValidatedObservation>, MemoryError> {
        // Validate extracted observations against source text.
        // Returns validated observations with verdicts.
    }
}
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

```rust
struct ValidationChallenge {
    accepted: bool,
    partial: bool,
    reason: String,
    criteria_failed: Vec<String>,
}

fn validate_observation(
    obs: &Observation,
    raw_text: &str,
    validator_llm: &dyn LlmProvider,
) -> Result<ValidationChallenge, MemoryError> {
    let challenge = validator_llm.challenge(
        observation=obs,
        source_text=raw_text,
        criteria=&[
            "是否有明确的证据支持？",
            "是否过度推测？",
            "subject/predicate/object 是否准确？",
            "置信度是否合理？",
            "是否混淆了用户事实和助手推测？",
        ]
    )?;
    Ok(challenge)
}
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
