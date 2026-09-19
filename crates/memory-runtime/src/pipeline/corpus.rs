//! Offline corpus seed loader (G1).
//!
//! Loads pre-structured knowledge bundles extracted from real project material
//! (schedule POC docs, boemenhu HarmonyOS debug notes, Sonny design notes).
//! No LLM required — enables end-to-end CLI smoke tests and demo workspaces.

use serde::{Deserialize, Serialize};

use crate::error::{MemoryError, MemoryResult};
use crate::models::concept::Concept;
use crate::models::hierarchy::RelationType;
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::relation::RelationLifecycle;
use crate::models::scope::LifecycleScope;
use crate::models::status::{ConceptStatus, ObservationStatus};
use crate::store::concept_store::SqliteConceptStore;
use crate::store::observation_store::SqliteObservationStore;
use crate::store::relation_store::SqliteRelationStore;
use crate::store::traits::{ConceptStore, ObservationStore, RawMemoryStore, RelationStore};
use crate::store::connection::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedObservation {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: Option<String>,
    pub evidence: String,
    #[serde(default = "default_ws")]
    pub workspace: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_role")]
    pub causal_role: Option<String>,
}

fn default_ws() -> String {
    "default".into()
}
fn default_source() -> String {
    "file_evidence".into()
}
fn default_role() -> Option<String> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedConcept {
    pub id: String,
    pub name: String,
    pub definition: Option<String>,
    pub entities: Vec<String>,
    #[serde(default = "default_ws")]
    pub workspace: String,
    #[serde(default = "default_scope")]
    pub lifecycle_scope: String,
    pub scope_key: Option<String>,
    #[serde(default = "default_conf")]
    pub confidence: f64,
}

fn default_scope() -> String {
    "project".into()
}
fn default_conf() -> f64 {
    0.85
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedRelation {
    pub src: String,
    pub dst: String,
    #[serde(default = "default_rel")]
    pub relation_type: String,
    #[serde(default = "default_ws")]
    pub workspace: String,
}

fn default_rel() -> String {
    "causal".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeedBundle {
    #[serde(default)]
    pub observations: Vec<SeedObservation>,
    #[serde(default)]
    pub concepts: Vec<SeedConcept>,
    #[serde(default)]
    pub relations: Vec<SeedRelation>,
}

#[derive(Debug, Clone, Default)]
pub struct SeedReport {
    pub observations: usize,
    pub concepts: usize,
    pub relations: usize,
}

pub fn load_bundle(path: &std::path::Path) -> MemoryResult<SeedBundle> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| {
        MemoryError::Config(format!("parse seed bundle {}: {e}", path.display()))
    })
}

pub fn apply_bundle(db: &Database, bundle: &SeedBundle) -> MemoryResult<SeedReport> {
    let raw = crate::store::raw_memory_store::SqliteRawMemoryStore::new(db.conn.clone());
    let obs_store = SqliteObservationStore::new(db.conn.clone());
    let concepts = SqliteConceptStore::new(db.conn.clone());
    let relations = SqliteRelationStore::new(db.conn.clone());
    let mut report = SeedReport::default();
    let now = chrono::Utc::now().to_rfc3339();

    for o in &bundle.observations {
        let memory_id = format!("seed-{}", o.id);
        let raw_mem = crate::models::raw_memory::RawMemory {
            memory_id: memory_id.clone(),
            workspace_id: o.workspace.clone(),
            session_id: format!("seed-{}", o.workspace),
            role: "system".into(),
            content: o.evidence.clone(),
            source_type: crate::models::raw_memory::SourceType::Manual,
            source_ref: "corpus-seed".into(),
            extraction_version: Some("seed.v1".into()),
            created_at: now.clone(),
        };
        // Parent row may already exist on re-seed.
        let _ = raw.insert(&raw_mem);

        let source = match o.source.as_str() {
            "user_confirm" => ObservationSourceType::UserConfirm,
            "user_negation" => ObservationSourceType::UserNegation,
            "assistant_guess" => ObservationSourceType::AssistantGuess,
            "user_message" => ObservationSourceType::UserMessage,
            _ => ObservationSourceType::FileEvidence,
        };
        let mut evidence = crate::confidence::BetaConfidence::new();
        if let Some(et) = source.initial_evidence() {
            evidence.update(&et);
        }
        // Seed corpus is verified project material — extra file-evidence mass.
        evidence.update(&crate::confidence::EvidenceType::FileEvidence);

        let obs = Observation {
            observation_id: format!("obs-{}", o.id),
            workspace_id: o.workspace.clone(),
            memory_id,
            subject_text: o.subject.clone(),
            subject_type: None,
            predicate: o.predicate.clone(),
            object_text: o.object.clone(),
            object_type: None,
            evidence_text: Some(o.evidence.clone()),
            extraction_confidence: source.extraction_confidence(),
            evidence_alpha: evidence.alpha,
            evidence_beta: evidence.beta,
            status: ObservationStatus::Confirmed,
            surprise_score: 0.4,
            source_type: source,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: Some(format!("seed-batch-{}", o.workspace)),
            superseded_by: None,
            cross_project_count: 1,
            causal_role: o.causal_role.clone(),
            consolidated: false,
            created_at: now.clone(),
        };
        match obs_store.get(&obs.observation_id)? {
            Some(_) => {}
            None => {
                obs_store.insert(&obs)?;
                report.observations += 1;
            }
        }
        // Timeline + cross-project sync
        if let Err(err) = crate::pipeline::timeline::record_from_observation(
            &crate::store::timeline_store::SqliteTimelineStore::new(db.conn.clone()),
            &obs,
            &now,
        ) {
            tracing::debug!(%err, "seed timeline skip");
        }
        let _ = obs_store.sync_cross_project_count(&obs.subject_text, &obs.predicate, obs.object_text.as_deref());
    }

    for c in &bundle.concepts {
        let scope = match c.lifecycle_scope.as_str() {
            "global" => LifecycleScope::Global,
            "domain" => LifecycleScope::Domain,
            _ => LifecycleScope::Project,
        };
        let concept = Concept {
            concept_id: c.id.clone(),
            workspace_id: c.workspace.clone(),
            name: c.name.clone(),
            concept_type: None,
            definition: c.definition.clone(),
            related_entities_json: Some(serde_json::to_string(&c.entities).unwrap_or_default()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: c.confidence,
            evidence_alpha: 5.0,
            evidence_beta: 1.0,
            status: ConceptStatus::Active,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 1,
            successful_recall_count: 1,
            failed_recall_count: 0,
            connection_count: 0,
            lifecycle_scope: scope,
            scope_key: c.scope_key.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        match concepts.get_concept(&concept.concept_id)? {
            Some(_) => {}
            None => {
                concepts.insert_concept(&concept)?;
                report.concepts += 1;
            }
        }
    }

    for r in &bundle.relations {
        let rt = match r.relation_type.as_str() {
            "shared_entity" => RelationType::SharedEntity,
            "shared_session" => RelationType::SharedSession,
            "temporal" => RelationType::Temporal,
            "embedding_similarity" => RelationType::EmbeddingSimilarity,
            _ => RelationType::Causal,
        };
        match relations.get_edge(&r.workspace, &r.src, &r.dst, rt)? {
            Some(_) => {}
            None => {
                let mut edge = relations.record_causal_evidence(
                    if rt == RelationType::Causal {
                        &r.workspace
                    } else {
                        &r.workspace
                    },
                    &r.src,
                    &r.dst,
                    &crate::confidence::EvidenceType::HumanReviewConfirm,
                    Some(ObservationSourceType::FileEvidence),
                    1,
                    &crate::models::causal::CausalStats::default(),
                )?;
                if rt != RelationType::Causal {
                    // Best-effort: causal path used above; non-causal store via record_evidence
                    edge = relations.record_evidence(
                        &r.workspace,
                        &r.src,
                        &r.dst,
                        rt,
                        &crate::confidence::EvidenceType::RepeatedOccurrence,
                    )?;
                }
                let _ = edge;
                report.relations += 1;
            }
        }
    }

    Ok(report)
}

/// Convenience: seed bundle lifecycle default for test helpers.
pub fn empty_bundle() -> SeedBundle {
    SeedBundle::default()
}

#[allow(unused)]
fn _lifecycle_hint() -> RelationLifecycle {
    RelationLifecycle::Confirmed
}
