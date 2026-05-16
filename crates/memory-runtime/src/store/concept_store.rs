use std::sync::Mutex;

use rusqlite::{Connection, params, params_from_iter};

use crate::error::MemoryResult;
use crate::models::concept::{Concept, ConceptCandidate, ConceptType};
use crate::models::status::{ConceptStatus, ObservationStatus};

use super::traits::ConceptStore;

pub struct SqliteConceptStore {
    conn: Mutex<Connection>,
}

impl SqliteConceptStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
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

fn candidate_params(c: &ConceptCandidate) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(c.candidate_id.clone()),
        Box::new(c.workspace_id.clone()),
        Box::new(c.name.clone()),
        Box::new(c.summary.clone()),
        Box::new(c.source_terms_json.clone()),
        Box::new(c.source_sessions_json.clone()),
        Box::new(c.source_observations_json.clone()),
        Box::new(c.known_facts_json.clone()),
        Box::new(c.rejected_hypotheses_json.clone()),
        Box::new(c.open_questions_json.clone()),
        Box::new(c.evidence_json.clone()),
        Box::new(c.evidence_count),
        Box::new(c.confidence),
        Box::new(c.evidence_alpha),
        Box::new(c.evidence_beta),
        Box::new(c.status.as_str().to_string()),
        Box::new(c.last_recalled_at.clone()),
        Box::new(c.recall_count),
        Box::new(c.successful_recall_count),
        Box::new(c.failed_recall_count),
        Box::new(c.created_at.clone()),
        Box::new(c.updated_at.clone()),
    ]
}

fn concept_params(c: &Concept) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(c.concept_id.clone()),
        Box::new(c.workspace_id.clone()),
        Box::new(c.name.clone()),
        Box::new(c.concept_type.as_ref().map(|t| t.as_str().to_string())),
        Box::new(c.definition.clone()),
        Box::new(c.related_entities_json.clone()),
        Box::new(c.known_facts_json.clone()),
        Box::new(c.rejected_hypotheses_json.clone()),
        Box::new(c.open_questions_json.clone()),
        Box::new(c.evidence_json.clone()),
        Box::new(c.confidence),
        Box::new(c.evidence_alpha),
        Box::new(c.evidence_beta),
        Box::new(c.status.as_str().to_string()),
        Box::new(c.parent_concept_id.clone()),
        Box::new(c.hierarchy_depth),
        Box::new(c.last_recalled_at.clone()),
        Box::new(c.recall_count),
        Box::new(c.successful_recall_count),
        Box::new(c.failed_recall_count),
        Box::new(c.connection_count),
        Box::new(c.created_at.clone()),
        Box::new(c.updated_at.clone()),
    ]
}

impl ConceptStore for SqliteConceptStore {
    fn insert_candidate(&self, candidate: &ConceptCandidate) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let placeholders: Vec<&str> = (1..=22).map(|i| {
            static VALS: [&str; 22] = ["?1","?2","?3","?4","?5","?6","?7","?8","?9","?10","?11","?12","?13","?14","?15","?16","?17","?18","?19","?20","?21","?22"];
            VALS[i - 1]
        }).collect();
        let sql = format!("INSERT INTO concept_candidate ({CANDIDATE_COLUMNS}) VALUES ({})", placeholders.join(", "));
        conn.execute(&sql, params_from_iter(candidate_params(candidate)))?;
        Ok(())
    }

    fn get_candidate(&self, candidate_id: &str) -> MemoryResult<Option<ConceptCandidate>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            &format!("SELECT {CANDIDATE_COLUMNS} FROM concept_candidate WHERE candidate_id = ?1"),
            params![candidate_id],
            row_to_candidate,
        );
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_candidates(&self, workspace_id: &str, status: Option<&str>) -> MemoryResult<Vec<ConceptCandidate>> {
        let conn = self.conn.lock().unwrap();
        let result = if let Some(status) = status {
            let sql = format!("SELECT {CANDIDATE_COLUMNS} FROM concept_candidate WHERE workspace_id = ?1 AND status = ?2 ORDER BY created_at DESC");
            collect_candidates(&conn, &sql, params![workspace_id, status])?
        } else {
            let sql = format!("SELECT {CANDIDATE_COLUMNS} FROM concept_candidate WHERE workspace_id = ?1 ORDER BY created_at DESC");
            collect_candidates(&conn, &sql, params![workspace_id])?
        };
        Ok(result)
    }

    fn insert_concept(&self, concept: &Concept) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let placeholders: Vec<String> = (1..=23).map(|i| format!("?{i}")).collect();
        let sql = format!("INSERT INTO concept ({CONCEPT_COLUMNS}) VALUES ({})", placeholders.join(", "));
        conn.execute(&sql, params_from_iter(concept_params(concept)))?;
        Ok(())
    }

    fn get_concept(&self, concept_id: &str) -> MemoryResult<Option<Concept>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            &format!("SELECT {CONCEPT_COLUMNS} FROM concept WHERE concept_id = ?1"),
            params![concept_id],
            row_to_concept,
        );
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_concepts(&self, workspace_id: &str, status: Option<&str>) -> MemoryResult<Vec<Concept>> {
        let conn = self.conn.lock().unwrap();
        let result = if let Some(status) = status {
            let sql = format!("SELECT {CONCEPT_COLUMNS} FROM concept WHERE workspace_id = ?1 AND status = ?2 ORDER BY created_at DESC");
            collect_concepts(&conn, &sql, params![workspace_id, status])?
        } else {
            let sql = format!("SELECT {CONCEPT_COLUMNS} FROM concept WHERE workspace_id = ?1 ORDER BY created_at DESC");
            collect_concepts(&conn, &sql, params![workspace_id])?
        };
        Ok(result)
    }

    fn update_concept(&self, concept: &Concept) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
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
        Ok(())
    }

    fn find_by_entities(&self, entities: &[String], workspace_id: &str) -> MemoryResult<Vec<Concept>> {
        let conn = self.conn.lock().unwrap();
        if entities.is_empty() {
            return Ok(vec![]);
        }
        let conditions: Vec<String> = entities.iter().enumerate().map(|(i, _)| {
            format!("related_entities_json LIKE '%' || ?{} || '%'", i + 3)
        }).collect();
        let sql = format!(
            "SELECT {CONCEPT_COLUMNS} FROM concept WHERE workspace_id = ?1 AND status = ?2 AND related_entities_json IS NOT NULL AND ({}) ORDER BY confidence DESC",
            conditions.join(" OR ")
        );
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(workspace_id.to_string()),
            Box::new("active".to_string()),
        ];
        for e in entities {
            p.push(Box::new(e.clone()));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
        let result = collect_concepts(&conn, &sql, &param_refs)?;
        Ok(result)
    }

    fn update_recall_stats(&self, concept_id: &str, success: bool) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        if success {
            conn.execute(
                "UPDATE concept SET last_recalled_at = ?1, recall_count = recall_count + 1, successful_recall_count = successful_recall_count + 1, updated_at = ?1 WHERE concept_id = ?2",
                params![&now, concept_id],
            )?;
        } else {
            conn.execute(
                "UPDATE concept SET last_recalled_at = ?1, recall_count = recall_count + 1, failed_recall_count = failed_recall_count + 1, updated_at = ?1 WHERE concept_id = ?2",
                params![&now, concept_id],
            )?;
        }
        Ok(())
    }
}

fn collect_candidates(conn: &Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) -> MemoryResult<Vec<ConceptCandidate>> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<ConceptCandidate> = stmt.query_map(p, row_to_candidate)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn collect_concepts(conn: &Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) -> MemoryResult<Vec<Concept>> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<Concept> = stmt.query_map(p, row_to_concept)?
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
        status: parse_obs_status(&row.get::<_, String>(15)?),
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
        concept_type: row.get::<_, Option<String>>(3)?.map(|s| parse_concept_type(&s)),
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

fn parse_obs_status(s: &str) -> ObservationStatus {
    match s {
        "candidate" => ObservationStatus::Candidate,
        "fast_stored" => ObservationStatus::FastStored,
        "confirmed" => ObservationStatus::Confirmed,
        "auto_confirmed" => ObservationStatus::AutoConfirmed,
        "rejected" => ObservationStatus::Rejected,
        "deprecated" => ObservationStatus::Deprecated,
        "disputed" => ObservationStatus::Disputed,
        "orphan" => ObservationStatus::Orphan,
        _ => ObservationStatus::Candidate,
    }
}

fn parse_concept_status(s: &str) -> ConceptStatus {
    match s {
        "candidate" => ConceptStatus::Candidate,
        "active" => ConceptStatus::Active,
        "labile" => ConceptStatus::Labile,
        "deprecated" => ConceptStatus::Deprecated,
        "disputed" => ConceptStatus::Disputed,
        _ => ConceptStatus::Candidate,
    }
}

fn parse_concept_type(s: &str) -> ConceptType {
    match s {
        "architecture" => ConceptType::Architecture,
        "bug_fix" => ConceptType::BugFix,
        "troubleshooting" => ConceptType::Troubleshooting,
        "data_asset" => ConceptType::DataAsset,
        "task_state" => ConceptType::TaskState,
        "preference" => ConceptType::Preference,
        _ => ConceptType::Architecture,
    }
}
