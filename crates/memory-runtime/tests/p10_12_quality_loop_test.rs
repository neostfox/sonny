//! P10-P12 acceptance: dreaming merge, persistence gate, action plan.

use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::persistence::{classify_persistence, detect_correction, PersistenceScope};
use memory_runtime::models::raw_memory::{RawMemory, SourceType};
use memory_runtime::models::status::ObservationStatus;
use memory_runtime::pipeline::action_plan::{plan_memory_action, MemoryAction};
use memory_runtime::pipeline::dream::{detect_neighborhood, dream_workspace, DreamIssue};
use memory_runtime::store::connection::Database;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::traits::ObservationStore;

fn raw(ws: &str, id: &str) -> RawMemory {
    RawMemory {
        memory_id: id.into(),
        workspace_id: ws.into(),
        session_id: format!("s-{id}"),
        role: "user".into(),
        content: "x".into(),
        source_type: SourceType::Manual,
        source_ref: "t".into(),
        extraction_version: None,
        created_at: "2026-07-01T00:00:00Z".into(),
    }
}

fn obs(ws: &str, mem: &str, subj: &str, pred: &str, obj: Option<&str>, alpha: f64) -> Observation {
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: ws.into(),
        memory_id: mem.into(),
        subject_text: subj.into(),
        subject_type: None,
        predicate: pred.into(),
        object_text: obj.map(|s| s.to_string()),
        object_type: None,
        evidence_text: Some("ev".into()),
        extraction_confidence: 0.8,
        evidence_alpha: alpha,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type: ObservationSourceType::UserMessage,
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

fn seed_raw(db: &Database, r: &RawMemory) {
    db.conn
        .lock()
        .execute(
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
fn dreaming_merges_duplicate_triples() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteObservationStore::new(db.conn.clone());
    let r1 = raw("ws", "m1");
    let r2 = raw("ws", "m2");
    seed_raw(&db, &r1);
    seed_raw(&db, &r2);

    store.insert(&obs("ws", "m1", "posmask", "has", Some("field"), 5.0)).unwrap();
    store.insert(&obs("ws", "m2", "posmask", "has", Some("field"), 2.0)).unwrap();

    let report = dream_workspace(&store, "ws").unwrap();
    assert!(report.merges >= 1, "report={report:?}");

    let live: Vec<_> = store
        .list_by_workspace("ws", None)
        .unwrap()
        .into_iter()
        .filter(|o| o.status == ObservationStatus::Candidate || o.status == ObservationStatus::Confirmed)
        .filter(|o| o.subject_text == "posmask" && o.predicate == "has")
        .collect();
    assert_eq!(live.len(), 1, "expected one live triple after merge");
    // Keep the stronger row (α=5) and accumulated RepeatedOccurrence.
    assert!(live[0].evidence_alpha > 5.0);

    // Second dream should skip (consolidated).
    let report2 = dream_workspace(&store, "ws").unwrap();
    assert_eq!(report2.merges, 0);
    assert_eq!(report2.neighborhoods, 0);
}

#[test]
fn dreaming_softens_conflicts() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteObservationStore::new(db.conn.clone());
    let r1 = raw("ws", "m1");
    let r2 = raw("ws", "m2");
    seed_raw(&db, &r1);
    seed_raw(&db, &r2);

    store.insert(&obs("ws", "m1", "posmask", "has", Some("field"), 4.0)).unwrap();
    store.insert(&obs("ws", "m2", "posmask", "not_has", Some("field"), 4.0)).unwrap();

    let report = dream_workspace(&store, "ws").unwrap();
    assert!(report.softenings >= 1, "report={report:?}");
    let rows = store.list_by_workspace("ws", None).unwrap();
    for o in rows.iter().filter(|o| o.subject_text == "posmask") {
        assert!(o.evidence_beta > 1.0, "conflict should raise beta");
    }
}

#[test]
fn task_temporary_correction_not_durable() {
    let s = detect_correction("错了，这次先别改配置").unwrap();
    assert_eq!(s.scope, PersistenceScope::TaskTemporary);
    assert!(!s.scope.is_durable());
}

#[test]
fn long_term_correction_is_durable() {
    let s = detect_correction("不对，以后都必须跑测试").unwrap();
    assert_eq!(s.scope, PersistenceScope::LongTerm);
    assert!(s.scope.is_durable());
}

#[test]
fn default_persistence_is_scenario() {
    assert_eq!(classify_persistence("改一下类型"), PersistenceScope::ScenarioSpecific);
}

#[test]
fn action_plan_reinforce_vs_add() {
    let existing = obs("ws", "m", "a", "has", Some("b"), 2.0);
    let same = obs("ws", "m2", "a", "has", Some("b"), 1.0);
    assert_eq!(plan_memory_action(&same, &[existing.clone()]), MemoryAction::Reinforce);
    let other = obs("ws", "m3", "c", "has", Some("d"), 1.0);
    assert_eq!(plan_memory_action(&other, &[existing]), MemoryAction::Add);
}

#[test]
fn detect_neighborhood_flags_duplicates() {
    let a = obs("ws", "m1", "x", "has", Some("y"), 2.0);
    let b = obs("ws", "m2", "x", "has", Some("y"), 2.0);
    let issues = detect_neighborhood(&[a, b]);
    assert!(issues.iter().any(|(i, _)| *i == DreamIssue::DuplicateTriple));
}
