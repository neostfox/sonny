use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};

use crate::embed::traits::EmbeddingProvider;
use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::concept::Concept;
use crate::models::embedding::EmbeddingSourceType;
use crate::models::recall::{Intent, MemoryContext, RecallBudget, RecallScore};
use crate::models::status::ConceptStatus;
use crate::store::traits::{ConceptStore, EmbeddingStore};

const DEFAULT_MAX_TOKENS: usize = 1500;
const SEMANTIC_WEIGHT: f64 = 0.6;
const ENTITY_WEIGHT: f64 = 0.4;
const SEMANTIC_TOP_K: usize = 10;
const SEMANTIC_THRESHOLD: f32 = 0.0;
/// P4-C: base forgetting half-life (mirrors `ConfidenceConfig::decay_half_life_days`).
const DEFAULT_DECAY_HALF_LIFE_DAYS: f64 = 90.0;
/// P4-C: each successful recall slows forgetting by this factor (spacing effect).
const REHEARSAL_DECELERATION: f64 = 0.5;

pub struct RecallEngine<'a, C, E, P>
where
    C: ConceptStore,
    E: EmbeddingStore,
    P: EmbeddingProvider,
{
    concepts: &'a C,
    embeddings: &'a E,
    provider: &'a P,
    decay_half_life_days: f64,
}

impl<'a, C, E, P> RecallEngine<'a, C, E, P>
where
    C: ConceptStore,
    E: EmbeddingStore,
    P: EmbeddingProvider,
{
    pub fn new(concepts: &'a C, embeddings: &'a E, provider: &'a P) -> Self {
        Self {
            concepts,
            embeddings,
            provider,
            decay_half_life_days: DEFAULT_DECAY_HALF_LIFE_DAYS,
        }
    }

    /// Override the base forgetting half-life (wire from
    /// `ConfidenceConfig::decay_half_life_days` when constructing from settings).
    pub fn with_decay_half_life(mut self, days: f64) -> Self {
        self.decay_half_life_days = days;
        self
    }

    pub async fn recall(&self, query: &str, workspace_id: &str) -> MemoryResult<MemoryContext> {
        self.recall_with_budget(query, workspace_id, DEFAULT_MAX_TOKENS)
            .await
    }

    pub async fn recall_with_budget(
        &self,
        query: &str,
        workspace_id: &str,
        max_tokens: usize,
    ) -> MemoryResult<MemoryContext> {
        let intent = classify_intent(query);
        let ranked = self.rank(query, workspace_id).await?;
        let concepts = self.load_ranked_concepts(&ranked)?;
        let mut context = build_context(workspace_id, &intent, &concepts, max_tokens);

        // P4-B closed loop: retrieval records only the attempt (count + clock).
        // Whether it was *successful* is decided later by explicit feedback via
        // `record_recall_outcome` — retrieval alone must not slow time decay.
        if context.current_concept.is_some() {
            for concept in concepts.iter().take(3) {
                self.concepts.record_recall(&concept.concept_id)?;
            }
        }

        context.token_count = estimate_tokens(&context.to_prompt());
        Ok(context)
    }

    pub async fn rank(&self, query: &str, workspace_id: &str) -> MemoryResult<Vec<RecallScore>> {
        let mut scores: HashMap<String, RecallScore> = HashMap::new();
        let query_vector = self.provider.embed(query).await?;
        let query_slice = query_vector.as_slice().unwrap_or(&[]);

        if !query_slice.is_empty() {
            for hit in self.embeddings.search(
                query_slice,
                workspace_id,
                SEMANTIC_TOP_K,
                SEMANTIC_THRESHOLD,
            )? {
                if hit.source_type == EmbeddingSourceType::Concept {
                    let entry = scores.entry(hit.source_id.clone()).or_insert(RecallScore {
                        concept_id: hit.source_id,
                        score: 0.0,
                        semantic_score: 0.0,
                        entity_score: 0.0,
                        recency: 1.0,
                    });
                    entry.semantic_score = entry.semantic_score.max(hit.score);
                }
            }
        }

        let query_entities = extract_query_entities(query);
        if !query_entities.is_empty() {
            for concept in self
                .concepts
                .find_by_entities(&query_entities, workspace_id)?
            {
                let overlap = entity_overlap(&query_entities, &concept);
                let entry = scores
                    .entry(concept.concept_id.clone())
                    .or_insert(RecallScore {
                        concept_id: concept.concept_id,
                        score: 0.0,
                        semantic_score: 0.0,
                        entity_score: 0.0,
                        recency: 1.0,
                    });
                entry.entity_score = entry.entity_score.max(overlap);
            }
        }

        // P4-C: recency is a multiplicative salience factor on the channel
        // score. It reads recall stats written AFTER ranking (rehearsal resets
        // the forgetting clock for the NEXT recall) and never touches α/β.
        let now = Utc::now();
        let mut ranked = Vec::with_capacity(scores.len());
        for mut score in scores.into_values() {
            score.recency = match self.concepts.get_concept(&score.concept_id)? {
                Some(concept) => concept_recency(&concept, self.decay_half_life_days, now),
                None => 1.0,
            };
            score.score = (score.semantic_score * SEMANTIC_WEIGHT
                + score.entity_score * ENTITY_WEIGHT)
                * score.recency;
            ranked.push(score);
        }
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(ranked)
    }

    fn load_ranked_concepts(&self, ranked: &[RecallScore]) -> MemoryResult<Vec<Concept>> {
        let mut concepts = Vec::with_capacity(ranked.len().min(5));
        for score in ranked.iter().take(5) {
            if let Some(concept) = self.concepts.get_concept(&score.concept_id)? {
                concepts.push(concept);
            }
        }
        Ok(concepts)
    }
}

/// P4-C: Ebbinghaus forgetting curve with rehearsal (spacing) effect.
///
/// `recency = exp(−λ_eff·Δt)` where
/// `λ_eff = (1 / half_life_days) / (1 + REHEARSAL_DECELERATION · successful_recall_count)`
/// and Δt is days since `anchor` (clamped at 0 for future timestamps).
///
/// Opaque or missing anchors yield 1.0 — legacy rows without parseable
/// timestamps are not penalized, they just don't decay.
pub fn recency_factor(
    anchor: Option<&str>,
    successful_recall_count: i64,
    half_life_days: f64,
    now: DateTime<Utc>,
) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let Some(parsed) = anchor.and_then(|a| DateTime::parse_from_rfc3339(a).ok()) else {
        return 1.0;
    };
    let days = (now - parsed.with_timezone(&Utc)).num_seconds().max(0) as f64 / 86_400.0;
    let lambda_eff = (1.0 / half_life_days)
        / (1.0 + REHEARSAL_DECELERATION * successful_recall_count.max(0) as f64);
    (-lambda_eff * days).exp()
}

/// Recency anchored on `last_recalled_at` (rehearsal resets the forgetting
/// clock), falling back to `created_at` for never-recalled concepts.
fn concept_recency(concept: &Concept, half_life_days: f64, now: DateTime<Utc>) -> f64 {
    let anchor = concept
        .last_recalled_at
        .as_deref()
        .unwrap_or(concept.created_at.as_str());
    recency_factor(
        Some(anchor),
        concept.successful_recall_count,
        half_life_days,
        now,
    )
}

pub fn classify_intent(query: &str) -> Intent {
    const TEMPLATES: &[(Intent, &[&str])] = &[
        (
            Intent::ContinueInvestigation,
            &[
                "继续看",
                "接着查",
                "还差",
                "接下来",
                "continue",
                "resume",
                "next",
            ],
        ),
        (
            Intent::VerifyFact,
            &[
                "确认",
                "是不是",
                "有没有",
                "verify",
                "confirm",
                "check",
                "whether",
            ],
        ),
        (
            Intent::CorrectMistake,
            &[
                "不对",
                "错了",
                "不是这个",
                "其实是",
                "wrong",
                "incorrect",
                "actually",
            ],
        ),
        (
            Intent::AddKnowledge,
            &["补充", "还有", "另外", "加上", "add", "also", "note"],
        ),
        (
            Intent::ReviewHistory,
            &[
                "之前",
                "上次",
                "历史",
                "原来",
                "previous",
                "history",
                "last time",
            ],
        ),
    ];

    let lower = query.to_lowercase();
    for (intent, keywords) in TEMPLATES {
        if keywords.iter().any(|keyword| lower.contains(keyword)) {
            return intent.clone();
        }
    }
    Intent::GeneralQuery
}

pub fn build_context(
    workspace_id: &str,
    intent: &Intent,
    concepts: &[Concept],
    max_tokens: usize,
) -> MemoryContext {
    let budget = RecallBudget::for_intent(intent, max_tokens);
    let mut context = MemoryContext {
        workspace: workspace_id.to_string(),
        current_concept: concepts.first().map(|concept| concept.name.clone()),
        intent: intent.as_str().to_string(),
        user_preferences: Vec::new(),
        known_facts: Vec::new(),
        rejected_hypotheses: Vec::new(),
        task_state: Vec::new(),
        relevant_entities: Vec::new(),
        token_count: 0,
    };

    for concept in concepts {
        push_concept_summary(&mut context, concept);
        push_json_items(&mut context.known_facts, &concept.known_facts_json);
        push_json_items(
            &mut context.rejected_hypotheses,
            &concept.rejected_hypotheses_json,
        );
        push_json_items(&mut context.task_state, &concept.open_questions_json);
        push_json_items(
            &mut context.relevant_entities,
            &concept.related_entities_json,
        );

        if concept
            .concept_type
            .as_ref()
            .is_some_and(|kind| kind == &crate::models::concept::ConceptType::Preference)
        {
            push_json_items(&mut context.user_preferences, &concept.known_facts_json);
        }
    }

    dedup(&mut context.user_preferences);
    dedup(&mut context.known_facts);
    dedup(&mut context.rejected_hypotheses);
    dedup(&mut context.task_state);
    dedup(&mut context.relevant_entities);

    truncate_section(&mut context.user_preferences, budget.user_preferences);
    truncate_section(&mut context.known_facts, budget.known_facts);
    truncate_section(&mut context.rejected_hypotheses, budget.rejected_hypotheses);
    truncate_section(&mut context.task_state, budget.task_state);
    truncate_section(&mut context.relevant_entities, budget.relevant_entities);
    enforce_total_budget(&mut context, budget.max_tokens);
    context.token_count = estimate_tokens(&context.to_prompt());
    context
}

fn push_concept_summary(context: &mut MemoryContext, concept: &Concept) {
    if concept.status == ConceptStatus::Disputed {
        context
            .rejected_hypotheses
            .push(format!("{} is disputed", concept.name));
    }
    if let Some(definition) = concept.definition.as_ref().filter(|s| !s.is_empty()) {
        context
            .known_facts
            .push(format!("{}: {}", concept.name, definition));
    }
}

fn push_json_items(out: &mut Vec<String>, json: &Option<String>) {
    let Some(raw) = json.as_deref().filter(|s| !s.is_empty()) else {
        return;
    };

    if let Ok(items) = serde_json::from_str::<Vec<String>>(raw) {
        out.extend(items.into_iter().filter(|item| !item.is_empty()));
    } else {
        out.push(raw.to_string());
    }
}

fn dedup(items: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

fn truncate_section(items: &mut Vec<String>, max_tokens: usize) {
    if max_tokens == 0 {
        items.clear();
        return;
    }

    let mut used = 0;
    let mut keep = 0;
    for item in items.iter() {
        let item_tokens = estimate_tokens(item).max(1);
        if used + item_tokens > max_tokens {
            break;
        }
        used += item_tokens;
        keep += 1;
    }
    items.truncate(keep);
}

fn enforce_total_budget(context: &mut MemoryContext, max_tokens: usize) {
    while estimate_tokens(&context.to_prompt()) > max_tokens {
        if pop_lowest_priority(context).is_none() {
            break;
        }
    }
}

fn pop_lowest_priority(context: &mut MemoryContext) -> Option<String> {
    context
        .relevant_entities
        .pop()
        .or_else(|| context.task_state.pop())
        .or_else(|| context.rejected_hypotheses.pop())
        .or_else(|| context.user_preferences.pop())
        .or_else(|| context.known_facts.pop())
}

pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

fn extract_query_entities(query: &str) -> Vec<String> {
    let mut entities = Vec::new();
    for raw in query
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c as u32 > 0x7f))
        .filter(|part| part.chars().count() >= 2)
    {
        let key = canonical_key_light(raw);
        if !key.is_empty() && !is_stopword(&key) {
            entities.push(key);
        }
    }
    dedup(&mut entities);
    entities
}

fn is_stopword(key: &str) -> bool {
    matches!(
        key,
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "what"
            | "when"
            | "where"
            | "why"
            | "how"
            | "确认"
            | "是不是"
            | "有没有"
            | "之前"
            | "上次"
            | "历史"
    )
}

fn entity_overlap(query_entities: &[String], concept: &Concept) -> f64 {
    let Some(raw) = concept.related_entities_json.as_deref() else {
        return 0.0;
    };
    let Ok(entities) = serde_json::from_str::<Vec<String>>(raw) else {
        return 0.0;
    };
    if entities.is_empty() {
        return 0.0;
    }

    let normalized: BTreeSet<_> = entities
        .iter()
        .map(|entity| canonical_key_light(entity))
        .collect();
    let matches = query_entities
        .iter()
        .filter(|entity| normalized.contains(*entity))
        .count();
    matches as f64 / normalized.len() as f64
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use ndarray::Array1;

    use super::*;
    use crate::models::concept::{Concept, ConceptType};
    use crate::models::embedding::EmbeddingSearchResult;
    use crate::store::concept_store::SqliteConceptStore;
    use crate::store::connection::Database;
    use crate::store::embedding_store::SqliteEmbeddingStore;

    struct StaticEmbedding;

    #[async_trait]
    impl EmbeddingProvider for StaticEmbedding {
        async fn embed(&self, text: &str) -> MemoryResult<Array1<f32>> {
            if text.contains("POSMASK") {
                Ok(Array1::from(vec![1.0, 0.0]))
            } else {
                Ok(Array1::from(vec![0.0, 1.0]))
            }
        }

        fn dim(&self) -> usize {
            2
        }

        async fn health_check(&self) -> MemoryResult<bool> {
            Ok(true)
        }

        fn name(&self) -> &str {
            "static"
        }
    }

    fn concept(id: &str, name: &str, entities: &[&str]) -> Concept {
        Concept {
            concept_id: id.to_string(),
            workspace_id: "ws".to_string(),
            name: name.to_string(),
            concept_type: None,
            definition: Some(format!("{name} definition")),
            related_entities_json: Some(serde_json::to_string(entities).unwrap()),
            known_facts_json: Some(
                serde_json::to_string(&[format!("{name} fact one"), format!("{name} fact two")])
                    .unwrap(),
            ),
            rejected_hypotheses_json: Some(
                serde_json::to_string(&[format!("{name} rejected")]).unwrap(),
            ),
            open_questions_json: Some(serde_json::to_string(&[format!("{name} next")]).unwrap()),
            evidence_json: None,
            confidence: 0.8,
            evidence_alpha: 4.0,
            evidence_beta: 1.0,
            status: ConceptStatus::Active,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            connection_count: 0,
            created_at: "2026-06-14T00:00:00Z".to_string(),
            updated_at: "2026-06-14T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn classify_intent_matches_core_keywords() {
        assert_eq!(
            classify_intent("继续看 POSMASK"),
            Intent::ContinueInvestigation
        );
        assert_eq!(
            classify_intent("确认 POSMASK 有没有机器字段"),
            Intent::VerifyFact
        );
        assert_eq!(
            classify_intent("不对，其实是 ORDERHDR"),
            Intent::CorrectMistake
        );
        assert_eq!(classify_intent("补充一个偏好"), Intent::AddKnowledge);
        assert_eq!(classify_intent("上次历史是什么"), Intent::ReviewHistory);
        assert_eq!(classify_intent("explain POSMASK"), Intent::GeneralQuery);
    }

    #[tokio::test]
    async fn recall_merges_semantic_and_entity_channels() {
        let db = Database::open_in_memory().unwrap();
        let concept_store = SqliteConceptStore::new(db.conn.clone());
        let embedding_store = SqliteEmbeddingStore::new(db.conn.clone());
        let embedding = StaticEmbedding;

        let posmask = concept("c-posmask", "POSMASK", &["posmask", "机器字段"]);
        let order = concept("c-order", "ORDERHDR", &["orderhdr"]);
        concept_store.insert_concept(&posmask).unwrap();
        concept_store.insert_concept(&order).unwrap();
        embedding_store
            .store_embedding("concept", "c-posmask", "ws", "POSMASK", &[1.0, 0.0])
            .unwrap();
        embedding_store
            .store_embedding("concept", "c-order", "ws", "ORDERHDR", &[0.0, 1.0])
            .unwrap();

        let engine = RecallEngine::new(&concept_store, &embedding_store, &embedding);
        let ranked = engine
            .rank("确认 POSMASK 有没有机器字段", "ws")
            .await
            .unwrap();

        assert_eq!(ranked[0].concept_id, "c-posmask");
        assert!(ranked[0].semantic_score > 0.9);
        assert!(ranked[0].entity_score > 0.0);
    }

    #[tokio::test]
    async fn recall_builds_context_with_effective_token_budget() {
        let db = Database::open_in_memory().unwrap();
        let concept_store = SqliteConceptStore::new(db.conn.clone());
        let embedding_store = SqliteEmbeddingStore::new(db.conn.clone());
        let embedding = StaticEmbedding;

        let mut posmask = concept("c-posmask", "POSMASK", &["posmask", "机器字段"]);
        posmask.known_facts_json = Some(
            serde_json::to_string(
                &(0..100)
                    .map(|i| format!("POSMASK detailed fact number {i}"))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
        concept_store.insert_concept(&posmask).unwrap();
        embedding_store
            .store_embedding("concept", "c-posmask", "ws", "POSMASK", &[1.0, 0.0])
            .unwrap();

        let engine = RecallEngine::new(&concept_store, &embedding_store, &embedding);
        let context = engine
            .recall_with_budget("确认 POSMASK 有没有机器字段", "ws", 180)
            .await
            .unwrap();

        assert_eq!(context.intent, Intent::VerifyFact.as_str());
        assert_eq!(context.current_concept.as_deref(), Some("POSMASK"));
        assert!(context.token_count <= 180);
        assert!(!context.known_facts.is_empty());

        // P4-B: retrieval bumps only the attempt counter; success arrives via
        // explicit feedback, never from the retrieval itself.
        let recalled = concept_store.get_concept("c-posmask").unwrap().unwrap();
        assert_eq!(recalled.recall_count, 1);
        assert_eq!(recalled.successful_recall_count, 0);
        assert!(recalled.last_recalled_at.is_some());
    }

    #[test]
    fn verify_fact_budget_preserves_more_facts_than_task_state() {
        let facts = (0..20).map(|i| format!("fact {i}")).collect::<Vec<_>>();
        let tasks = (0..20).map(|i| format!("task {i}")).collect::<Vec<_>>();
        let mut test_concept = concept("c", "Concept", &["concept"]);
        test_concept.concept_type = Some(ConceptType::TaskState);
        test_concept.known_facts_json = Some(serde_json::to_string(&facts).unwrap());
        test_concept.open_questions_json = Some(serde_json::to_string(&tasks).unwrap());

        let context = build_context("ws", &Intent::VerifyFact, &[test_concept], 250);

        assert!(context.known_facts.len() > context.task_state.len());
        assert!(context.token_count <= 250);
    }

    #[test]
    fn recency_factor_follows_ebbinghaus_with_rehearsal() {
        let now = DateTime::parse_from_rfc3339("2026-07-04T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Fresh anchor → no decay.
        assert!((recency_factor(Some("2026-07-04T00:00:00Z"), 0, 90.0, now) - 1.0).abs() < 1e-9);
        // 90 days elapsed, never successfully recalled → e^-1.
        let stale = recency_factor(Some("2026-04-05T00:00:00Z"), 0, 90.0, now);
        assert!((stale - (-1.0f64).exp()).abs() < 1e-6);
        // Rehearsal slows forgetting: 2 successful recalls halve λ → e^-0.5.
        let rehearsed = recency_factor(Some("2026-04-05T00:00:00Z"), 2, 90.0, now);
        assert!((rehearsed - (-0.5f64).exp()).abs() < 1e-6);
        assert!(rehearsed > stale);
    }

    #[test]
    fn recency_factor_tolerates_opaque_or_missing_anchor() {
        let now = Utc::now();
        // Legacy fixtures carry opaque timestamps — no penalty, no decay.
        assert_eq!(recency_factor(Some("t0"), 0, 90.0, now), 1.0);
        assert_eq!(recency_factor(None, 0, 90.0, now), 1.0);
        // Degenerate half-life disables decay instead of dividing by zero.
        assert_eq!(recency_factor(Some("2026-01-01T00:00:00Z"), 0, 0.0, now), 1.0);
        // Future anchors clamp to zero elapsed time.
        let future = (now + chrono::Duration::days(30)).to_rfc3339();
        assert_eq!(recency_factor(Some(&future), 0, 90.0, now), 1.0);
    }

    #[tokio::test]
    async fn rank_downranks_long_unrecalled_concepts() {
        let db = Database::open_in_memory().unwrap();
        let concept_store = SqliteConceptStore::new(db.conn.clone());
        let embedding_store = SqliteEmbeddingStore::new(db.conn.clone());
        let embedding = StaticEmbedding;
        let now = Utc::now();

        // Identical semantic signal; only the forgetting clock differs.
        let mut fresh = concept("c-fresh", "POSMASK-fresh", &["e-fresh"]);
        fresh.created_at = now.to_rfc3339();
        let mut stale = concept("c-stale", "POSMASK-stale", &["e-stale"]);
        stale.created_at = (now - chrono::Duration::days(300)).to_rfc3339();
        let mut rehearsed = concept("c-rehearsed", "POSMASK-rehearsed", &["e-rehearsed"]);
        rehearsed.created_at = (now - chrono::Duration::days(300)).to_rfc3339();
        rehearsed.last_recalled_at = Some((now - chrono::Duration::days(1)).to_rfc3339());

        for c in [&fresh, &stale, &rehearsed] {
            concept_store.insert_concept(c).unwrap();
            embedding_store
                .store_embedding("concept", &c.concept_id, "ws", &c.name, &[1.0, 0.0])
                .unwrap();
        }

        let engine = RecallEngine::new(&concept_store, &embedding_store, &embedding);
        let ranked = engine.rank("explain POSMASK", "ws").await.unwrap();

        assert_eq!(ranked.len(), 3);
        // The 300-day-stale concept sinks to the bottom; a recent recall
        // (rehearsal) rescues an equally old concept.
        assert_eq!(ranked[2].concept_id, "c-stale");
        assert!(ranked[2].recency < 0.1);
        let fresh_score = ranked.iter().find(|s| s.concept_id == "c-fresh").unwrap();
        let rehearsed_score = ranked
            .iter()
            .find(|s| s.concept_id == "c-rehearsed")
            .unwrap();
        assert!(fresh_score.recency > 0.95);
        assert!(rehearsed_score.recency > 0.95);
        assert!(fresh_score.score > ranked[2].score);
    }

    #[test]
    fn query_entity_extraction_normalizes_and_deduplicates() {
        let entities = extract_query_entities("确认 POSMASK posmask 有没有 机器字段");

        assert_eq!(
            entities,
            vec!["posmask".to_string(), "机器字段".to_string()]
        );
    }

    #[test]
    fn embedding_hits_ignore_non_concept_sources() {
        let mut scores = vec![EmbeddingSearchResult {
            source_id: "obs".to_string(),
            source_type: EmbeddingSourceType::Observation,
            score: 1.0,
        }];
        scores.retain(|hit| hit.source_type == EmbeddingSourceType::Concept);

        assert!(scores.is_empty());
    }
}
