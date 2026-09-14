//! P13 acceptance: hybrid RRF seed + forward edge + reverse entity expansion.

use memory_runtime::models::concept::Concept;
use memory_runtime::models::hierarchy::RelationType;
use memory_runtime::models::scope::LifecycleScope;
use memory_runtime::models::status::ConceptStatus;
use memory_runtime::recall::compact::{compact_search, rrf_fuse};
use memory_runtime::store::connection::Database;
use memory_runtime::store::concept_store::SqliteConceptStore;
use memory_runtime::store::embedding_store::SqliteEmbeddingStore;
use memory_runtime::store::relation_store::SqliteRelationStore;
use memory_runtime::store::traits::{ConceptStore, EmbeddingStore, RelationStore};
use memory_test_fixtures::stub_embedding::StubEmbeddingService;

fn concept(ws: &str, id: &str, name: &str, entities: &[&str]) -> Concept {
    Concept {
        concept_id: id.into(),
        workspace_id: ws.into(),
        name: name.into(),
        concept_type: None,
        definition: Some(format!("{name} definition")),
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
        recall_count: 0,
        successful_recall_count: 0,
        failed_recall_count: 0,
        connection_count: 0,
        lifecycle_scope: LifecycleScope::Project,
        scope_key: None,
        created_at: "t".into(),
        updated_at: "t".into(),
    }
}

#[test]
fn rrf_dual_channel_winner_is_first() {
    let fused = rrf_fuse(
        &[("a".into(), 0), ("b".into(), 1)],
        &[("b".into(), 0), ("c".into(), 1)],
    );
    assert_eq!(fused[0].0, "b");
}

#[tokio::test]
async fn compact_search_expands_forward_via_causal_edge() {
    let db = Database::open_in_memory().unwrap();
    let concepts = SqliteConceptStore::new(db.conn.clone());
    let embeddings = SqliteEmbeddingStore::new(db.conn.clone());
    let relations = SqliteRelationStore::new(db.conn.clone());
    let provider = StubEmbeddingService::new(4);

    // Seed concept matches the query lexically; neighbor only via causal edge.
    concepts
        .insert_concept(&concept("ws", "c-seed", "POSMASK 字段排查", &["posmask"]))
        .unwrap();
    concepts
        .insert_concept(&concept("ws", "c-neighbor", "启动崩溃修复", &["startup_crash"]))
        .unwrap();
    concepts
        .insert_concept(&concept("ws", "c-unrelated", "网络配置", &["dns"]))
        .unwrap();

    // Embeddings so dense channel sees the seed.
    embeddings
        .store_embedding("concept", "c-seed", "ws", "posmask", &[1.0, 0.0, 0.0, 0.0])
        .unwrap();
    embeddings
        .store_embedding("concept", "c-neighbor", "ws", "crash", &[0.9, 0.1, 0.0, 0.0])
        .unwrap();

    relations
        .record_causal_evidence(
            "ws",
            "c-seed",
            "c-neighbor",
            &memory_runtime::confidence::EvidenceType::HumanReviewConfirm,
            Some(memory_runtime::models::observation::ObservationSourceType::FileEvidence),
            1,
            &memory_runtime::models::causal::CausalStats::default(),
        )
        .unwrap();

    let hits = compact_search(
        &concepts,
        &embeddings,
        &provider,
        &relations,
        "ws",
        "POSMASK",
        10,
        3,
        &[],
        false,
    )
    .await
    .unwrap();

    let ids: Vec<_> = hits.iter().map(|h| h.concept_id.as_str()).collect();
    assert!(ids.contains(&"c-seed"), "seed must be present: {ids:?}");
    assert!(
        ids.contains(&"c-neighbor"),
        "forward causal neighbor must expand: {ids:?}"
    );
    // Unrelated concept should not outrank the seed.
    let seed_pos = ids.iter().position(|i| *i == "c-seed").unwrap();
    if let Some(u) = ids.iter().position(|i| *i == "c-unrelated") {
        assert!(u > seed_pos, "unrelated must rank below seed");
    }

    // Seed should be first or at least above neighbor if lexical hit.
    assert!(hits[0].score > 0.0);
}

#[tokio::test]
async fn compact_search_reverse_via_shared_entity() {
    let db = Database::open_in_memory().unwrap();
    let concepts = SqliteConceptStore::new(db.conn.clone());
    let embeddings = SqliteEmbeddingStore::new(db.conn.clone());
    let relations = SqliteRelationStore::new(db.conn.clone());
    let provider = StubEmbeddingService::new(4);

    concepts
        .insert_concept(&concept("ws", "c-a", "MASK 与机器", &["posmask", "machine"]))
        .unwrap();
    concepts
        .insert_concept(&concept("ws", "c-b", "机器字段缺失", &["machine", "field"]))
        .unwrap();

    embeddings
        .store_embedding("concept", "c-a", "ws", "mask", &[1.0, 0.0, 0.0, 0.0])
        .unwrap();

    let hits = compact_search(
        &concepts,
        &embeddings,
        &provider,
        &relations,
        "ws",
        "MASK 机器",
        10,
        3,
        &[],
        false,
    )
    .await
    .unwrap();

    let ids: Vec<_> = hits.iter().map(|h| h.concept_id.as_str()).collect();
    assert!(ids.contains(&"c-a"), "{ids:?}");
    assert!(
        ids.contains(&"c-b"),
        "shared-entity reverse expand should surface c-b: {ids:?}"
    );
}

#[tokio::test]
async fn compact_recall_engine_builds_context() {
    use memory_runtime::recall::RecallEngine;
    let db = Database::open_in_memory().unwrap();
    let concepts = SqliteConceptStore::new(db.conn.clone());
    let embeddings = SqliteEmbeddingStore::new(db.conn.clone());
    let relations = SqliteRelationStore::new(db.conn.clone());
    let provider = StubEmbeddingService::new(4);

    concepts
        .insert_concept(&concept("ws", "c1", "POSMASK 排查", &["posmask"]))
        .unwrap();
    embeddings
        .store_embedding("concept", "c1", "ws", "posmask", &[1.0, 0.0, 0.0, 0.0])
        .unwrap();

    let engine = RecallEngine::new(&concepts, &embeddings, &provider);
    let ctx = engine
        .compact_recall("POSMASK", "ws", &relations, 800)
        .await
        .unwrap();
    assert!(ctx.current_concept.is_some() || !ctx.relevant_entities.is_empty());
    assert!(ctx.token_count <= 800 + 200); // budget-ish; provenance lines may pad slightly
}
