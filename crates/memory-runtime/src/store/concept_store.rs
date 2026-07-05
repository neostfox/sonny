use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::LazyLock;

use rusqlite::{params, Connection};

use crate::error::MemoryResult;
use crate::models::concept::{Concept, ConceptCandidate, ConceptType};
use crate::models::status::{CandidateStatus, ConceptStatus};

use super::traits::ConceptStore;

pub struct SqliteConceptStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteConceptStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const CANDIDATE_COLUMNS: &str = "\
    candidate_id, workspace_id, name, summary, source_terms_json, source_sessions_json, \
    source_observations_json, known_facts_json, rejected_hypotheses_json, open_questions_json, \
    evidence_json, evidence_count, confidence, evidence_alpha, evidence_beta, status, \
    last_recalled_at, recall_count, successful_recall_count, failed_recall_count, created_at, updated_at";

const CONCEPT_COLUMNS: &str = "\
    concept_id, workspace_id, name, concept_type, definition, related_entities_json, \
    known_facts_json, rejected_hypotheses_json, open_questions_json, evidence_json, confidence, \
    evidence_alpha, evidence_beta, status, parent_concept_id, hierarchy_depth, \
    last_recalled_at, recall_count, successful_recall_count, failed_recall_count, \
    connection_count, created_at, updated_at";

static CANDIDATE_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO concept_candidate ({CANDIDATE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)")
});
static CANDIDATE_GET: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {CANDIDATE_COLUMNS} FROM concept_candidate WHERE candidate_id = ?1")
});
static CANDIDATE_LIST: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {CANDIDATE_COLUMNS} FROM concept_candidate WHERE workspace_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY created_at DESC")
});
static CONCEPT_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO concept ({CONCEPT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)")
});
static CONCEPT_GET: LazyLock<String> =
    LazyLock::new(|| format!("SELECT {CONCEPT_COLUMNS} FROM concept WHERE concept_id = ?1"));
static CONCEPT_LIST: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {CONCEPT_COLUMNS} FROM concept WHERE workspace_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY created_at DESC")
});
static CONCEPT_COLS_QUALIFIED: LazyLock<String> = LazyLock::new(|| {
    CONCEPT_COLUMNS
        .split(", ")
        .map(|c| format!("c.{c}"))
        .collect::<Vec<_>>()
        .join(", ")
});

impl ConceptStore for SqliteConceptStore {
    fn insert_candidate(&self, candidate: &ConceptCandidate) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let status = candidate.status.as_str();
        conn.execute(
            &CANDIDATE_INSERT,
            params![
                &candidate.candidate_id,
                &candidate.workspace_id,
                &candidate.name,
                &candidate.summary,
                &candidate.source_terms_json,
                &candidate.source_sessions_json,
                &candidate.source_observations_json,
                &candidate.known_facts_json,
                &candidate.rejected_hypotheses_json,
                &candidate.open_questions_json,
                &candidate.evidence_json,
                &candidate.evidence_count,
                &candidate.confidence,
                &candidate.evidence_alpha,
                &candidate.evidence_beta,
                &status,
                &candidate.last_recalled_at,
                &candidate.recall_count,
                &candidate.successful_recall_count,
                &candidate.failed_recall_count,
                &candidate.created_at,
                &candidate.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_candidate(&self, candidate_id: &str) -> MemoryResult<Option<ConceptCandidate>> {
        let conn = self.conn.lock();
        let result = conn.query_row(&CANDIDATE_GET, params![candidate_id], row_to_candidate);
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_candidates(
        &self,
        workspace_id: &str,
        status: Option<CandidateStatus>,
    ) -> MemoryResult<Vec<ConceptCandidate>> {
        let conn = self.conn.lock();
        let s = status.map(|st| st.as_str());
        let result = map_rows(
            &conn,
            &CANDIDATE_LIST,
            params![workspace_id, s],
            row_to_candidate,
        )?;
        Ok(result)
    }

    fn insert_concept(&self, concept: &Concept) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let status = concept.status.as_str();
        let concept_type = concept.concept_type.as_ref().map(|t| t.as_str());
        conn.execute(
            &CONCEPT_INSERT,
            params![
                &concept.concept_id,
                &concept.workspace_id,
                &concept.name,
                &concept_type,
                &concept.definition,
                &concept.related_entities_json,
                &concept.known_facts_json,
                &concept.rejected_hypotheses_json,
                &concept.open_questions_json,
                &concept.evidence_json,
                &concept.confidence,
                &concept.evidence_alpha,
                &concept.evidence_beta,
                &status,
                &concept.parent_concept_id,
                &concept.hierarchy_depth,
                &concept.last_recalled_at,
                &concept.recall_count,
                &concept.successful_recall_count,
                &concept.failed_recall_count,
                &concept.connection_count,
                &concept.created_at,
                &concept.updated_at,
            ],
        )?;
        sync_entity_concepts(&conn, concept)?;
        Ok(())
    }

    fn get_concept(&self, concept_id: &str) -> MemoryResult<Option<Concept>> {
        let conn = self.conn.lock();
        let result = conn.query_row(&CONCEPT_GET, params![concept_id], row_to_concept);
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_concepts(
        &self,
        workspace_id: &str,
        status: Option<ConceptStatus>,
    ) -> MemoryResult<Vec<Concept>> {
        let conn = self.conn.lock();
        let s = status.map(|st| st.as_str());
        let result = map_rows(
            &conn,
            &CONCEPT_LIST,
            params![workspace_id, s],
            row_to_concept,
        )?;
        Ok(result)
    }

    fn update_concept(&self, concept: &Concept) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE concept SET name = ?1, concept_type = ?2, definition = ?3, related_entities_json = ?4,
             known_facts_json = ?5, rejected_hypotheses_json = ?6, open_questions_json = ?7, evidence_json = ?8,
             confidence = ?9, evidence_alpha = ?10, evidence_beta = ?11, status = ?12,
             parent_concept_id = ?13, hierarchy_depth = ?14, updated_at = ?15
             WHERE concept_id = ?16",
            params![
                concept.name, concept.concept_type.as_ref().map(|t| t.as_str()),
                concept.definition, concept.related_entities_json, concept.known_facts_json,
                concept.rejected_hypotheses_json, concept.open_questions_json, concept.evidence_json,
                concept.confidence, concept.evidence_alpha, concept.evidence_beta, concept.status.as_str(),
                concept.parent_concept_id, concept.hierarchy_depth, concept.updated_at, concept.concept_id,
            ],
        )?;
        if changed == 0 {
            return Err(crate::error::MemoryError::ConceptNotFound {
                concept_id: concept.concept_id.clone(),
            });
        }
        sync_entity_concepts(&conn, concept)?;
        Ok(())
    }

    fn find_by_entities(
        &self,
        entities: &[String],
        workspace_id: &str,
    ) -> MemoryResult<Vec<Concept>> {
        let conn = self.conn.lock();
        if entities.is_empty() {
            return Ok(vec![]);
        }
        let cols = &*CONCEPT_COLS_QUALIFIED;
        let placeholders: Vec<String> =
            (0..entities.len()).map(|i| format!("?{}", i + 3)).collect();
        // Exact-match JOIN replaces substring LIKE on related_entities_json,
        // so searching "user" no longer matches "user_profile" (C2).
        let sql = format!(
            "SELECT DISTINCT {cols} FROM concept c \
             JOIN entity_concept ec ON ec.concept_id = c.concept_id \
             WHERE c.workspace_id = ?1 AND c.status = ?2 AND ec.entity IN ({}) \
             ORDER BY c.confidence DESC",
            placeholders.join(", ")
        );
        let active = ConceptStatus::Active.as_str();
        let params = rusqlite::params_from_iter(
            [workspace_id, active]
                .into_iter()
                .chain(entities.iter().map(String::as_str)),
        );
        map_rows(&conn, &sql, params, row_to_concept)
    }

    fn record_recall(&self, concept_id: &str) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().to_rfc3339();
        let changed = conn.execute(
            "UPDATE concept SET last_recalled_at = ?1, recall_count = recall_count + 1, updated_at = ?1 WHERE concept_id = ?2",
            params![&now, concept_id],
        )?;
        require_concept_row(changed, concept_id)
    }

    fn record_recall_outcome(&self, concept_id: &str, success: bool) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().to_rfc3339();
        let sql = if success {
            "UPDATE concept SET successful_recall_count = successful_recall_count + 1, updated_at = ?1 WHERE concept_id = ?2"
        } else {
            "UPDATE concept SET failed_recall_count = failed_recall_count + 1, updated_at = ?1 WHERE concept_id = ?2"
        };
        let changed = conn.execute(sql, params![&now, concept_id])?;
        require_concept_row(changed, concept_id)
    }
}

/// H4 (quality-guidelines): recall-stat updates must not silently succeed on a
/// non-existent concept_id.
fn require_concept_row(changed: usize, concept_id: &str) -> MemoryResult<()> {
    if changed == 0 {
        return Err(crate::error::MemoryError::ConceptNotFound {
            concept_id: concept_id.to_string(),
        });
    }
    Ok(())
}

fn map_rows<T, P, F>(conn: &Connection, sql: &str, params: P, map: F) -> MemoryResult<Vec<T>>
where
    P: rusqlite::Params,
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params, map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_to_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConceptCandidate> {
    Ok(ConceptCandidate {
        candidate_id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        summary: row.get(3)?,
        source_terms_json: row.get(4)?,
        source_sessions_json: row.get(5)?,
        source_observations_json: row.get(6)?,
        known_facts_json: row.get(7)?,
        rejected_hypotheses_json: row.get(8)?,
        open_questions_json: row.get(9)?,
        evidence_json: row.get(10)?,
        evidence_count: row.get(11)?,
        confidence: row.get(12)?,
        evidence_alpha: row.get(13)?,
        evidence_beta: row.get(14)?,
        status: parse_candidate_status(&row.get::<_, String>(15)?),
        last_recalled_at: row.get(16)?,
        recall_count: row.get(17)?,
        successful_recall_count: row.get(18)?,
        failed_recall_count: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

fn row_to_concept(row: &rusqlite::Row<'_>) -> rusqlite::Result<Concept> {
    Ok(Concept {
        concept_id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        concept_type: row
            .get::<_, Option<String>>(3)?
            .and_then(|s| parse_concept_type(&s)),
        definition: row.get(4)?,
        related_entities_json: row.get(5)?,
        known_facts_json: row.get(6)?,
        rejected_hypotheses_json: row.get(7)?,
        open_questions_json: row.get(8)?,
        evidence_json: row.get(9)?,
        confidence: row.get(10)?,
        evidence_alpha: row.get(11)?,
        evidence_beta: row.get(12)?,
        status: parse_concept_status(&row.get::<_, String>(13)?),
        parent_concept_id: row.get(14)?,
        hierarchy_depth: row.get(15)?,
        last_recalled_at: row.get(16)?,
        recall_count: row.get(17)?,
        successful_recall_count: row.get(18)?,
        failed_recall_count: row.get(19)?,
        connection_count: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

fn parse_candidate_status(s: &str) -> CandidateStatus {
    s.parse().unwrap_or_else(|_| {
        tracing::warn!("Unknown candidate status '{s}', defaulting to candidate");
        CandidateStatus::Candidate
    })
}

fn parse_concept_status(s: &str) -> ConceptStatus {
    s.parse().unwrap_or_else(|_| {
        tracing::warn!("Unknown concept status '{s}', defaulting to candidate");
        ConceptStatus::Candidate
    })
}

fn parse_concept_type(s: &str) -> Option<ConceptType> {
    match s.parse::<ConceptType>() {
        Ok(t) => Some(t),
        Err(_) => {
            tracing::warn!("Unknown concept type '{s}', defaulting to none");
            None
        }
    }
}

/// Parse a concept's `related_entities_json` into entity strings.
fn parse_entities(json: &Option<String>) -> Vec<String> {
    json.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// Rebuild the entity_concept join table for a concept (delete + reinsert).
fn sync_entity_concepts(conn: &Connection, concept: &Concept) -> MemoryResult<()> {
    conn.execute(
        "DELETE FROM entity_concept WHERE concept_id = ?1",
        params![&concept.concept_id],
    )?;
    for entity in parse_entities(&concept.related_entities_json) {
        conn.execute(
            "INSERT OR IGNORE INTO entity_concept (entity, concept_id, workspace_id) VALUES (?1, ?2, ?3)",
            params![entity, &concept.concept_id, &concept.workspace_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::status::ConceptStatus;
    use crate::store::connection::Database;

    fn make_concept(id: &str, ws: &str, entities: &[&str]) -> Concept {
        Concept {
            concept_id: id.to_string(),
            workspace_id: ws.to_string(),
            name: id.to_string(),
            concept_type: None,
            definition: None,
            related_entities_json: Some(serde_json::to_string(entities).unwrap()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: 0.8,
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
            created_at: "2026-06-13T00:00:00Z".to_string(),
            updated_at: "2026-06-13T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn find_by_entities_no_false_positives() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteConceptStore::new(db.conn.clone());

        // Concept whose only entity is "user_profile"
        let concept = make_concept("c1", "ws", &["user_profile"]);
        store.insert_concept(&concept).unwrap();

        // Searching for "user" must NOT match "user_profile" (the old LIKE bug)
        let hits = store.find_by_entities(&["user".to_string()], "ws").unwrap();
        assert!(
            hits.is_empty(),
            "false positive: 'user' matched concept with entity 'user_profile'"
        );

        // Exact entity match still works
        let hits = store
            .find_by_entities(&["user_profile".to_string()], "ws")
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept_id, "c1");
    }

    #[test]
    fn update_concept_syncs_entities() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteConceptStore::new(db.conn.clone());

        let mut concept = make_concept("c2", "ws", &["alpha"]);
        store.insert_concept(&concept).unwrap();
        assert_eq!(
            store
                .find_by_entities(&["alpha".to_string()], "ws")
                .unwrap()
                .len(),
            1
        );

        // Update changes the entity set
        concept.related_entities_json = Some(serde_json::to_string(&["beta"]).unwrap());
        store.update_concept(&concept).unwrap();

        // Old entity no longer matches; new one does
        assert!(store
            .find_by_entities(&["alpha".to_string()], "ws")
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .find_by_entities(&["beta".to_string()], "ws")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn concept_full_field_round_trip() {
        // Locks the column-positional binding: CONCEPT_COLUMNS order must match the
        // params! order and row_to_concept indices. Any reorder silently corrupts data.
        let db = Database::open_in_memory().unwrap();
        let store = SqliteConceptStore::new(db.conn.clone());

        let concept = Concept {
            concept_id: "rt1".into(),
            workspace_id: "ws".into(),
            name: "Round Trip Concept".into(),
            concept_type: Some(ConceptType::DataAsset),
            definition: Some("a definition".into()),
            related_entities_json: Some(serde_json::to_string(&["e1", "e2"]).unwrap()),
            known_facts_json: Some(serde_json::to_string(&["fact"]).unwrap()),
            rejected_hypotheses_json: Some(serde_json::to_string(&["rej"]).unwrap()),
            open_questions_json: Some(serde_json::to_string(&["q"]).unwrap()),
            evidence_json: Some(serde_json::to_string(&["ev"]).unwrap()),
            confidence: 0.42,
            evidence_alpha: 3.0,
            evidence_beta: 4.0,
            status: ConceptStatus::Labile,
            parent_concept_id: Some("parent".into()),
            hierarchy_depth: 2,
            last_recalled_at: Some("2026-01-01T00:00:00Z".into()),
            recall_count: 5,
            successful_recall_count: 3,
            failed_recall_count: 2,
            connection_count: 7,
            created_at: "2026-06-01T00:00:00Z".into(),
            updated_at: "2026-06-02T00:00:00Z".into(),
        };

        store.insert_concept(&concept).unwrap();
        let got = store
            .get_concept("rt1")
            .unwrap()
            .expect("concept should exist");

        assert_eq!(got.name, "Round Trip Concept");
        assert_eq!(got.concept_type, Some(ConceptType::DataAsset));
        assert_eq!(got.definition.as_deref(), Some("a definition"));
        assert_eq!(got.related_entities_json, concept.related_entities_json);
        assert_eq!(got.known_facts_json, concept.known_facts_json);
        assert_eq!(got.evidence_json, concept.evidence_json);
        assert_eq!(got.confidence, 0.42);
        assert_eq!(got.evidence_alpha, 3.0);
        assert_eq!(got.evidence_beta, 4.0);
        assert_eq!(got.status, ConceptStatus::Labile);
        assert_eq!(got.parent_concept_id.as_deref(), Some("parent"));
        assert_eq!(got.hierarchy_depth, 2);
        assert_eq!(got.recall_count, 5);
        assert_eq!(got.successful_recall_count, 3);
        assert_eq!(got.failed_recall_count, 2);
        assert_eq!(got.connection_count, 7);
    }
}
