use memory_runtime::confidence::{BetaConfidence, EvidenceType};
use memory_runtime::entity::canonical_key;
use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::raw_memory::{RawMemory, SourceType};
use memory_runtime::models::status::ObservationStatus;
use memory_runtime::pipeline::extract::extract_observations;
use memory_runtime::pipeline::ingest::{detect_and_parse, SessionParser, TrellisJournalParser};
use memory_runtime::store::connection::Database;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::raw_memory_store::SqliteRawMemoryStore;
use memory_runtime::store::traits::{ObservationStore, RawMemoryStore};

fn make_test_raw_memory(
    workspace_id: &str,
    session_id: &str,
    role: &str,
    content: &str,
) -> RawMemory {
    RawMemory {
        memory_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        session_id: session_id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        source_type: SourceType::SessionFile,
        source_ref: "test".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Seed raw_memory rows directly via SQL through a shared connection.
fn seed_raw_memories(conn: &rusqlite::Connection, memories: &[RawMemory]) {
    for m in memories {
        conn.execute(
            "INSERT OR IGNORE INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                m.memory_id, m.workspace_id, m.session_id, m.role,
                m.content, m.source_type.as_str(), m.source_ref, m.created_at
            ],
        ).unwrap();
    }
}

#[test]
fn test_full_pipeline_json_ingest_to_store() {
    let db = Database::open_in_memory().unwrap();

    // Step 1: Ingest JSON session
    let json = r#"{
        "session_id": "integration_test_1",
        "date": "2026-05-16",
        "messages": [
            {"role": "user", "content": "POSMASK 表没有机器字段"},
            {"role": "assistant", "content": "了解，我来检查一下。"}
        ]
    }"#;

    let memories = detect_and_parse(json, "test.json", "test_ws", "test.json").unwrap();
    assert_eq!(memories.len(), 2);

    // Step 2: Seed raw memories through the shared connection
    {
        let conn = db.conn.lock();
        seed_raw_memories(&conn, &memories);
    }

    // Step 3: Observation store shares the same connection (P0-B fix)
    let obs_store = SqliteObservationStore::new(db.conn.clone());
    let obs = Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: "test_ws".to_string(),
        memory_id: memories[0].memory_id.clone(),
        subject_text: "POSMASK".to_string(),
        subject_type: Some("database_table".to_string()),
        predicate: "not_has_field".to_string(),
        object_text: Some("机器字段".to_string()),
        object_type: Some("field".to_string()),
        evidence_text: Some("POSMASK 表没有机器字段".to_string()),
        extraction_confidence: 0.7,
        memory_type_candidate: None,
        observation_detail_json: None,
        evidence_alpha: 1.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type: ObservationSourceType::UserMessage,
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    obs_store.insert(&obs).unwrap();

    // Step 4: Retrieve and verify
    let retrieved_obs = obs_store.get(&obs.observation_id).unwrap().unwrap();
    assert_eq!(retrieved_obs.subject_text, "POSMASK");
    assert_eq!(retrieved_obs.predicate, "not_has_field");

    let all_obs = obs_store.list_by_workspace("test_ws", None).unwrap();
    assert_eq!(all_obs.len(), 1);

    let by_entity = obs_store.find_by_entity("POSMASK", "test_ws").unwrap();
    assert_eq!(by_entity.len(), 1);

    let is_dup = obs_store
        .check_duplicate("POSMASK", "not_has_field", Some("机器字段"), "test_ws")
        .unwrap();
    assert!(is_dup);
    let not_dup = obs_store
        .check_duplicate("OTHER", "not_has_field", Some("机器字段"), "test_ws")
        .unwrap();
    assert!(!not_dup);
}

#[test]
fn test_confidence_integration() {
    let mut bc = BetaConfidence::new();
    assert!((bc.confidence() - 0.5).abs() < f64::EPSILON);

    bc.update(&EvidenceType::UserConfirmation);
    assert!(bc.confidence() > 0.5);

    bc.update(&EvidenceType::UserNegation);
    assert!(bc.confidence() < 1.0);
}

#[test]
fn test_entity_normalization_integration() {
    assert_eq!(canonical_key("User Model"), canonical_key("user_model"));
    assert_eq!(canonical_key("User Model"), canonical_key("usermodel"));
    assert_eq!(canonical_key("User Model"), canonical_key("UserModel"));
    assert_eq!(canonical_key("User Model"), canonical_key("user-model"));
}

#[test]
fn test_trellis_journal_ingest() {
    let journal = r#"# Journal - test (Part 1)

## Session 1: Test Session

**Date**: 2026-05-12
**Task**: Test task

### Summary

This is a test summary with some technical details.

### Main Changes

- Changed UserModel to use SQLite
- Fixed auth handler bug
"#;

    let memories = TrellisJournalParser
        .parse(journal, "ws1", "journal.md")
        .unwrap();
    assert!(!memories.is_empty());
    assert!(memories.iter().all(|m| m.workspace_id == "ws1"));
    assert!(memories
        .iter()
        .all(|m| m.source_type == SourceType::TrellisJournal));
}

#[tokio::test]
async fn test_extract_with_mock_llm() {
    use memory_test_fixtures::mock_llm::MockLlmProvider;

    let mock = MockLlmProvider::new().with_response(
        "POSMASK",
        r#"{"observations":[{"subject_text":"POSMASK","subject_type":"database_table","predicate":"not_has_field","object_text":"机器字段","object_type":"field","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message","confidence":0.9}]}"#,
    );

    let raw = make_test_raw_memory("ws", "s1", "user", "POSMASK 表没有机器字段");
    let observations = extract_observations(&[raw], &mock).await.unwrap();

    assert_eq!(observations.len(), 1);
    // subject_text normalized via canonical_key_light ("POSMASK" -> "posmask")
    assert_eq!(observations[0].subject_text, "posmask");
    assert_eq!(observations[0].predicate, "not_has_field");
    assert_eq!(
        observations[0].source_type,
        ObservationSourceType::UserMessage
    );
}

#[tokio::test]
async fn test_extract_and_dedup_filters_duplicates() {
    use memory_runtime::pipeline::extract::extract_and_dedup;
    use memory_test_fixtures::mock_llm::MockLlmProvider;

    let db = Database::open_in_memory().unwrap();
    let raw = make_test_raw_memory("ws", "s1", "user", "POSMASK 表没有机器字段");
    {
        let conn = db.conn.lock();
        seed_raw_memories(&conn, &[raw.clone()]);
    }
    let obs_store = SqliteObservationStore::new(db.conn.clone());
    let mock = MockLlmProvider::new().with_response(
        "POSMASK",
        r#"{"observations":[{"subject_text":"POSMASK","predicate":"not_has_field","object_text":"机器字段","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"}]}"#,
    );

    // First extraction: store empty -> keeps the observation.
    let first = extract_and_dedup(&[raw.clone()], &mock, &obs_store)
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    obs_store.insert_batch(&first).unwrap();

    // Re-extracting identical content: subject normalizes to the same "posmask" key,
    // check_duplicate hits -> dropped.
    let second = extract_and_dedup(&[raw], &mock, &obs_store).await.unwrap();
    assert!(
        second.is_empty(),
        "re-extraction of identical content should yield no new observations"
    );
}

#[test]
fn test_batch_insert_and_query() {
    let db = Database::open_in_memory().unwrap();

    // Seed raw memories first (FK constraint)
    let raw_memories: Vec<RawMemory> = (0..5)
        .map(|i| RawMemory {
            memory_id: format!("mem_{i}"),
            workspace_id: "ws".to_string(),
            session_id: "test".to_string(),
            role: "user".to_string(),
            content: format!("content {i}"),
            source_type: SourceType::SessionFile,
            source_ref: "test".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .collect();
    {
        let conn = db.conn.lock();
        seed_raw_memories(&conn, &raw_memories);
    }

    // Observation store shares the same connection (P0-B fix)
    let obs_store = SqliteObservationStore::new(db.conn.clone());
    let observations: Vec<Observation> = (0..5)
        .map(|i| Observation {
            observation_id: uuid::Uuid::new_v4().to_string(),
            workspace_id: "ws".to_string(),
            memory_id: format!("mem_{i}"),
            subject_text: format!("entity_{i}"),
            subject_type: None,
            predicate: "has_value".to_string(),
            object_text: Some(format!("value_{i}")),
            object_type: None,
            evidence_text: None,
            extraction_confidence: 0.7,
            memory_type_candidate: None,
            observation_detail_json: None,
            evidence_alpha: 1.0,
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: ObservationSourceType::UserMessage,
            consolidated: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .collect();

    obs_store.insert_batch(&observations).unwrap();

    let all = obs_store.list_by_workspace("ws", None).unwrap();
    assert_eq!(all.len(), 5);

    let by_entity = obs_store.find_by_entity("entity_2", "ws").unwrap();
    assert_eq!(by_entity.len(), 1);
}

#[test]
fn test_raw_memory_store_crud() {
    let db = Database::open_in_memory().unwrap();
    let store = SqliteRawMemoryStore::new(db.conn.clone());

    let raw = make_test_raw_memory("ws1", "session_abc", "user", "test content");
    store.insert(&raw).unwrap();

    let by_session = store.get_by_session("session_abc").unwrap();
    assert_eq!(by_session.len(), 1);
    assert_eq!(by_session[0].content, "test content");

    let by_workspace = store.list_by_workspace("ws1", 10).unwrap();
    assert_eq!(by_workspace.len(), 1);

    // Different workspace returns empty
    let empty = store.list_by_workspace("ws2", 10).unwrap();
    assert!(empty.is_empty());
}

/// P0-B regression: a single Database must back both RawMemoryStore and
/// ObservationStore simultaneously. Before the Arc<Mutex> refactor, constructing
/// the second store moved the Connection out of the first.
#[test]
fn test_multiple_stores_share_connection() {
    let db = Database::open_in_memory().unwrap();

    let raw_store = SqliteRawMemoryStore::new(db.conn.clone());
    let obs_store = SqliteObservationStore::new(db.conn.clone());

    // Insert a raw memory through the raw store
    let raw = make_test_raw_memory("ws1", "shared_session", "user", "shared connection works");
    raw_store.insert(&raw).unwrap();

    // Insert an observation referencing that raw memory through the obs store.
    // The FK constraint is satisfied only because both stores see the same DB.
    let obs = Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: "ws1".to_string(),
        memory_id: raw.memory_id.clone(),
        subject_text: "connection".to_string(),
        subject_type: None,
        predicate: "is_shared".to_string(),
        object_text: Some("true".to_string()),
        object_type: None,
        evidence_text: None,
        extraction_confidence: 0.7,
        memory_type_candidate: None,
        observation_detail_json: None,
        evidence_alpha: 1.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.0,
        source_type: ObservationSourceType::UserMessage,
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    obs_store.insert(&obs).unwrap();

    // Both stores read the shared state
    assert_eq!(raw_store.get_by_session("shared_session").unwrap().len(), 1);
    assert_eq!(obs_store.list_by_workspace("ws1", None).unwrap().len(), 1);
}
