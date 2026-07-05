//! SQLite-backed concept edge store (P5-A).

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::LazyLock;

use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::confidence::EvidenceType;
use crate::error::MemoryResult;
use crate::models::hierarchy::RelationType;
use crate::models::relation::{canonical_pair, ConceptRelation, RelationLifecycle};

use super::traits::RelationStore;

pub struct SqliteRelationStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRelationStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const RELATION_COLUMNS: &str = "\
    relation_id, workspace_id, src_concept_id, dst_concept_id, relation_type, lifecycle, \
    evidence_alpha, evidence_beta, evidence_count, last_evidence_at, created_at, updated_at";

static RELATION_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO concept_relation ({RELATION_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)")
});
static RELATION_GET: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {RELATION_COLUMNS} FROM concept_relation WHERE workspace_id = ?1 AND src_concept_id = ?2 AND dst_concept_id = ?3 AND relation_type = ?4")
});
static RELATION_NEIGHBORS: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {RELATION_COLUMNS} FROM concept_relation WHERE workspace_id = ?1 AND (src_concept_id = ?2 OR dst_concept_id = ?2) ORDER BY evidence_alpha / (evidence_alpha + evidence_beta) DESC")
});
static RELATION_LIST: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {RELATION_COLUMNS} FROM concept_relation WHERE workspace_id = ?1 ORDER BY created_at, src_concept_id, dst_concept_id")
});
const RELATION_UPDATE: &str = "UPDATE concept_relation SET lifecycle = ?5, evidence_alpha = ?6, \
    evidence_beta = ?7, evidence_count = ?8, last_evidence_at = ?9, updated_at = ?10 \
    WHERE workspace_id = ?1 AND src_concept_id = ?2 AND dst_concept_id = ?3 AND relation_type = ?4";
const CONNECTION_COUNT_BUMP: &str =
    "UPDATE concept SET connection_count = connection_count + 1 WHERE concept_id IN (?1, ?2)";

fn row_to_relation(row: &Row<'_>) -> rusqlite::Result<ConceptRelation> {
    let relation_type: String = row.get(4)?;
    let lifecycle: String = row.get(5)?;
    // Strict parse: an enum value outside the vocabulary must surface as an
    // error, not silently coerce (a directed edge read back as symmetric would
    // flip direction semantics without any signal).
    let relation_type = relation_type.parse::<RelationType>().map_err(|()| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("unknown relation_type: {relation_type}").into(),
        )
    })?;
    let lifecycle = lifecycle.parse::<RelationLifecycle>().map_err(|()| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            format!("unknown lifecycle: {lifecycle}").into(),
        )
    })?;
    Ok(ConceptRelation {
        relation_id: row.get(0)?,
        workspace_id: row.get(1)?,
        src_concept_id: row.get(2)?,
        dst_concept_id: row.get(3)?,
        relation_type,
        lifecycle,
        evidence_alpha: row.get(6)?,
        evidence_beta: row.get(7)?,
        evidence_count: row.get(8)?,
        last_evidence_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

impl RelationStore for SqliteRelationStore {
    fn record_evidence(
        &self,
        workspace_id: &str,
        src_concept_id: &str,
        dst_concept_id: &str,
        relation_type: RelationType,
        evidence: &EvidenceType,
    ) -> MemoryResult<ConceptRelation> {
        let (src, dst) = canonical_pair(src_concept_id, dst_concept_id, relation_type);
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        // One transaction: insert + connection_count bump + evidence update must
        // land together or not at all (crash consistency).
        let tx = conn.unchecked_transaction()?;

        let existing = tx
            .query_row(
                &RELATION_GET,
                params![workspace_id, src, dst, relation_type.as_str()],
                row_to_relation,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;

        let mut edge = match existing {
            Some(edge) => edge,
            None => {
                let edge = ConceptRelation::new(workspace_id, src, dst, relation_type, &now);
                tx.execute(
                    &RELATION_INSERT,
                    params![
                        &edge.relation_id,
                        &edge.workspace_id,
                        &edge.src_concept_id,
                        &edge.dst_concept_id,
                        edge.relation_type.as_str(),
                        edge.lifecycle.as_str(),
                        &edge.evidence_alpha,
                        &edge.evidence_beta,
                        &edge.evidence_count,
                        &edge.last_evidence_at,
                        &edge.created_at,
                        &edge.updated_at,
                    ],
                )?;
                tx.execute(CONNECTION_COUNT_BUMP, params![src, dst])?;
                edge
            }
        };

        edge.add_evidence(evidence, &now);
        tx.execute(
            RELATION_UPDATE,
            params![
                &edge.workspace_id,
                &edge.src_concept_id,
                &edge.dst_concept_id,
                edge.relation_type.as_str(),
                edge.lifecycle.as_str(),
                &edge.evidence_alpha,
                &edge.evidence_beta,
                &edge.evidence_count,
                &edge.last_evidence_at,
                &edge.updated_at,
            ],
        )?;
        tx.commit()?;
        Ok(edge)
    }

    fn get_edge(
        &self,
        workspace_id: &str,
        src_concept_id: &str,
        dst_concept_id: &str,
        relation_type: RelationType,
    ) -> MemoryResult<Option<ConceptRelation>> {
        let (src, dst) = canonical_pair(src_concept_id, dst_concept_id, relation_type);
        let conn = self.conn.lock();
        let result = conn.query_row(
            &RELATION_GET,
            params![workspace_id, src, dst, relation_type.as_str()],
            row_to_relation,
        );
        match result {
            Ok(edge) => Ok(Some(edge)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn neighbors(
        &self,
        workspace_id: &str,
        concept_id: &str,
    ) -> MemoryResult<Vec<ConceptRelation>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&RELATION_NEIGHBORS)?;
        let rows = stmt.query_map(params![workspace_id, concept_id], row_to_relation)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn list_by_workspace(&self, workspace_id: &str) -> MemoryResult<Vec<ConceptRelation>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&RELATION_LIST)?;
        let rows = stmt.query_map(params![workspace_id], row_to_relation)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::concept::Concept;
    use crate::models::status::ConceptStatus;
    use crate::store::concept_store::SqliteConceptStore;
    use crate::store::connection::Database;
    use crate::store::traits::ConceptStore;

    fn store() -> (Database, SqliteRelationStore, SqliteConceptStore) {
        let db = Database::open_in_memory().unwrap();
        let relations = SqliteRelationStore::new(db.conn.clone());
        let concepts = SqliteConceptStore::new(db.conn.clone());
        (db, relations, concepts)
    }

    /// Seed endpoint concepts so edges satisfy the concept_relation FK
    /// (migration 009). The FK checks `concept_id` only, so workspace is
    /// irrelevant here.
    fn seed(concepts: &SqliteConceptStore, ids: &[&str]) {
        for id in ids {
            concepts.insert_concept(&concept(id)).unwrap();
        }
    }

    fn concept(id: &str) -> Concept {
        Concept {
            concept_id: id.to_string(),
            workspace_id: "ws".to_string(),
            name: id.to_string(),
            concept_type: None,
            definition: None,
            related_entities_json: None,
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: 0.5,
            evidence_alpha: 1.0,
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

    #[test]
    fn record_evidence_creates_then_accumulates() {
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-a", "c-b"]);

        let first = relations
            .record_evidence("ws", "c-a", "c-b", RelationType::Causal, &EvidenceType::FileEvidence)
            .unwrap();
        assert_eq!(first.evidence_count, 1);
        assert!((first.weight() - 2.5 / 3.5).abs() < 1e-9);

        let second = relations
            .record_evidence("ws", "c-a", "c-b", RelationType::Causal, &EvidenceType::FileEvidence)
            .unwrap();
        assert_eq!(second.evidence_count, 2);
        assert_eq!(second.relation_id, first.relation_id);
        assert_eq!(relations.list_by_workspace("ws").unwrap().len(), 1);
    }

    #[test]
    fn symmetric_edge_is_one_row_regardless_of_call_order() {
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-a", "c-b"]);
        relations
            .record_evidence("ws", "c-b", "c-a", RelationType::SharedEntity, &EvidenceType::RepeatedOccurrence)
            .unwrap();
        relations
            .record_evidence("ws", "c-a", "c-b", RelationType::SharedEntity, &EvidenceType::RepeatedOccurrence)
            .unwrap();

        let edges = relations.list_by_workspace("ws").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src_concept_id, "c-a");
        assert_eq!(edges[0].evidence_count, 2);
    }

    #[test]
    fn directed_edges_keep_reverse_direction_distinct() {
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-a", "c-b"]);
        relations
            .record_evidence("ws", "c-b", "c-a", RelationType::Causal, &EvidenceType::RepeatedOccurrence)
            .unwrap();
        relations
            .record_evidence("ws", "c-a", "c-b", RelationType::Causal, &EvidenceType::RepeatedOccurrence)
            .unwrap();
        assert_eq!(relations.list_by_workspace("ws").unwrap().len(), 2);
    }

    #[test]
    fn new_edge_bumps_connection_count_once() {
        let (_db, relations, concepts) = store();
        concepts.insert_concept(&concept("c-a")).unwrap();
        concepts.insert_concept(&concept("c-b")).unwrap();

        for _ in 0..3 {
            relations
                .record_evidence("ws", "c-a", "c-b", RelationType::SharedEntity, &EvidenceType::RepeatedOccurrence)
                .unwrap();
        }

        assert_eq!(concepts.get_concept("c-a").unwrap().unwrap().connection_count, 1);
        assert_eq!(concepts.get_concept("c-b").unwrap().unwrap().connection_count, 1);
    }

    #[test]
    fn neighbors_returns_both_directions_strongest_first() {
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-x", "c-hub", "c-y", "c-other", "c-unrelated"]);
        relations
            .record_evidence("ws", "c-x", "c-hub", RelationType::Causal, &EvidenceType::RepeatedOccurrence)
            .unwrap();
        for _ in 0..3 {
            relations
                .record_evidence("ws", "c-hub", "c-y", RelationType::Causal, &EvidenceType::UserConfirmation)
                .unwrap();
        }
        relations
            .record_evidence("ws", "c-other", "c-unrelated", RelationType::Causal, &EvidenceType::RepeatedOccurrence)
            .unwrap();

        let hub_edges = relations.neighbors("ws", "c-hub").unwrap();
        assert_eq!(hub_edges.len(), 2);
        assert_eq!(hub_edges[0].dst_concept_id, "c-y");
        assert!(hub_edges[0].weight() > hub_edges[1].weight());
    }

    #[test]
    fn unknown_enum_values_error_instead_of_coercing() {
        let (db, relations, concepts) = store();
        seed(&concepts, &["c-a", "c-b"]);
        db.conn
            .lock()
            .execute(
                "INSERT INTO concept_relation (relation_id, workspace_id, src_concept_id, \
                 dst_concept_id, relation_type, lifecycle, created_at, updated_at) \
                 VALUES ('r1', 'ws', 'c-a', 'c-b', 'defenestrates', 'candidate', 't0', 't0')",
                [],
            )
            .unwrap();

        assert!(relations.list_by_workspace("ws").is_err());
    }

    #[test]
    fn dangling_endpoint_is_rejected_by_fk() {
        // P5-A audit + P4-B F2: migration 009 adds src/dst FKs to concept, so an
        // edge to a non-existent concept can no longer be silently created.
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-a"]); // only the source exists

        let err = relations.record_evidence(
            "ws",
            "c-a",
            "c-ghost",
            RelationType::Causal,
            &EvidenceType::FileEvidence,
        );
        assert!(err.is_err(), "edge to a ghost concept must fail the FK");
        assert!(relations.list_by_workspace("ws").unwrap().is_empty());
    }

    #[test]
    fn workspaces_are_isolated() {
        let (_db, relations, concepts) = store();
        seed(&concepts, &["c-a", "c-b"]);
        relations
            .record_evidence("ws-1", "c-a", "c-b", RelationType::Causal, &EvidenceType::RepeatedOccurrence)
            .unwrap();
        assert!(relations.list_by_workspace("ws-2").unwrap().is_empty());
        assert!(relations
            .get_edge("ws-2", "c-a", "c-b", RelationType::Causal)
            .unwrap()
            .is_none());
    }
}
