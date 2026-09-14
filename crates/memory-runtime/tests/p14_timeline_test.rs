//! P14 acceptance: entity–property timeline versioning.

use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::status::ObservationStatus;
use memory_runtime::models::timeline::TimelineStatus;
use memory_runtime::pipeline::timeline::{current_value, format_history, record_from_observation};
use memory_runtime::store::connection::Database;
use memory_runtime::store::timeline_store::SqliteTimelineStore;
use memory_runtime::store::traits::TimelineStore;

fn obs(ws: &str, subj: &str, pred: &str, obj: Option<&str>) -> Observation {
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: ws.into(),
        memory_id: "m".into(),
        subject_text: subj.into(),
        subject_type: None,
        predicate: pred.into(),
        object_text: obj.map(|s| s.to_string()),
        object_type: None,
        evidence_text: Some("ev".into()),
        extraction_confidence: 0.8,
        evidence_alpha: 2.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Confirmed,
        surprise_score: 0.5,
        source_type: ObservationSourceType::UserMessage,
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

#[test]
fn timeline_keeps_history_when_value_updates() {
    let db = Database::open_in_memory().unwrap();
    let tl = SqliteTimelineStore::new(db.conn.clone());

    let o1 = obs("ws", "user", "uses_editor", Some("vim"));
    record_from_observation(&tl, &o1, "2026-01-01T00:00:00Z").unwrap();

    let o2 = obs("ws", "user", "uses_editor", Some("helix"));
    record_from_observation(&tl, &o2, "2026-02-01T00:00:00Z").unwrap();

    assert_eq!(
        current_value(&tl, "ws", "user", "uses_editor").unwrap().as_deref(),
        Some("helix")
    );
    let hist = tl.history("ws", "user", "uses_editor").unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(
        hist.iter().filter(|e| e.status == TimelineStatus::Active).count(),
        1
    );
    assert_eq!(
        hist.iter().filter(|e| e.status == TimelineStatus::Superseded).count(),
        1
    );
    // Newest first
    assert_eq!(hist[0].value.as_deref(), Some("helix"));

    let lines = format_history(&hist);
    assert!(lines[0].contains("helix"));
    assert!(lines[1].contains("vim"));
}

#[test]
fn entity_timeline_lists_all_properties() {
    let db = Database::open_in_memory().unwrap();
    let tl = SqliteTimelineStore::new(db.conn.clone());
    record_from_observation(&tl, &obs("ws", "posmask", "has", Some("field")), "t1").unwrap();
    record_from_observation(&tl, &obs("ws", "posmask", "depends_on", Some("db")), "t2").unwrap();

    let all = tl.history_for_entity("ws", "posmask").unwrap();
    let props: Vec<_> = all.iter().map(|e| e.property.as_str()).collect();
    assert!(props.contains(&"has"));
    assert!(props.contains(&"depends_on"));
}
