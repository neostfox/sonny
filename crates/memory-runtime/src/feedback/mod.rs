//! P4-B feedback engine — closes the recall loop (D9).
//!
//! Recall (P4-A) hands memories to the user and records only the *attempt*
//! (`record_recall`); this module resolves each attempt from the user's
//! explicit reaction. Classification is keyword-based per
//! quality-control.md §Feedback Classification; revision weights live on
//! [`FeedbackType::revision_weights`]. Positive feedback (Confirm /
//! Supplement / Preference) counts a successful recall — which is what slows
//! time decay (P4-C) — and negative feedback (Negate / Correct) counts a
//! failed one, so a hot-but-wrong concept no longer strengthens itself just
//! by being retrieved.
//!
//! Persistence is atomic: the concept revision, recall-outcome counter, any
//! observation negate/correct write, and the ledger row all commit in ONE
//! SQLite transaction (`apply_feedback`), so a crash mid-apply can never leave
//! the concept revised but the ledger short, nor a superseded observation
//! without its replacement.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::Connection;

use crate::confidence::{BetaConfidence, EvidenceType};
use crate::entity::canonical_key_light;
use crate::error::{MemoryError, MemoryResult};
use crate::models::concept::{Concept, ConceptType};
use crate::models::feedback::{Feedback, FeedbackResult, FeedbackType};
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::status::{ConceptStatus, ObservationStatus};
use crate::store::concept_store::{
    get_concept_conn, record_recall_outcome_conn, update_concept_conn,
};
use crate::store::feedback_store::insert_feedback_conn;
use crate::store::observation_store::{
    get_observation_conn, insert_observation, supersede_observation_conn,
    update_observation_confidence_conn,
};
use crate::text::keyword_hit;

/// Below this confidence a revised concept is deprecated (quality-control.md
/// §Confidence Status After Revision).
const DEPRECATION_THRESHOLD: f64 = 0.40;
/// A low-confidence Candidate must be at least this old before deprecation.
const CANDIDATE_GRACE_DAYS: f64 = 30.0;

/// The observation-side write a feedback entails, computed in the pure phase
/// and applied inside the transaction.
enum ObsWrite {
    None,
    /// Negate: drop the target observation's Beta evidence to `(alpha, beta)`.
    Confidence {
        id: String,
        alpha: f64,
        beta: f64,
    },
    /// Correct: supersede `old_id` with a fresh `user_confirm` observation.
    Supersede {
        old_id: String,
        replacement: Box<Observation>,
    },
}

/// P4-B feedback engine. Owns the shared connection directly (rather than the
/// store traits) so `apply_feedback` can wrap all of its writes in a single
/// transaction; the per-table SQL is reused from the stores' `*_conn` helpers.
pub struct FeedbackEngine {
    conn: Arc<Mutex<Connection>>,
}

impl FeedbackEngine {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Apply one piece of user feedback to a recalled concept.
    ///
    /// - classifies `feedback_text`, applies the revision weights to the
    ///   concept's Beta evidence, and runs the status-transition rule;
    /// - Negate: appends the text to `rejected_hypotheses_json` (negative
    ///   information is high-value memory) and, given a target observation,
    ///   drops its fact confidence by the UserNegation weight;
    /// - Correct: given a target observation, supersedes it with a fresh
    ///   `user_confirm` observation carrying the correction (P2-D mechanism);
    /// - Supplement: merges entities extracted from the text into the concept;
    /// - Preference: types an untyped concept as Preference and stores the
    ///   text as a known fact (recall surfaces it under user_preferences);
    /// - resolves the pending recall outcome (success/failure counters);
    /// - appends the event to the feedback ledger.
    ///
    /// Reads happen first, then a pure computation, then every write commits in
    /// one transaction — all-or-nothing.
    pub fn apply_feedback(
        &self,
        workspace_id: &str,
        concept_id: &str,
        feedback_text: &str,
        target_observation_id: Option<&str>,
    ) -> MemoryResult<FeedbackResult> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // ---- Read phase: load the concept (+ target observation) ----
        let (mut concept, target_obs) = {
            let conn = self.conn.lock();
            let concept = get_concept_conn(&conn, concept_id)?
                .filter(|c| c.workspace_id == workspace_id)
                .ok_or_else(|| MemoryError::ConceptNotFound {
                    concept_id: concept_id.to_string(),
                })?;
            let target_obs = match target_observation_id {
                Some(id) => Some(get_observation_conn(&conn, id)?.ok_or_else(|| {
                    MemoryError::ObservationNotFound {
                        observation_id: id.to_string(),
                    }
                })?),
                None => None,
            };
            (concept, target_obs)
        };

        // ---- Pure phase: compute the new concept, observation op, ledger row ----
        let feedback_type = classify_feedback(feedback_text);
        let (alpha_delta, beta_delta) = feedback_type.revision_weights();

        concept.evidence_alpha += alpha_delta;
        concept.evidence_beta += beta_delta;
        concept.confidence =
            concept.evidence_alpha / (concept.evidence_alpha + concept.evidence_beta);
        let status_changed = apply_status_rule(&mut concept, now);

        let mut obs_write = ObsWrite::None;
        match feedback_type {
            FeedbackType::Negate => {
                append_json_item(&mut concept.rejected_hypotheses_json, feedback_text);
                if let Some(obs) = &target_obs {
                    let mut evidence =
                        BetaConfidence::with_values(obs.evidence_alpha, obs.evidence_beta);
                    evidence.update(&EvidenceType::UserNegation);
                    obs_write = ObsWrite::Confidence {
                        id: obs.observation_id.clone(),
                        alpha: evidence.alpha,
                        beta: evidence.beta,
                    };
                }
            }
            FeedbackType::Correct => {
                if let Some(obs) = &target_obs {
                    obs_write = ObsWrite::Supersede {
                        old_id: obs.observation_id.clone(),
                        replacement: Box::new(build_correction(obs, feedback_text, &now_str)),
                    };
                }
            }
            FeedbackType::Supplement => {
                merge_entities(&mut concept, feedback_text);
            }
            FeedbackType::Preference => {
                if concept.concept_type.is_none() {
                    concept.concept_type = Some(ConceptType::Preference);
                }
                append_json_item(&mut concept.known_facts_json, feedback_text);
            }
            FeedbackType::Confirm | FeedbackType::General => {}
        }
        concept.updated_at = now_str.clone();

        let outcome = feedback_type.recall_outcome();
        let feedback_row = Feedback {
            feedback_id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            concept_id: concept_id.to_string(),
            observation_id: target_observation_id.map(str::to_string),
            feedback_type: feedback_type.clone(),
            feedback_text: feedback_text.to_string(),
            alpha_delta,
            beta_delta,
            created_at: now_str,
        };

        // ---- Write phase: everything commits together or not at all ----
        {
            let mut guard = self.conn.lock();
            let tx = guard.transaction()?;
            update_concept_conn(&tx, &concept)?;
            // Closed loop: the explicit reaction — not the retrieval itself —
            // decides whether the recall counts as successful (P4-C audit fix).
            if let Some(success) = outcome {
                record_recall_outcome_conn(&tx, concept_id, success)?;
            }
            match &obs_write {
                ObsWrite::None => {}
                ObsWrite::Confidence { id, alpha, beta } => {
                    update_observation_confidence_conn(&tx, id, *alpha, *beta)?;
                }
                ObsWrite::Supersede {
                    old_id,
                    replacement,
                } => {
                    insert_observation(&tx, replacement)?;
                    supersede_observation_conn(&tx, old_id, &replacement.observation_id)?;
                }
            }
            insert_feedback_conn(&tx, &feedback_row)?;
            tx.commit()?;
        }

        Ok(FeedbackResult {
            feedback_type,
            concept_id: concept_id.to_string(),
            new_confidence: concept.confidence,
            status_changed,
            new_status: status_changed.then(|| concept.status.as_str().to_string()),
        })
    }
}

/// Build the `user_confirm` replacement observation for a Correct feedback
/// (P2-D single-observation analog of `reextract`): same subject/predicate as
/// the superseded row, the correction as object, evidence seeded from the
/// UserConfirm source.
fn build_correction(old: &Observation, feedback_text: &str, now: &str) -> Observation {
    let source_type = ObservationSourceType::UserConfirm;
    let mut evidence = BetaConfidence::new();
    if let Some(et) = source_type.initial_evidence() {
        evidence.update(&et);
    }
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: old.workspace_id.clone(),
        memory_id: old.memory_id.clone(),
        subject_text: old.subject_text.clone(),
        subject_type: old.subject_type.clone(),
        predicate: old.predicate.clone(),
        object_text: Some(feedback_text.to_string()),
        object_type: None,
        evidence_text: Some(feedback_text.to_string()),
        extraction_confidence: source_type.extraction_confidence(),
        evidence_alpha: evidence.alpha,
        evidence_beta: evidence.beta,
        status: ObservationStatus::Candidate,
        surprise_score: old.surprise_score,
        source_type,
        consolidated: false,
        memory_type_candidate: old.memory_type_candidate.clone(),
        observation_detail_json: None,
        extraction_batch_id: None,
        superseded_by: None,
        created_at: now.to_string(),
    }
}

/// Keyword classification (quality-control.md §Feedback Classification), with
/// a preference branch appended — the spec's revision table has a
/// `preference` type but its classifier sketch omits it. Order matters:
/// negate wins over confirm so "不对" never matches confirm's "对".
///
/// Matching is boundary-aware, not raw substring (P4-B audit F1): ASCII
/// keywords and single-character CJK keywords require non-word neighbors, so
/// "know" no longer negates via `no` and "针对" no longer confirms via `对`.
/// Multi-character CJK keywords stay substring — Chinese has no delimiter to
/// anchor a word boundary on ("还有一个" must still hit "还有").
pub fn classify_feedback(text: &str) -> FeedbackType {
    const TABLES: &[(FeedbackType, &[&str])] = &[
        (
            FeedbackType::Negate,
            &["不对", "错了", "不是", "不正确", "no", "wrong", "不是这个"],
        ),
        (
            FeedbackType::Confirm,
            &["对", "没错", "正确", "是的", "yes", "right", "就是这个"],
        ),
        (
            FeedbackType::Supplement,
            &["还有", "补充", "另外", "加上", "also", "and"],
        ),
        (
            FeedbackType::Correct,
            &["应该是", "其实是", "实际上是", "actually", "should be"],
        ),
        (
            FeedbackType::Preference,
            &["偏好", "以后都", "我喜欢", "prefer", "i like", "always use"],
        ),
    ];

    let lower = text.to_lowercase();
    for (feedback_type, keywords) in TABLES {
        if keywords.iter().any(|kw| keyword_hit(&lower, kw)) {
            return feedback_type.clone();
        }
    }
    FeedbackType::General
}

/// Status transition after revision (quality-control.md, adapted to the code's
/// `ConceptStatus` vocabulary: Active/Labile play the spec's Confirmed roles).
fn apply_status_rule(concept: &mut Concept, now: DateTime<Utc>) -> bool {
    if concept.confidence >= DEPRECATION_THRESHOLD {
        return false;
    }
    let deprecate = match concept.status {
        ConceptStatus::Active | ConceptStatus::Labile => true,
        ConceptStatus::Candidate => days_since(&concept.created_at, now) > CANDIDATE_GRACE_DAYS,
        ConceptStatus::Deprecated | ConceptStatus::Disputed => false,
    };
    if deprecate {
        concept.status = ConceptStatus::Deprecated;
    }
    deprecate
}

fn days_since(created_at: &str, now: DateTime<Utc>) -> f64 {
    DateTime::parse_from_rfc3339(created_at)
        .map(|t| (now - t.with_timezone(&Utc)).num_seconds().max(0) as f64 / 86_400.0)
        .unwrap_or(0.0)
}

/// Append `item` to a JSON string-array column, tolerating empty/opaque values.
fn append_json_item(json: &mut Option<String>, item: &str) {
    let mut items: Vec<String> = json
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    if !items.iter().any(|existing| existing == item) {
        items.push(item.to_string());
    }
    *json = serde_json::to_string(&items).ok().or_else(|| json.take());
}

/// Supplement: merge entities mentioned in the feedback into the concept.
/// Stored in canonical form — `update_concept` re-syncs the `entity_concept`
/// join table verbatim from this column, and recall's entity channel looks up
/// canonical keys.
fn merge_entities(concept: &mut Concept, feedback_text: &str) {
    let mut entities: Vec<String> = concept
        .related_entities_json
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    let known: std::collections::BTreeSet<String> =
        entities.iter().map(|e| canonical_key_light(e)).collect();

    for raw in feedback_text
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c as u32 > 0x7f))
        .filter(|part| part.chars().count() >= 2)
    {
        let key = canonical_key_light(raw);
        if !key.is_empty() && !is_feedback_stopword(&key) && !known.contains(&key) {
            entities.push(key);
        }
    }
    if !entities.is_empty() {
        concept.related_entities_json = serde_json::to_string(&entities).ok();
    }
}

/// Classification trigger words and glue that must not become entities.
fn is_feedback_stopword(key: &str) -> bool {
    matches!(
        key,
        "还有" | "补充" | "另外" | "加上" | "also" | "and" | "the" | "for" | "with" | "this"
            | "that" | "还要" | "以及"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::concept_store::SqliteConceptStore;
    use crate::store::connection::Database;
    use crate::store::feedback_store::SqliteFeedbackStore;
    use crate::store::observation_store::SqliteObservationStore;
    use crate::store::traits::{ConceptStore, FeedbackStore, ObservationStore};

    fn concept(id: &str, status: ConceptStatus, alpha: f64, beta: f64) -> Concept {
        Concept {
            concept_id: id.to_string(),
            workspace_id: "ws".to_string(),
            name: format!("concept-{id}"),
            concept_type: None,
            definition: None,
            related_entities_json: Some(r#"["posmask"]"#.to_string()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: alpha / (alpha + beta),
            evidence_alpha: alpha,
            evidence_beta: beta,
            status,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            connection_count: 0,
            created_at: "2026-06-01T00:00:00Z".to_string(),
            updated_at: "2026-06-01T00:00:00Z".to_string(),
        }
    }

    fn observation(id: &str) -> Observation {
        Observation {
            observation_id: id.to_string(),
            workspace_id: "ws".to_string(),
            memory_id: "m1".to_string(),
            subject_text: "POSMASK".to_string(),
            subject_type: None,
            predicate: "has_field".to_string(),
            object_text: Some("机器字段".to_string()),
            object_type: None,
            evidence_text: None,
            extraction_confidence: 0.7,
            evidence_alpha: 1.0,
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: ObservationSourceType::UserMessage,
            consolidated: false,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: None,
            superseded_by: None,
            created_at: "2026-06-01T00:00:00Z".to_string(),
        }
    }

    struct Fixture {
        db: Database,
    }

    impl Fixture {
        fn new() -> Self {
            let db = Database::open_in_memory().unwrap();
            // Observations reference raw_memory(memory_id); seed the parent row.
            db.conn
                .lock()
                .execute(
                    "INSERT INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
                     VALUES ('m1', 'ws', 's1', 'user', 'seed', 'session_file', 't', '2026-06-01T00:00:00Z')",
                    [],
                )
                .unwrap();
            Self { db }
        }
        fn concepts(&self) -> SqliteConceptStore {
            SqliteConceptStore::new(self.db.conn.clone())
        }
        fn observations(&self) -> SqliteObservationStore {
            SqliteObservationStore::new(self.db.conn.clone())
        }
        fn feedback(&self) -> SqliteFeedbackStore {
            SqliteFeedbackStore::new(self.db.conn.clone())
        }
    }

    #[test]
    fn classify_feedback_matches_spec_keywords() {
        assert_eq!(classify_feedback("不对，错了"), FeedbackType::Negate);
        assert_eq!(classify_feedback("没错，正确"), FeedbackType::Confirm);
        assert_eq!(classify_feedback("还有一个字段"), FeedbackType::Supplement);
        assert_eq!(
            classify_feedback("应该是 PostgreSQL"),
            FeedbackType::Correct
        );
        assert_eq!(classify_feedback("我喜欢用 rg"), FeedbackType::Preference);
        assert_eq!(classify_feedback("嗯，继续"), FeedbackType::General);
        // Negate precedence: "不对" must not fall through to confirm's "对".
        assert_eq!(classify_feedback("不对"), FeedbackType::Negate);
        assert_eq!(classify_feedback("That is WRONG"), FeedbackType::Negate);
    }

    #[test]
    fn classify_feedback_requires_word_boundaries() {
        // F1: ASCII keywords must not match inside larger words.
        assert_eq!(classify_feedback("I don't know"), FeedbackType::General); // no ⊄ know
        assert_eq!(classify_feedback("I understand"), FeedbackType::General); // and ⊄ understand
        assert_eq!(classify_feedback("take notes"), FeedbackType::General); // no ⊄ notes
        assert_eq!(classify_feedback("no, that's it"), FeedbackType::Negate);
        assert_eq!(classify_feedback("A and B"), FeedbackType::Supplement);
        // F1 follow-up: `_`/`-` are word chars, so identifier-like tokens don't
        // split into keywords — "no" must not negate inside these.
        assert_eq!(classify_feedback("set no_cache = true"), FeedbackType::General);
        assert_eq!(classify_feedback("it's a no-op here"), FeedbackType::General);
        // F1: single-char CJK keyword "对" needs non-word neighbors.
        assert_eq!(classify_feedback("针对这个再查一下"), FeedbackType::General);
        assert_eq!(classify_feedback("对，就是这个"), FeedbackType::Confirm);
        assert_eq!(classify_feedback("对"), FeedbackType::Confirm);
        // Multi-char CJK keywords still match without delimiters.
        assert_eq!(classify_feedback("还有一个字段"), FeedbackType::Supplement);
        // Multi-word ASCII keywords bound at the phrase edges.
        assert_eq!(classify_feedback("it should be utf-8"), FeedbackType::Correct);
    }

    #[test]
    fn confirm_raises_confidence_and_counts_successful_recall() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        let feedback = fx.feedback();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let result = engine.apply_feedback("ws", "c1", "没错，就是这个", None).unwrap();

        assert_eq!(result.feedback_type, FeedbackType::Confirm);
        assert!((result.new_confidence - 0.75).abs() < 1e-9); // (1+2)/(1+2+1)
        assert!(!result.status_changed);

        let updated = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(updated.evidence_alpha, 3.0);
        assert_eq!(updated.successful_recall_count, 1);
        assert_eq!(updated.failed_recall_count, 0);

        let ledger = feedback.list_by_concept("ws", "c1").unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].alpha_delta, 2.0);
    }

    #[test]
    fn negate_records_rejected_hypothesis_and_failed_recall() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        let observations = fx.observations();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();
        observations.insert(&observation("o1")).unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let result = engine
            .apply_feedback("ws", "c1", "不对，POSMASK 没有机器字段", Some("o1"))
            .unwrap();

        assert_eq!(result.feedback_type, FeedbackType::Negate);
        // Concept: beta += 3 → 1/(1+4) = 0.20 < 0.40 → Active deprecates.
        assert!((result.new_confidence - 0.20).abs() < 1e-9);
        assert!(result.status_changed);
        assert_eq!(result.new_status.as_deref(), Some("deprecated"));

        let updated = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(updated.status, ConceptStatus::Deprecated);
        let rejected: Vec<String> =
            serde_json::from_str(updated.rejected_hypotheses_json.as_deref().unwrap()).unwrap();
        assert_eq!(rejected, vec!["不对，POSMASK 没有机器字段".to_string()]);
        assert_eq!(updated.failed_recall_count, 1);
        assert_eq!(updated.successful_recall_count, 0);

        // Observation: UserNegation weight (0, +3) drops fact confidence.
        let obs = observations.get("o1").unwrap().unwrap();
        assert_eq!(obs.evidence_beta, 4.0);
        assert!(obs.fact_confidence() < 0.5);
    }

    #[test]
    fn correct_supersedes_observation_with_user_confirm_replacement() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        let observations = fx.observations();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 4.0, 1.0))
            .unwrap();
        observations.insert(&observation("o1")).unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let result = engine
            .apply_feedback("ws", "c1", "其实是金额字段", Some("o1"))
            .unwrap();

        assert_eq!(result.feedback_type, FeedbackType::Correct);

        let old = observations.get("o1").unwrap().unwrap();
        assert_eq!(old.status, ObservationStatus::Superseded);
        let replacement_id = old.superseded_by.expect("superseded_by must point at replacement");
        let replacement = observations.get(&replacement_id).unwrap().unwrap();
        assert_eq!(replacement.source_type, ObservationSourceType::UserConfirm);
        assert_eq!(replacement.subject_text, "POSMASK");
        assert_eq!(replacement.predicate, "has_field");
        assert_eq!(replacement.object_text.as_deref(), Some("其实是金额字段"));
        // UserConfirm seeds UserConfirmation evidence: Beta(3, 1).
        assert_eq!(replacement.evidence_alpha, 3.0);

        let updated = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(updated.failed_recall_count, 1);
    }

    #[test]
    fn supplement_merges_new_entities_and_counts_success() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        engine
            .apply_feedback("ws", "c1", "还有 ORDERHDR 也相关", None)
            .unwrap();

        let updated = concepts.get_concept("c1").unwrap().unwrap();
        let entities: Vec<String> =
            serde_json::from_str(updated.related_entities_json.as_deref().unwrap()).unwrap();
        assert!(entities.contains(&"posmask".to_string()));
        // Merged in canonical form so the entity_concept sync and recall's
        // canonical lookup agree.
        assert!(entities.contains(&"orderhdr".to_string()));
        assert_eq!(updated.successful_recall_count, 1);
        // Entity channel picks the merged entity up immediately.
        let found = concepts
            .find_by_entities(&["orderhdr".to_string()], "ws")
            .unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn preference_types_concept_and_stores_fact() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        engine
            .apply_feedback("ws", "c1", "我喜欢用 ripgrep 搜索", None)
            .unwrap();

        let updated = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(updated.concept_type, Some(ConceptType::Preference));
        let facts: Vec<String> =
            serde_json::from_str(updated.known_facts_json.as_deref().unwrap()).unwrap();
        assert_eq!(facts, vec!["我喜欢用 ripgrep 搜索".to_string()]);
    }

    #[test]
    fn general_feedback_changes_nothing_but_is_ledgered() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        let feedback = fx.feedback();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 2.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let result = engine.apply_feedback("ws", "c1", "嗯，继续吧", None).unwrap();

        assert_eq!(result.feedback_type, FeedbackType::General);
        let updated = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(updated.evidence_alpha, 2.0);
        assert_eq!(updated.successful_recall_count, 0);
        assert_eq!(updated.failed_recall_count, 0);
        assert_eq!(feedback.list_by_concept("ws", "c1").unwrap().len(), 1);
    }

    #[test]
    fn young_candidate_survives_negation_but_old_one_deprecates() {
        let fx = Fixture::new();
        let concepts = fx.concepts();

        let mut young = concept("young", ConceptStatus::Candidate, 1.0, 1.0);
        young.created_at = Utc::now().to_rfc3339();
        concepts.insert_concept(&young).unwrap();
        let mut old = concept("old", ConceptStatus::Candidate, 1.0, 1.0);
        old.created_at = (Utc::now() - chrono::Duration::days(45)).to_rfc3339();
        concepts.insert_concept(&old).unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let young_result = engine.apply_feedback("ws", "young", "不对", None).unwrap();
        let old_result = engine.apply_feedback("ws", "old", "不对", None).unwrap();

        assert!(!young_result.status_changed);
        assert!(old_result.status_changed);
        assert_eq!(
            concepts.get_concept("old").unwrap().unwrap().status,
            ConceptStatus::Deprecated
        );
    }

    #[test]
    fn wrong_workspace_is_concept_not_found() {
        let fx = Fixture::new();
        let concepts = fx.concepts();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let err = engine.apply_feedback("other-ws", "c1", "没错", None);
        assert!(matches!(
            err,
            Err(MemoryError::ConceptNotFound { .. })
        ));
    }

    #[test]
    fn failed_feedback_leaves_no_partial_state() {
        // F2 atomicity: a Negate whose target observation is missing must abort
        // with nothing written — the concept keeps its α/β and the ledger stays
        // empty (the read phase fails before the write transaction opens).
        let fx = Fixture::new();
        let concepts = fx.concepts();
        let feedback = fx.feedback();
        concepts
            .insert_concept(&concept("c1", ConceptStatus::Active, 1.0, 1.0))
            .unwrap();

        let engine = FeedbackEngine::new(fx.db.conn.clone());
        let err = engine.apply_feedback("ws", "c1", "不对", Some("ghost-obs"));
        assert!(matches!(err, Err(MemoryError::ObservationNotFound { .. })));

        let unchanged = concepts.get_concept("c1").unwrap().unwrap();
        assert_eq!(unchanged.evidence_alpha, 1.0);
        assert_eq!(unchanged.evidence_beta, 1.0);
        assert_eq!(unchanged.failed_recall_count, 0);
        assert!(unchanged.rejected_hypotheses_json.is_none());
        assert!(feedback.list_by_concept("ws", "c1").unwrap().is_empty());
    }
}
