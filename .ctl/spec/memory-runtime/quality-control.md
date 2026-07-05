# Memory Runtime — Quality Control & Confidence

## Confidence Model: Beta-Bernoulli

### Overview

Every observation, concept candidate, and concept maintains a Beta(alpha, beta) distribution over its **fact** truthfulness:

- **alpha**: accumulated positive evidence
- **beta**: accumulated negative evidence
- **fact_confidence**: `alpha / (alpha + beta)`
- **Initial prior**: Beta(1, 1) → fact_confidence = 0.5

Observations additionally carry an **immutable** `extraction_confidence`, fixed at extraction time by `source_type` provenance (FileEvidence 0.9, UserConfirm 0.85, UserNegation 0.8, UserMessage 0.7, AssistantGuess 0.3). Recall ranking uses the source-gated product:

```
effective_confidence = extraction_confidence × fact_confidence
```

An untrustworthy source cannot reach high confidence even if repeatedly uncorrected; a trustworthy source contradicted by evidence is still pressed down. Code: `ObservationSourceType::extraction_confidence()`, `Observation::effective_confidence()`.

Concept / ConceptCandidate have no extraction dimension (they are grown, not extracted) and use `fact_confidence` directly.

### Why Beta-Bernoulli

- Zero additional dependencies (pure Rust)
- Sequential: each new observation naturally updates the prior
- Interpretable: alpha = "supporting evidence count", beta = "contradicting evidence count"
- Auditable: users can see exactly "N supporting, M contradicting → confidence X"
- Conservative: uniform prior means no observation is trusted by default

### Evidence Weight Table

Evidence drives `fact_confidence` via α/β updates. Time is **not** evidence — see [Time Decay](#time-decay) for the separate salience model.

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
| Assistant speculation only | +0 | | No alpha boost |

12 evidence types — matches `EvidenceType` enum and `EVIDENCE_TYPE_VARIANT_COUNT` in `confidence/mod.rs`.

### Implementation (f64)

```rust
pub struct BetaConfidence {
    pub alpha: f64,
    pub beta: f64,
}

impl BetaConfidence {
    pub fn new() -> Self { Self { alpha: 1.0, beta: 1.0 } }

    pub fn confidence(&self) -> f64 { self.alpha / (self.alpha + self.beta) }

    pub fn update(&mut self, evidence_type: &EvidenceType) {
        let (a, b) = get_weight(evidence_type);
        self.alpha += a;
        self.beta += b;
    }
}
```

> **Note**: there is deliberately **no** `update_with_decay` on `BetaConfidence`. Decay is a recall-time salience factor on the vitality score, not a mutation of the evidence posterior. See [Time Decay](#time-decay).

## Concept Vitality Model

Vitality is a composite score that determines whether a concept should be promoted, demoted, or maintained. It is computed **at recall / consolidation time** and never persisted into α/β.

### Vitality Dimensions

```rust
fn compute_vitality(concept: &Concept) -> f64 {
    // 1. Bayesian fact confidence
    let bayesian = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    // 2. Recall success rate (0.5 prior when never recalled)
    let success_rate = if concept.recall_count > 0 {
        concept.successful_recall_count as f64 / concept.recall_count as f64
    } else { 0.5 };

    // 3. Source diversity (5-session cap)
    let diversity = (concept.unique_session_count as f64 / 5.0).min(1.0);

    // 4. Recency — Ebbinghaus forgetting curve with rehearsal (spacing) effect.
    //    Successful recall slows forgetting: λ shrinks as successful_recall_count grows.
    let anchor = concept.last_recalled_at.or(concept.created_at);
    let days = days_since(&anchor);
    let lambda_base = 1.0 / 90.0; // ~90-day base half-life
    let lambda_eff = lambda_base / (1.0 + 0.5 * concept.successful_recall_count as f64);
    let time_decay = (-(lambda_eff) * days).exp();

    // 5. Network connectivity (10-connection cap)
    let connectivity = (concept.connection_count as f64 / 10.0).min(1.0);

    0.30 * bayesian + 0.25 * success_rate + 0.20 * diversity +
    0.15 * time_decay + 0.10 * connectivity
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

## Time Decay

### Design Decision: Multiplicative Recency, Not Evidence Mutation

Decay models how **salient** a concept is at recall time — not how **true** it is. These are independent dimensions and must not be conflated:

- **fact_confidence (α/β)** answers "how much evidence supports/contradicts this fact". Only evidence updates it.
- **time_decay** answers "how long since this was last useful". It is a multiplicative factor inside `compute_vitality`, recomputed every recall from `last_recalled_at`.

**Rejected alternative** — mutating `evidence_beta` by inactivity (`beta += 0.5 × days/30`):

1. **Concept confusion**: a well-evidenced fact that simply hasn't been queried in 90 days would see β grow without bound → fact_confidence → 0 → auto-demote. It did not become false; it became stale. Those are different.
2. **Double penalty**: `bayesian` already reads α/β at 0.30 weight; an inactivity-driven β would depress both `bayesian` and `time_decay` (0.15) — decay counted twice.
3. **Irreversible / no rehearsal path**: additive β has no reset on recall, so the spacing effect (the single most validated finding in memory research) cannot be expressed.

### Decay Formula (Ebbinghaus + Rehearsal)

```
λ_eff   = λ_base / (1 + k · successful_recall_count)      // k = 0.5, λ_base = 1/90
Δt      = now − last_recalled_at          (fallback: created_at if never recalled)
recency = exp(−λ_eff · Δt)                               // ∈ (0, 1]
```

- Recall **resets the forgetting clock** (`last_recalled_at`, via `record_recall`), but only a recall the user *validated* slows future decay: `successful_recall_count` is incremented by `record_recall_outcome(true)` from explicit positive feedback (P4-B), never by retrieval itself. This is the spacing effect without the rehearsal positive-feedback defect (a hot-but-wrong concept no longer strengthens just by being retrieved).
- Never-recalled concepts anchor Δt to `created_at`; combined with the `success_rate = 0.5` prior in vitality, a brand-new concept starts near the middle and ages out if never touched.
- Config knob: `ConfidenceConfig.decay_half_life_days` (default 90.0) sets `λ_base = 1 / decay_half_life_days`.

### When Decay Runs

- **Recall ranking**: `time_decay` is one factor in `compute_vitality`, applied live to every candidate concept.
- **Consolidation**: a periodic consolidation pass evaluates vitality for auto promote/demote. It does **not** mutate α/β; it only transitions status based on the vitality thresholds above.
- **`last_recalled_at` update**: every recall attempt (`record_recall`) refreshes the anchor, so an active concept never decays; success/failure counters move only on explicit feedback (`record_recall_outcome`).

## Cross-Session Consistency

### Auto-Confirmation Algorithm

```rust
struct AutoConfirmResult {
    can_confirm: bool,
    vitality_sufficient: bool,
    enough_sessions: bool,
    no_conflicts: bool,
    bayesian_confident: bool,
    vitality: f64,
    confidence: f64,
}

fn check_auto_confirmation(concept: &Concept) -> AutoConfirmResult {
    let vitality = compute_vitality(concept);
    let bayesian_confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    let vitality_sufficient = vitality > 0.80;
    let enough_sessions = concept.unique_session_count >= 3;
    let no_conflicts = concept.conflicts == 0;
    let bayesian_confident = bayesian_confidence > 0.75;

    AutoConfirmResult {
        can_confirm: vitality_sufficient && enough_sessions && no_conflicts && bayesian_confident,
        vitality_sufficient, enough_sessions, no_conflicts, bayesian_confident,
        vitality, confidence: bayesian_confidence,
    }
}
```

### Semantic Equivalence Detection

Determine if two observations express the same fact:

```rust
fn are_semantically_equivalent(obs_a: &Observation, obs_b: &Observation, embedding_sim: f64) -> bool {
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
fn mark_labile(concept_id: &str, timeout_hours: f64) -> Result<(), MemoryError> {
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
    if concept.status != ConceptStatus::Labile { return Ok(()); }

    if !concept.pending_corrections.is_empty() {
        apply_corrections(concept)?;
        concept.evidence_alpha += 0.3; // reward for successful correction integration
    } else {
        concept.successful_recall_count += 1;
        concept.evidence_alpha += 0.1; // retrieval practice effect
    }

    concept.status = ConceptStatus::Active;
    concept.last_consolidated_at = Some(Utc::now());
    update_concept(concept)
}
```

Note: `evidence_alpha += 0.1` on successful recall is a *retrieval-practice* evidence signal (genuinely strengthens the fact), distinct from the *recency* salience factor. The two coexist: successful recall both adds weak evidence (α) and resets the decay clock (`last_recalled_at`).

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
    let negate_keywords = ["不对", "错了", "不是", "不正确", "no", "wrong", "不是这个"];
    let confirm_keywords = ["对", "没错", "正确", "是的", "yes", "right", "就是这个"];
    let supplement_keywords = ["还有", "补充", "另外", "加上", "also", "and"];
    let correct_keywords = ["应该是", "其实是", "实际上是", "actually", "should be"];
    let preference_keywords = ["偏好", "以后都", "我喜欢", "prefer", "i like", "always use"];

    // keyword_hit is boundary-aware, NOT raw substring — see rules below.
    if negate_keywords.iter().any(|kw| keyword_hit(feedback_text, kw)) { return "negate"; }
    if confirm_keywords.iter().any(|kw| keyword_hit(feedback_text, kw)) { return "confirm"; }
    if supplement_keywords.iter().any(|kw| keyword_hit(feedback_text, kw)) { return "supplement"; }
    if correct_keywords.iter().any(|kw| keyword_hit(feedback_text, kw)) { return "correct"; }
    if preference_keywords.iter().any(|kw| keyword_hit(feedback_text, kw)) { return "preference"; }
    "general"
}
```

**Matching rules** (P4-B audit F1 — raw `contains` misclassified "I don't
know" as negate via `no` ⊂ "know" and "针对…" as confirm via `对`):

- Input is lowercased first; classification order is significant (negate
  before confirm, so "不对" can never hit confirm's "对").
- **ASCII keywords** and **single-character CJK keywords** match only with
  non-word neighbors on both sides (word chars = `is_alphanumeric()`, which
  includes CJK — so "针对" does not contain a standalone "对").
- **Multi-character CJK keywords** remain substring matches: Chinese has no
  delimiter to anchor a boundary on, and "还有一个" must still hit "还有".
- Implementation: `feedback::classify_feedback` / `keyword_hit`.

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
    let weights = REVISION_WEIGHTS.get(feedback_type).expect("unknown feedback type");
    concept.evidence_alpha += weights.alpha;
    concept.evidence_beta += weights.beta;
    concept.confidence = concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);

    if concept.confidence < 0.40 &&
       (concept.status == ConceptStatus::Confirmed || concept.status == ConceptStatus::AutoConfirmed) {
        concept.status = ConceptStatus::Deprecated;
    } else if concept.confidence < 0.40 && concept.status == ConceptStatus::Candidate {
        if days_since(&concept.created_at) > 30 {
            concept.status = ConceptStatus::Deprecated;
        }
    }

    match feedback_type {
        "negate" => create_rejected_hypothesis(concept, feedback_text)?,
        "correct" => reject_old_and_create_new(concept, feedback_text)?,
        "supplement" => add_entities_from_feedback(concept, feedback_text)?,
        _ => {}
    }

    update_concept(concept)
}
```

## Quality Checklist

Before any concept enters `confirmed` or `auto_confirmed`:
- [ ] Has evidence with source_type NOT only "assistant_guess"
- [ ] fact_confidence > 0.75 (bayesian)
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

## Adversarial Validation

### Overview

A second LLM (Validator) challenges each extracted observation. This addresses the core risk: LLM extraction quality is the system's biggest uncertainty.

**Placement**: adversarial validation is a quality gate at **Promote** (low-frequency, cluster-threshold-met), not at **Extract** (high-frequency, every session). Running a second LLM call on every extraction doubles ingest cost/latency; running it at promotion pays the cost only when a candidate is about to become a stable Concept.

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

### Cost Control Strategies

| Strategy | Description | When to Use |
|----------|-------------|-------------|
| **Sampling** | Only validate obs with surprise_score > 0.5 | Default mode |
| **Caching** | Reuse validation prompts for similar session structures | Repeated session types |
| **Self-consistency** | Same LLM extracts 3×, keep consistent results | No validator LLM available |
| **Skip small sessions** | Only validate sessions with >10 observations | Cost-sensitive environments |

### Quality Gate

Adversarial validation runs as a quality gate between Cluster and Promote:

```
RawMemory → [Extractor LLM] → Observations → [Embed] → [Cluster] → Candidate
                                                                     ↓
                                              [Validator LLM] → accepted/adjusted/rejected → Promote
```
