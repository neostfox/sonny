//! P6 acceptance: cross-project count, promotion, visibility, alias merge.

use memory_runtime::models::concept::Concept;
use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::raw_memory::{RawMemory, SourceType};
use memory_runtime::models::scope::{
    evaluate_promotion, LifecycleScope, PromotionEvidence, PromotionThresholds,
};
use memory_runtime::models::status::{ConceptStatus, ObservationStatus};
use memory_runtime::pipeline::promote::{maybe_promote_concept, PromotionReport};
use memory_runtime::store::connection::Database;
use memory_runtime::store::concept_store::SqliteConceptStore;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::traits::{ConceptStore, ObservationStore};

fn raw(ws: &str, id: &str) -> RawMemory {
    RawMemory {
        memory_id: id.to_string(),
        workspace_id: ws.to_string(),
        session_id: format!("s-{id}"),
        role: "user".into(),
        content: "x".into(),
        source_type: SourceType::Manual,
        source_ref: "t".into(),
        extraction_version: None,
        created_at: "2026-07-01T00:00:00Z".into(),
    }
}

fn obs(ws: &str, mem: &str, subject: &str, object: &str) -> Observation {
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: ws.to_string(),
        memory_id: mem.to_string(),
        subject_text: subject.to_string(),
        subject_type: None,
        predicate: "causes".to_string(),
        object_text: Some(object.to_string()),
        object_type: None,
        evidence_text: Some(format!("{subject} causes {object}")),
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
        causal_role: None,
        consolidated: false,
        created_at: "2026-07-01T00:00:00Z".into(),
    }
}

fn concept(ws: &str, id: &str, entities: &[&str], confidence: f64) -> Concept {
    Concept {
        concept_id: id.to_string(),
        workspace_id: ws.to_string(),
        name: format!("concept-{id}"),
        concept_type: None,
        definition: Some("same causal pattern".into()),
        related_entities_json: Some(serde_json::to_string(entities).unwrap()),
        known_facts_json: None,
        rejected_hypotheses_json: None,
        open_questions_json: None,
        evidence_json: None,
        confidence,
        evidence_alpha: 9.0,
        evidence_beta: 1.0,
        status: ConceptStatus::Active,
        parent_concept_id: None,
        hierarchy_depth: 0,
        last_recalled_at: None,
        recall_count: 2,
        successful_recall_count: 2,
        failed_recall_count: 0,
        connection_count: 3,
        lifecycle_scope: LifecycleScope::Project,
        scope_key: None,
        created_at: "2026-07-01T00:00:00Z".into(),
        updated_at: "2026-07-01T00:00:00Z".into(),
    }
}

fn seed_raw(conn: &rusqlite::Connection, r: &RawMemory) {
    conn.execute(
        "INSERT OR IGNORE INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            r.memory_id, r.workspace_id, r.session_id, r.role, r.content,
            r.source_type.as_str(), r.source_ref, r.created_at
        ],
    )
    .unwrap();
}

#[test]
fn same_triple_in_two_workspaces_raises_cross_project_count() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteObservationStore::new(db.conn.clone());

    let r1 = raw("ws-a", "m-a");
    let r2 = raw("ws-b", "m-b");
    {
        let conn = db.conn.lock();
        seed_raw(&conn, &r1);
        seed_raw(&conn, &r2);
    }

    store.insert(&obs("ws-a", "m-a", "posmask", "machine_field")).unwrap();
    // Before ws-b: count is 1
    let before = store.sync_cross_project_count("posmask", "causes", Some("machine_field")).unwrap();
    assert_eq!(before, 1);

    store.insert(&obs("ws-b", "m-b", "posmask", "machine_field")).unwrap();
    let after = store.sync_cross_project_count("posmask", "causes", Some("machine_field")).unwrap();
    assert_eq!(after, 2);

    let matches = store
        .find_duplicate_any_workspace("posmask", "causes", Some("machine_field"))
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert!(matches.iter().all(|o| o.cross_project_count == 2));
}

#[test]
fn project_concept_stays_invisible_to_other_workspace() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteConceptStore::new(db.conn.clone());
    store.insert_concept(&concept("ws-a", "c1", &["posmask"], 0.9)).unwrap();

    let visible = store
        .list_visible_concepts("ws-b", Some(ConceptStatus::Active), &[], true)
        .unwrap();
    assert!(visible.is_empty());
}

#[test]
fn global_concept_is_visible_to_other_workspace() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteConceptStore::new(db.conn.clone());
    let mut c = concept("ws-a", "c1", &["posmask"], 0.95);
    c.lifecycle_scope = LifecycleScope::Global;
    store.insert_concept(&c).unwrap();

    let visible = store
        .list_visible_concepts("ws-b", Some(ConceptStatus::Active), &[], true)
        .unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].concept_id, "c1");

    // Without include_global, still invisible
    let hidden = store
        .list_visible_concepts("ws-b", Some(ConceptStatus::Active), &[], false)
        .unwrap();
    assert!(hidden.is_empty());
}

#[test]
fn domain_concept_visible_only_with_matching_key() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteConceptStore::new(db.conn.clone());
    let mut c = concept("ws-a", "c1", &["posmask"], 0.9);
    c.lifecycle_scope = LifecycleScope::Domain;
    c.scope_key = Some("rust".into());
    store.insert_concept(&c).unwrap();

    let hit = store
        .list_visible_concepts("ws-b", Some(ConceptStatus::Active), &["rust".into()], false)
        .unwrap();
    assert_eq!(hit.len(), 1);

    let miss = store
        .list_visible_concepts("ws-b", Some(ConceptStatus::Active), &["python".into()], false)
        .unwrap();
    assert!(miss.is_empty());
}

#[test]
fn promotion_elevates_and_merges_peer() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteConceptStore::new(db.conn.clone());

    let mut primary = concept("ws-a", "c-primary", &["posmask", "machine_field"], 0.9);
    primary.connection_count = 3;
    primary.recall_count = 3;
    store.insert_concept(&primary).unwrap();

    let mut peer = concept("ws-b", "c-peer", &["posmask", "machine_field"], 0.8);
    peer.connection_count = 3;
    store.insert_concept(&peer).unwrap();

    let report = maybe_promote_concept(
        &store,
        "c-primary",
        2,
        0,
        &PromotionThresholds::default(),
        Some("rust"),
        "2026-07-02T00:00:00Z",
    )
    .unwrap();

    assert_eq!(report.to, Some(LifecycleScope::Domain));
    let promoted = store.get_concept("c-primary").unwrap().unwrap();
    assert_eq!(promoted.lifecycle_scope, LifecycleScope::Domain);
    assert_eq!(promoted.scope_key.as_deref(), Some("rust"));

    // Peer should have been aliased and deprecated
    let peer_after = store.get_concept("c-peer").unwrap().unwrap();
    assert_eq!(peer_after.status, ConceptStatus::Deprecated);
    assert_eq!(store.resolve_alias("c-peer").unwrap(), "c-primary");
}

#[test]
fn single_workspace_evidence_cannot_promote() {
    let t = PromotionThresholds::default();
    let e = PromotionEvidence {
        cross_project_count: 1,
        conflict_count: 0,
        unique_session_count: 5,
        confidence: 0.99,
    };
    assert_eq!(evaluate_promotion(&e, &t), None);
}

#[test]
fn promotion_report_defaults_when_concept_missing() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteConceptStore::new(db.conn.clone());
    let report: PromotionReport = maybe_promote_concept(
        &store,
        "missing",
        3,
        0,
        &PromotionThresholds::default(),
        None,
        "now",
    )
    .unwrap();
    assert_eq!(report.to, None);
}
