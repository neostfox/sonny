//! Link stage (P5-A): derive concept-to-concept edges from existing evidence.
//!
//! Three backfill sources, in increasing signal strength (design TODO.md
//! 断点四 — heterogeneous evidence feeds the same edge):
//!
//! 1. Shared entities — two concepts referencing the same canonical entity get
//!    a symmetric `shared_entity` edge; one evidence increment per shared
//!    entity. Ubiquitous entities (owned by more than
//!    `MAX_SHARED_ENTITY_OWNERS` concepts) are skipped: they would emit a
//!    quadratic K(K-1)/2 burst of edges, and an entity shared that widely is
//!    not evidence that any particular pair of concepts is related. Skips are
//!    counted in `LinkReport.shared_entity_skipped`, never dropped silently.
//! 2. Coclaim batches — observations extracted in one batch whose subjects
//!    belong to different concepts link those concepts with a symmetric
//!    `shared_session` edge; one increment per batch per pair.
//! 3. `causes` observations — a live observation `S causes O` creates a
//!    DIRECTED `causal` edge from every concept containing S to every concept
//!    containing O, weighted by the observation's source provenance.
//!
//! Idempotency caveat: evidence increments are keyed by run, not by source row
//! — re-running link_workspace re-accumulates. Callers run it once per
//! consolidation cycle, not per query.

use std::collections::{BTreeMap, BTreeSet};

use crate::confidence::EvidenceType;
use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::hierarchy::RelationType;
use crate::models::observation::Observation;
use crate::models::status::{ConceptStatus, ObservationStatus};
use crate::store::traits::{ConceptStore, ObservationStore, RelationStore};

/// An entity owned by more than this many concepts is too ubiquitous to be
/// pairwise-relatedness evidence; its `shared_entity` backfill is skipped to
/// avoid a quadratic K(K-1)/2 edge burst.
const MAX_SHARED_ENTITY_OWNERS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// Evidence increments applied to directed causal edges.
    pub causal_evidence: usize,
    /// Evidence increments applied to shared-entity edges.
    pub shared_entity_evidence: usize,
    /// Evidence increments applied to coclaim (shared-session) edges.
    pub coclaim_evidence: usize,
    /// Ubiquitous entities skipped for shared-entity backfill (owners >
    /// `MAX_SHARED_ENTITY_OWNERS`). Surfaced so the cap is never a silent drop.
    pub shared_entity_skipped: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinkEngine;

impl LinkEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn link_workspace<C, O, R>(
        &self,
        workspace_id: &str,
        concepts: &C,
        observations: &O,
        relations: &R,
    ) -> MemoryResult<LinkReport>
    where
        C: ConceptStore,
        O: ObservationStore,
        R: RelationStore,
    {
        let mut report = LinkReport::default();

        // Entity index over Active concepts: canonical entity key → concept ids.
        let active = concepts.list_concepts(workspace_id, Some(ConceptStatus::Active))?;
        let mut entity_index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for concept in &active {
            for key in concept_entity_keys(concept.related_entities_json.as_deref()) {
                entity_index
                    .entry(key)
                    .or_default()
                    .insert(concept.concept_id.clone());
            }
        }

        // 1. Shared entities → symmetric shared_entity edges. Skip ubiquitous
        // entities (owners > MAX_SHARED_ENTITY_OWNERS): quadratic edge burst,
        // and wide sharing is not pairwise-relatedness evidence.
        for owners in entity_index.values() {
            if owners.len() > MAX_SHARED_ENTITY_OWNERS {
                report.shared_entity_skipped += 1;
                continue;
            }
            for (a, b) in unordered_pairs(owners) {
                relations.record_evidence(
                    workspace_id,
                    a,
                    b,
                    RelationType::SharedEntity,
                    &EvidenceType::RepeatedOccurrence,
                )?;
                report.shared_entity_evidence += 1;
            }
        }

        let live: Vec<Observation> = observations
            .list_by_workspace(workspace_id, None)?
            .into_iter()
            .filter(|obs| is_live(&obs.status))
            .collect();

        // 2. Coclaim batches → symmetric shared_session edges.
        let mut batch_concepts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for obs in &live {
            let Some(batch_id) = obs.extraction_batch_id.as_deref() else {
                continue;
            };
            if let Some(owners) = entity_index.get(&canonical_key_light(&obs.subject_text)) {
                batch_concepts
                    .entry(batch_id.to_string())
                    .or_default()
                    .extend(owners.iter().cloned());
            }
        }
        for members in batch_concepts.values() {
            for (a, b) in unordered_pairs(members) {
                relations.record_evidence(
                    workspace_id,
                    a,
                    b,
                    RelationType::SharedSession,
                    &EvidenceType::RepeatedOccurrence,
                )?;
                report.coclaim_evidence += 1;
            }
        }

        // 3. `causes` observations → directed causal edges (subject → object).
        for obs in &live {
            if obs.predicate != "causes" {
                continue;
            }
            let Some(object_text) = obs.object_text.as_deref().filter(|s| !s.is_empty()) else {
                continue;
            };
            let empty = BTreeSet::new();
            let sources = entity_index
                .get(&canonical_key_light(&obs.subject_text))
                .unwrap_or(&empty);
            let targets = entity_index
                .get(&canonical_key_light(object_text))
                .unwrap_or(&empty);
            let evidence = causal_evidence_type(obs);
            for src in sources {
                for dst in targets {
                    if src == dst {
                        continue;
                    }
                    relations.record_evidence(
                        workspace_id,
                        src,
                        dst,
                        RelationType::Causal,
                        &evidence,
                    )?;
                    report.causal_evidence += 1;
                }
            }
        }

        Ok(report)
    }
}

/// Edge evidence implied by a `causes` observation's provenance. Falls back to
/// `RepeatedOccurrence` for uncorroborated user claims so the edge still
/// accumulates weak positive evidence (source trust already gated the
/// observation itself at extraction).
fn causal_evidence_type(obs: &Observation) -> EvidenceType {
    obs.source_type
        .initial_evidence()
        .unwrap_or(EvidenceType::RepeatedOccurrence)
}

fn is_live(status: &ObservationStatus) -> bool {
    matches!(
        status,
        ObservationStatus::Candidate
            | ObservationStatus::FastStored
            | ObservationStatus::Confirmed
            | ObservationStatus::AutoConfirmed
    )
}

fn concept_entity_keys(related_entities_json: Option<&str>) -> BTreeSet<String> {
    let Some(raw) = related_entities_json.filter(|s| !s.is_empty()) else {
        return BTreeSet::new();
    };
    let Ok(entities) = serde_json::from_str::<Vec<String>>(raw) else {
        return BTreeSet::new();
    };
    entities
        .iter()
        .map(|entity| canonical_key_light(entity))
        .filter(|key| !key.is_empty())
        .collect()
}

fn unordered_pairs(ids: &BTreeSet<String>) -> Vec<(&str, &str)> {
    let ordered: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut pairs = Vec::new();
    for left in 0..ordered.len() {
        for right in (left + 1)..ordered.len() {
            pairs.push((ordered[left], ordered[right]));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::concept::Concept;
    use crate::models::observation::{Observation, ObservationSourceType};
    use crate::models::raw_memory::{RawMemory, SourceType};
    use crate::models::relation::RelationLifecycle;
    use crate::store::concept_store::SqliteConceptStore;
    use crate::store::connection::Database;
    use crate::store::observation_store::SqliteObservationStore;
    use crate::store::raw_memory_store::SqliteRawMemoryStore;
    use crate::store::relation_store::SqliteRelationStore;
    use crate::store::traits::RawMemoryStore;

    struct Fixture {
        _db: Database,
        concepts: SqliteConceptStore,
        observations: SqliteObservationStore,
        relations: SqliteRelationStore,
        raw_memories: SqliteRawMemoryStore,
    }

    impl Fixture {
        /// Insert an observation plus the raw_memory row its FK requires.
        fn insert_observation(&self, obs: &Observation) {
            self.raw_memories
                .insert(&RawMemory {
                    memory_id: obs.memory_id.clone(),
                    workspace_id: obs.workspace_id.clone(),
                    session_id: format!("s-{}", obs.memory_id),
                    role: "user".to_string(),
                    content: obs.subject_text.clone(),
                    source_type: SourceType::Manual,
                    source_ref: "test".to_string(),
                    extraction_version: None,
                    created_at: obs.created_at.clone(),
                })
                .unwrap();
            self.observations.insert(obs).unwrap();
        }
    }

    fn fixture() -> Fixture {
        let db = Database::open_in_memory().unwrap();
        Fixture {
            concepts: SqliteConceptStore::new(db.conn.clone()),
            observations: SqliteObservationStore::new(db.conn.clone()),
            relations: SqliteRelationStore::new(db.conn.clone()),
            raw_memories: SqliteRawMemoryStore::new(db.conn.clone()),
            _db: db,
        }
    }

    fn concept(id: &str, entities: &[&str]) -> Concept {
        Concept {
            concept_id: id.to_string(),
            workspace_id: "ws".to_string(),
            name: id.to_string(),
            concept_type: None,
            definition: None,
            related_entities_json: Some(serde_json::to_string(entities).unwrap()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
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
            created_at: "t0".to_string(),
            updated_at: "t0".to_string(),
        }
    }

    fn observation(
        id: &str,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        source: ObservationSourceType,
        batch: Option<&str>,
    ) -> Observation {
        Observation {
            observation_id: id.to_string(),
            workspace_id: "ws".to_string(),
            memory_id: format!("m-{id}"),
            subject_text: subject.to_string(),
            subject_type: None,
            predicate: predicate.to_string(),
            object_text: object.map(str::to_string),
            object_type: None,
            evidence_text: None,
            extraction_confidence: source.extraction_confidence(),
            evidence_alpha: 1.0,
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: source,
            consolidated: false,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: batch.map(str::to_string),
            superseded_by: None,
            created_at: "t0".to_string(),
        }
    }

    #[test]
    fn causes_observation_creates_directed_causal_edge() {
        let f = fixture();
        f.concepts.insert_concept(&concept("c-mask", &["POSMASK"])).unwrap();
        f.concepts.insert_concept(&concept("c-crash", &["启动崩溃"])).unwrap();
        f.insert_observation(&observation(
            "o1",
            "POSMASK",
            "causes",
            Some("启动崩溃"),
            ObservationSourceType::FileEvidence,
            None,
        ));

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        assert_eq!(report.causal_evidence, 1);
        let edge = f
            .relations
            .get_edge("ws", "c-mask", "c-crash", RelationType::Causal)
            .unwrap()
            .expect("causal edge must exist");
        assert_eq!(edge.src_concept_id, "c-mask");
        assert!((edge.weight() - 2.5 / 3.5).abs() < 1e-9); // FileEvidence: alpha += 1.5
        // Reverse direction must NOT exist.
        assert!(f
            .relations
            .get_edge("ws", "c-crash", "c-mask", RelationType::Causal)
            .unwrap()
            .is_none());
    }

    #[test]
    fn shared_entity_creates_symmetric_edge_per_entity() {
        let f = fixture();
        f.concepts
            .insert_concept(&concept("c-a", &["User Model", "orderhdr"]))
            .unwrap();
        f.concepts
            .insert_concept(&concept("c-b", &["user-model", "ORDERHDR"]))
            .unwrap();

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        // Two shared canonical entities → two evidence increments on ONE edge.
        assert_eq!(report.shared_entity_evidence, 2);
        let edges = f.relations.list_by_workspace("ws").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation_type, RelationType::SharedEntity);
        assert_eq!(edges[0].evidence_count, 2);
    }

    #[test]
    fn ubiquitous_entity_is_skipped_not_quadratically_expanded() {
        let f = fixture();
        // 9 concepts all sharing one entity → owners = 9 > MAX_SHARED_ENTITY_OWNERS.
        // Without the cap this would emit 9*8/2 = 36 shared_entity edges.
        for i in 0..(MAX_SHARED_ENTITY_OWNERS + 1) {
            f.concepts
                .insert_concept(&concept(&format!("c-{i}"), &["hot-entity"]))
                .unwrap();
        }

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        assert_eq!(report.shared_entity_evidence, 0);
        assert_eq!(report.shared_entity_skipped, 1);
        assert!(f.relations.list_by_workspace("ws").unwrap().is_empty());
    }

    #[test]
    fn entity_at_owner_cap_still_links() {
        let f = fixture();
        // Exactly MAX_SHARED_ENTITY_OWNERS owners is within the cap → full backfill.
        for i in 0..MAX_SHARED_ENTITY_OWNERS {
            f.concepts
                .insert_concept(&concept(&format!("c-{i}"), &["shared"]))
                .unwrap();
        }

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        let n = MAX_SHARED_ENTITY_OWNERS;
        assert_eq!(report.shared_entity_skipped, 0);
        assert_eq!(report.shared_entity_evidence, n * (n - 1) / 2);
    }

    #[test]
    fn coclaim_batch_links_concepts_of_cobatched_subjects() {
        let f = fixture();
        f.concepts.insert_concept(&concept("c-a", &["alpha"])).unwrap();
        f.concepts.insert_concept(&concept("c-b", &["beta"])).unwrap();
        f.insert_observation(&observation("o1", "alpha", "has", Some("x"), ObservationSourceType::UserMessage, Some("batch-1")));
        f.insert_observation(&observation("o2", "beta", "has", Some("y"), ObservationSourceType::UserMessage, Some("batch-1")));

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        assert_eq!(report.coclaim_evidence, 1);
        assert!(f
            .relations
            .get_edge("ws", "c-a", "c-b", RelationType::SharedSession)
            .unwrap()
            .is_some());
    }

    #[test]
    fn superseded_observations_contribute_nothing() {
        let f = fixture();
        f.concepts.insert_concept(&concept("c-a", &["alpha"])).unwrap();
        f.concepts.insert_concept(&concept("c-b", &["beta"])).unwrap();
        let mut stale = observation(
            "o1",
            "alpha",
            "causes",
            Some("beta"),
            ObservationSourceType::FileEvidence,
            Some("batch-1"),
        );
        stale.status = ObservationStatus::Superseded;
        f.insert_observation(&stale);

        let report = LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        assert_eq!(report.causal_evidence, 0);
        assert_eq!(report.coclaim_evidence, 0);
    }

    #[test]
    fn repeated_cross_source_evidence_upgrades_lifecycle() {
        let f = fixture();
        f.concepts.insert_concept(&concept("c-mask", &["POSMASK"])).unwrap();
        f.concepts.insert_concept(&concept("c-crash", &["crash"])).unwrap();
        // Three independent causal observations: file + file + user confirmation.
        for (id, source) in [
            ("o1", ObservationSourceType::FileEvidence),
            ("o2", ObservationSourceType::FileEvidence),
            ("o3", ObservationSourceType::UserConfirm),
        ] {
            f.insert_observation(&observation(id, "POSMASK", "causes", Some("crash"), source, None));
        }

        LinkEngine::new()
            .link_workspace("ws", &f.concepts, &f.observations, &f.relations)
            .unwrap();

        let edge = f
            .relations
            .get_edge("ws", "c-mask", "c-crash", RelationType::Causal)
            .unwrap()
            .unwrap();
        assert_eq!(edge.evidence_count, 3);
        assert_eq!(edge.lifecycle, RelationLifecycle::Confirmed); // UserConfirm promotes
        assert!(edge.weight() > 0.8);
    }
}
