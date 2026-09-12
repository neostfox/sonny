//! P7 acceptance: causal role, do-stats, heterogeneous evidence weights.

use memory_runtime::confidence::{BetaConfidence, EvidenceType};
use memory_runtime::models::causal::{
    causal_edge_evidence, heterogeneous_evidence_weight, CausalRole, CausalStats,
};
use memory_runtime::models::hierarchy::RelationType;
use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::store::connection::Database;
use memory_runtime::store::relation_store::SqliteRelationStore;
use memory_runtime::store::traits::RelationStore;

#[test]
fn intervention_and_file_evidence_confirm_edge_immediately() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteRelationStore::new(db.conn.clone());
    {
        let conn = db.conn.lock();
        for id in ["c1", "c2"] {
            conn.execute(
                "INSERT INTO concept (concept_id, workspace_id, name, confidence, evidence_alpha, evidence_beta, status, hierarchy_depth, connection_count, lifecycle_scope, created_at, updated_at)
                 VALUES (?1,'ws',?1,0.8,1,1,'active',0,0,'project','t','t')",
                rusqlite::params![id],
            )
            .unwrap();
        }
    }

    let stats = CausalStats {
        p_do: Some(0.85),
        p_given: Some(0.7),
        p_not_given: Some(0.2),
    };
    let edge = store
        .record_causal_evidence(
            "ws",
            "c1",
            "c2",
            &EvidenceType::HumanReviewConfirm,
            Some(ObservationSourceType::FileEvidence),
            3,
            &stats,
        )
        .unwrap();

    assert_eq!(edge.lifecycle, memory_runtime::models::relation::RelationLifecycle::Confirmed);
    assert!(edge.causal_stats.p_do.unwrap() > 0.8);
    assert!(edge.causal_stats.intervention_lift().is_some());
    // reuse=3 → factor 1.5; HumanReviewConfirm base α+=2 → scaled 3.0
    assert!(edge.evidence_alpha >= 3.0);
}

#[test]
fn association_only_does_not_set_p_do() {
    let mut stats = CausalStats::default();
    let mut obs = dummy_obs();
    obs.causal_role = Some(CausalRole::ObservedAssociation.as_str().into());
    stats.absorb_observation(&obs);
    assert!(stats.p_do.is_none());
    assert!(stats.p_given.is_some());
}

#[test]
fn confound_never_maps_to_edge_evidence() {
    let mut obs = dummy_obs();
    obs.causal_role = Some(CausalRole::Confound.as_str().into());
    assert!(causal_edge_evidence(&obs).is_none());
}

#[test]
fn heterogeneous_weights_downweight_assistant_guess() {
    let (a, _) = heterogeneous_evidence_weight(
        &EvidenceType::RepeatedOccurrence,
        Some(ObservationSourceType::AssistantGuess),
        1,
    );
    let (b, _) = heterogeneous_evidence_weight(
        &EvidenceType::RepeatedOccurrence,
        Some(ObservationSourceType::FileEvidence),
        1,
    );
    assert!(a < b);
}

#[test]
fn cross_project_reuse_scales_edge_posterior() {
    let mut single = BetaConfidence::new();
    let mut multi = BetaConfidence::new();
    memory_runtime::models::causal::update_with_heterogeneous_evidence(
        &mut single,
        &EvidenceType::RepeatedOccurrence,
        None,
        1,
    );
    memory_runtime::models::causal::update_with_heterogeneous_evidence(
        &mut multi,
        &EvidenceType::RepeatedOccurrence,
        None,
        4,
    );
    assert!(multi.confidence() > single.confidence());
}

fn dummy_obs() -> Observation {
    Observation {
        observation_id: "o".into(),
        workspace_id: "ws".into(),
        memory_id: "m".into(),
        subject_text: "A".into(),
        subject_type: None,
        predicate: "causes".into(),
        object_text: Some("B".into()),
        object_type: None,
        evidence_text: None,
        extraction_confidence: 0.9,
        evidence_alpha: 2.0,
        evidence_beta: 1.0,
        status: memory_runtime::models::status::ObservationStatus::Confirmed,
        surprise_score: 0.5,
        source_type: ObservationSourceType::FileEvidence,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: None,
        superseded_by: None,
        cross_project_count: 1,
        causal_role: None,
        consolidated: false,
        created_at: "t".into(),
    }
}

// silence unused import warning for RelationType in case tests grow
#[allow(dead_code)]
fn _rt() -> RelationType {
    RelationType::Causal
}
