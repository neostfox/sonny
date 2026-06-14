use memory_runtime::confidence::{BetaConfidence, EvidenceType};
use memory_runtime::embed::embed_and_store;
use memory_runtime::embed::traits::EmbeddingProvider;
use memory_runtime::entity::canonical_key;
use memory_runtime::models::embedding::EmbeddingSourceType;
use memory_runtime::models::observation::{Observation, ObservationSourceType};
use memory_runtime::models::raw_memory::{RawMemory, SourceType};
use memory_runtime::models::status::ObservationStatus;
use memory_runtime::pipeline::extract::extract_observations;
use memory_runtime::pipeline::ingest::{detect_and_parse, JournalParser, SessionParser};
use memory_runtime::store::connection::Database;
use memory_runtime::store::embedding_store::SqliteEmbeddingStore;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::raw_memory_store::SqliteRawMemoryStore;
use memory_runtime::store::traits::{EmbeddingStore, ObservationStore, RawMemoryStore};
use memory_test_fixtures::stub_embedding::StubEmbeddingService;

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
        extraction_version: None,
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

#[tokio::test]
async fn test_embedding_provider_store_search_roundtrip() {
    let db = Database::open_in_memory().unwrap();
    let emb_store = SqliteEmbeddingStore::new(db.conn.clone());
    let emb = StubEmbeddingService::new(1024);

    embed_and_store(
        &emb,
        &emb_store,
        EmbeddingSourceType::Observation,
        "obs-posmask",
        "ws",
        "POSMASK not_has 机器字段",
    )
    .await
    .unwrap();
    embed_and_store(
        &emb,
        &emb_store,
        EmbeddingSourceType::Observation,
        "obs-other",
        "ws",
        "ORDERHDR has 金额字段",
    )
    .await
    .unwrap();

    let query = emb.embed("POSMASK not_has 机器字段").await.unwrap();
    let hits = emb_store
        .search(query.as_slice().unwrap(), "ws", 2, 0.0)
        .unwrap();

    assert_eq!(emb.dim(), 1024);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].source_id, "obs-posmask");
    assert_eq!(hits[0].source_type, EmbeddingSourceType::Observation);
    assert!(hits[0].score > hits[1].score);
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
        predicate: "not_has".to_string(),
        object_text: Some("机器字段".to_string()),
        object_type: Some("field".to_string()),
        evidence_text: Some("POSMASK 表没有机器字段".to_string()),
        extraction_confidence: 0.7,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: None,
        superseded_by: None,
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
    assert_eq!(retrieved_obs.predicate, "not_has");

    let all_obs = obs_store.list_by_workspace("test_ws", None).unwrap();
    assert_eq!(all_obs.len(), 1);

    let by_entity = obs_store.find_by_entity("POSMASK", "test_ws").unwrap();
    assert_eq!(by_entity.len(), 1);

    let dup = obs_store
        .find_duplicate("POSMASK", "not_has", Some("机器字段"), "test_ws")
        .unwrap();
    assert!(dup.is_some());
    let not_dup = obs_store
        .find_duplicate("OTHER", "not_has_field", Some("机器字段"), "test_ws")
        .unwrap();
    assert!(not_dup.is_none());
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
fn test_journal_ingest() {
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

    let memories = JournalParser.parse(journal, "ws1", "journal.md").unwrap();
    assert!(!memories.is_empty());
    assert!(memories.iter().all(|m| m.workspace_id == "ws1"));
    assert!(memories
        .iter()
        .all(|m| m.source_type == SourceType::Journal));
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
    assert_eq!(observations[0].predicate, "not_has");
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
        seed_raw_memories(&conn, std::slice::from_ref(&raw));
    }
    let obs_store = SqliteObservationStore::new(db.conn.clone());
    let mock = MockLlmProvider::new().with_response(
        "POSMASK",
        r#"{"observations":[{"subject_text":"POSMASK","predicate":"not_has_field","object_text":"机器字段","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"}]}"#,
    );

    // First extraction: store empty -> keeps the observation.
    let first = extract_and_dedup(std::slice::from_ref(&raw), &mock, &obs_store)
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    obs_store.insert_batch(&first).unwrap();

    // Re-extracting identical content: find_duplicate hits -> the fresh duplicate is
    // dropped AND the existing observation's evidence is accumulated (P3-D).
    let existing_id = first[0].observation_id.clone();
    let second = extract_and_dedup(&[raw], &mock, &obs_store).await.unwrap();
    assert!(
        second.is_empty(),
        "re-extraction of identical content should yield no new observations"
    );

    // UserMessage seeds no evidence (α stays 1.0); the dedup repeat adds
    // RepeatedOccurrence (+1.0 α), so the existing observation's α is now 2.0.
    let bumped = obs_store.get(&existing_id).unwrap().unwrap();
    assert_eq!(bumped.evidence_alpha, 2.0);
    assert_eq!(bumped.evidence_beta, 1.0);
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
            extraction_version: None,
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
            extraction_batch_id: None,
            superseded_by: None,
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
        extraction_batch_id: None,
        superseded_by: None,
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

/// P2-C: observations sharing an extraction_batch_id are reachable via find_coclaim,
/// and the coclaim relation does not leak across batches.
#[test]
fn test_coclaim_links_same_batch_observations() {
    let db = Database::open_in_memory().unwrap();

    // Seed raw memories (FK constraint on observation.memory_id).
    let raws: Vec<RawMemory> = (0..4)
        .map(|i| RawMemory {
            memory_id: format!("mem_{i}"),
            workspace_id: "ws".to_string(),
            session_id: "s1".to_string(),
            role: "user".to_string(),
            content: format!("content {i}"),
            source_type: SourceType::SessionFile,
            source_ref: "t".to_string(),
            extraction_version: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .collect();
    {
        let conn = db.conn.lock();
        seed_raw_memories(&conn, &raws);
    }

    let obs_store = SqliteObservationStore::new(db.conn.clone());

    let mk = |i: usize, batch: Option<&str>| Observation {
        observation_id: format!("obs_{i}"),
        workspace_id: "ws".to_string(),
        memory_id: format!("mem_{i}"),
        subject_text: format!("entity_{i}"),
        subject_type: None,
        predicate: "has_value".to_string(),
        object_text: Some(format!("value_{i}")),
        object_type: None,
        evidence_text: None,
        extraction_confidence: 0.7,
        evidence_alpha: 1.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type: ObservationSourceType::UserMessage,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: batch.map(|b| b.to_string()),
        superseded_by: None,
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    // Batch A: obs_0, obs_1, obs_2 share a batch. obs_3 sits alone in batch B.
    let batch = vec![
        mk(0, Some("batch_a")),
        mk(1, Some("batch_a")),
        mk(2, Some("batch_a")),
        mk(3, Some("batch_b")),
    ];
    obs_store.insert_batch(&batch).unwrap();

    // find_coclaim(obs_0) returns the two batch_a siblings, not the batch_b one.
    let siblings = obs_store.find_coclaim("obs_0").unwrap();
    let sibling_ids: Vec<&str> = siblings.iter().map(|o| o.observation_id.as_str()).collect();
    assert_eq!(sibling_ids.len(), 2);
    assert!(sibling_ids.contains(&"obs_1"));
    assert!(sibling_ids.contains(&"obs_2"));
    assert!(!sibling_ids.contains(&"obs_3"));

    // An observation whose batch has no partners returns empty.
    let lone = obs_store.find_coclaim("obs_3").unwrap();
    assert!(lone.is_empty());
}

/// P2-D: reextract supersedes a session's old observations and stamps the new prompt
/// version, leaving the old rows traceable via `superseded_by`.
#[tokio::test]
async fn test_reextract_supersedes_old_observations() {
    use memory_runtime::pipeline::extract::{reextract, EXTRACTION_PROMPT_VERSION};
    use memory_test_fixtures::mock_llm::MockLlmProvider;

    let db = Database::open_in_memory().unwrap();
    let raw_store = SqliteRawMemoryStore::new(db.conn.clone());
    let obs_store = SqliteObservationStore::new(db.conn.clone());

    // One grounded raw memory in session s1.
    let raw = make_test_raw_memory("ws", "s1", "user", "POSMASK 表没有机器字段");
    raw_store.insert(&raw).unwrap();

    // --- v1 extraction (old prompt): WRONGLY extracts "has_field" from a negation. ---
    let v1 = MockLlmProvider::new().with_response(
        "POSMASK",
        r#"{"observations":[{"subject_text":"POSMASK","predicate":"has_field","object_text":"机器字段","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"}]}"#,
    );
    let first = extract_observations(std::slice::from_ref(&raw), &v1)
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    obs_store.insert_batch(&first).unwrap();
    // Simulate a prior extraction with an older prompt version.
    raw_store
        .set_session_extraction_version("s1", "2026-01-01.v0")
        .unwrap();
    let old_id = first[0].observation_id.clone();
    assert_eq!(
        raw_store
            .session_extraction_version("s1")
            .unwrap()
            .as_deref(),
        Some("2026-01-01.v0")
    );

    // --- v2 re-extraction (new prompt): correctly extracts "not_has_field". ---
    let v2 = MockLlmProvider::new().with_response(
        "POSMASK",
        r#"{"observations":[{"subject_text":"POSMASK","predicate":"not_has_field","object_text":"机器字段","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"}]}"#,
    );
    let outcome = reextract("s1", &v2, &raw_store, &obs_store).await.unwrap();
    assert_eq!(
        outcome.superseded, 1,
        "the single old observation must be superseded"
    );
    assert_eq!(outcome.new_observations.len(), 1);
    let new_batch = outcome.new_observations[0]
        .extraction_batch_id
        .clone()
        .expect("new observation has a batch id");

    // Old row is retained but Superseded, pointing at the replacement batch.
    let old = obs_store.get(&old_id).unwrap().unwrap();
    assert_eq!(old.status, ObservationStatus::Superseded);
    assert_eq!(old.superseded_by.as_deref(), Some(new_batch.as_str()));

    // The new observation is the only live (Candidate) one; superseded is filtered out.
    let active = obs_store
        .list_by_workspace("ws", Some(ObservationStatus::Candidate))
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].predicate, "not_has");
    let superseded = obs_store
        .list_by_workspace("ws", Some(ObservationStatus::Superseded))
        .unwrap();
    assert_eq!(superseded.len(), 1);

    // Session is now stamped with the current prompt version.
    assert_eq!(
        raw_store
            .session_extraction_version("s1")
            .unwrap()
            .as_deref(),
        Some(EXTRACTION_PROMPT_VERSION)
    );
}

/// F2: find_coclaim excludes siblings in dead statuses (superseded/rejected/deprecated)
/// so clustering never pulls rows whose coclaim edges linger after re-extraction.
#[test]
fn test_find_coclaim_excludes_dead_statuses() {
    let db = Database::open_in_memory().unwrap();

    let raws: Vec<RawMemory> = (0..2)
        .map(|i| RawMemory {
            memory_id: format!("mem_{i}"),
            workspace_id: "ws".to_string(),
            session_id: "s1".to_string(),
            role: "user".to_string(),
            content: format!("content {i}"),
            source_type: SourceType::SessionFile,
            source_ref: "t".to_string(),
            extraction_version: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .collect();
    {
        let conn = db.conn.lock();
        seed_raw_memories(&conn, &raws);
    }
    let obs_store = SqliteObservationStore::new(db.conn.clone());

    // Two co-claimed observations in one batch.
    let mk = |i: usize| Observation {
        observation_id: format!("obs_{i}"),
        workspace_id: "ws".to_string(),
        memory_id: format!("mem_{i}"),
        subject_text: format!("entity_{i}"),
        subject_type: None,
        predicate: "has_value".to_string(),
        object_text: Some(format!("value_{i}")),
        object_type: None,
        evidence_text: None,
        extraction_confidence: 0.7,
        evidence_alpha: 1.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type: ObservationSourceType::UserMessage,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: Some("batch_x".to_string()),
        superseded_by: None,
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    obs_store.insert_batch(&[mk(0), mk(1)]).unwrap();

    // Both candidates -> find_coclaim returns the sibling.
    assert_eq!(obs_store.find_coclaim("obs_0").unwrap().len(), 1);

    // Supersede obs_1: its coclaim edge remains, but find_coclaim must now exclude it.
    obs_store
        .update_status("obs_1", ObservationStatus::Superseded)
        .unwrap();
    assert!(
        obs_store.find_coclaim("obs_0").unwrap().is_empty(),
        "superseded siblings must be filtered from find_coclaim"
    );
}
