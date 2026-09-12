//! P8 acceptance: structural fingerprints + cross-workspace transfer notes.

use memory_runtime::models::concept::Concept;
use memory_runtime::models::hierarchy::RelationType;
use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::relation::RelationLifecycle;
use memory_runtime::models::status::{ConceptStatus, ObservationStatus};
use memory_runtime::pipeline::transfer::{
    find_structural_peers, format_transfer_notes, transfer_notes_for_concepts, ConceptGraph,
};
use memory_runtime::store::connection::Database;
use memory_runtime::store::concept_store::SqliteConceptStore;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::relation_store::SqliteRelationStore;
use memory_runtime::store::traits::{ConceptStore, ObservationStore, RelationStore};

fn seed_raw(db: &Database, ws: &str, id: &str) {
    db.conn
        .lock()
        .execute(
            "INSERT OR IGNORE INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
             VALUES (?1,?2,?3,'user','x','manual','t','t')",
            rusqlite::params![id, ws, format!("s-{id}")],
        )
        .unwrap();
}

fn concept(ws: &str, id: &str, entities: &[&str]) -> Concept {
    Concept {
        concept_id: id.to_string(),
        workspace_id: ws.to_string(),
        name: format!("mod-{id}"),
        concept_type: None,
        definition: None,
        related_entities_json: Some(serde_json::to_string(entities).unwrap()),
        known_facts_json: None,
        rejected_hypotheses_json: None,
        open_questions_json: None,
        evidence_json: None,
        confidence: 0.9,
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
        lifecycle_scope: memory_runtime::models::scope::LifecycleScope::Project,
        scope_key: None,
        created_at: "t".into(),
        updated_at: "t".into(),
    }
}

fn causes_obs(ws: &str, mem: &str, subject: &str, object: &str) -> Observation {
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: ws.to_string(),
        memory_id: mem.to_string(),
        subject_text: subject.to_string(),
        subject_type: None,
        predicate: "causes".into(),
        object_text: Some(object.to_string()),
        object_type: None,
        evidence_text: Some("evidence".into()),
        extraction_confidence: 0.9,
        evidence_alpha: 4.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Confirmed,
        surprise_score: 0.5,
        source_type: ObservationSourceType::FileEvidence,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: None,
        superseded_by: None,
        cross_project_count: 1,
        causal_role: Some("intervention".into()),
        consolidated: false,
        created_at: "t".into(),
    }
}

#[test]
fn isomorphic_modules_match_across_workspaces() {
    let db = Database::open_in_memory().unwrap();
    let concepts = SqliteConceptStore::new(db.conn.clone());
    let observations = SqliteObservationStore::new(db.conn.clone());
    let relations = SqliteRelationStore::new(db.conn.clone());

    for (ws, mid, obs_id) in [
        ("ws-a", "m-a1", "raw-a1"),
        ("ws-a", "m-a2", "raw-a2"),
        ("ws-b", "m-b1", "raw-b1"),
        ("ws-b", "m-b2", "raw-b2"),
    ] {
        seed_raw(&db, ws, obs_id);
        let _ = mid;
    }

    // ws-a: AlphaModule with causal chain alpha_src -> alpha_mid -> alpha_sink
    concepts.insert_concept(&concept("ws-a", "mod-a", &["alpha_src", "alpha_mid", "alpha_sink"])).unwrap();
    observations.insert(&causes_obs("ws-a", "raw-a1", "alpha_src", "alpha_mid")).unwrap();
    observations.insert(&causes_obs("ws-a", "raw-a2", "alpha_mid", "alpha_sink")).unwrap();

    // ws-b: completely different names, same structure
    concepts.insert_concept(&concept("ws-b", "mod-b", &["beta_src", "beta_mid", "beta_sink"])).unwrap();
    observations.insert(&causes_obs("ws-b", "raw-b1", "beta_src", "beta_mid")).unwrap();
    observations.insert(&causes_obs("ws-b", "raw-b2", "beta_mid", "beta_sink")).unwrap();

    // Give mod-b a confirmed causal self-loop style edge (self -> causal_peer placeholder)
    // by recording a causal relation involving mod-b.
    relations
        .record_causal_evidence(
            "ws-b",
            "mod-b",
            "mod-b", // will be rejected as self-loop — use a second concept instead
            &memory_runtime::confidence::EvidenceType::HumanReviewConfirm,
            Some(ObservationSourceType::FileEvidence),
            2,
            &memory_runtime::models::causal::CausalStats {
                p_do: Some(0.9),
                p_given: Some(0.8),
                p_not_given: Some(0.1),
            },
        )
        .unwrap_err(); // self-loop rejected

    // Add a peer concept in ws-b so the causal edge is valid and transferable.
    concepts.insert_concept(&concept("ws-b", "mod-b-peer", &["beta_peer"])).unwrap();
    let edge = relations
        .record_causal_evidence(
            "ws-b",
            "mod-b",
            "mod-b-peer",
            &memory_runtime::confidence::EvidenceType::HumanReviewConfirm,
            Some(ObservationSourceType::FileEvidence),
            2,
            &memory_runtime::models::causal::CausalStats::default(),
        )
        .unwrap();
    assert_eq!(edge.lifecycle, RelationLifecycle::Confirmed);

    let peers = find_structural_peers(
        &concepts,
        &observations,
        &relations,
        "ws-a",
        "mod-a",
        0.5,
        2,
    )
    .unwrap();

    // Structural peer should be found despite disjoint entity names.
    assert!(
        peers.iter().any(|p| p.concept_id == "mod-b"),
        "expected structural peer mod-b, got {:?}",
        peers.iter().map(|p| &p.concept_id).collect::<Vec<_>>()
    );

    let notes = transfer_notes_for_concepts(
        &concepts,
        &observations,
        &relations,
        "ws-a",
        &["mod-a".to_string()],
        10,
    )
    .unwrap();
    assert!(!notes.is_empty());
    assert!(
        notes.iter().any(|n| n.contains("ws-b")),
        "notes should mention source workspace: {notes:?}"
    );
}

#[test]
fn unrelated_structure_is_not_a_peer() {
    let a = ConceptGraph::from_parts(
        ["a".into(), "b".into(), "c".into(), "d".into()],
        vec![
            memory_runtime::pipeline::transfer::StructuralEdge {
                src: "a".into(),
                dst: "b".into(),
                predicate: "causes".into(),
                directed: true,
            },
            memory_runtime::pipeline::transfer::StructuralEdge {
                src: "b".into(),
                dst: "c".into(),
                predicate: "causes".into(),
                directed: true,
            },
        ],
    );
    let b = ConceptGraph::from_parts(
        ["x".into()],
        vec![memory_runtime::pipeline::transfer::StructuralEdge {
            src: "x".into(),
            dst: "x".into(),
            predicate: "related_to".into(),
            directed: false,
        }],
    );
    let sim = ConceptGraph::similarity(&a.fingerprint(2), &b.fingerprint(2));
    assert!(sim < 0.5, "sim={sim}");
}

#[test]
fn format_notes_lists_validated_edges() {
    use memory_runtime::models::relation::ConceptRelation;
    let mut edge = ConceptRelation::new("ws-b", "p1", "p2", RelationType::Causal, "t");
    edge.lifecycle = RelationLifecycle::Validated;
    let peer = memory_runtime::pipeline::transfer::StructuralPeer {
        concept_id: "p".into(),
        workspace_id: "ws-b".into(),
        similarity: 0.9,
        transferable_causal_edges: vec![edge],
    };
    let notes = format_transfer_notes(&[peer], 5);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("Validated"));
}
