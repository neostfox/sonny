use std::sync::Mutex;

use rusqlite::{Connection, params, params_from_iter};

use crate::error::MemoryResult;
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::status::ObservationStatus;

use super::traits::ObservationStore;

pub struct SqliteObservationStore {
    conn: Mutex<Connection>,
}

impl SqliteObservationStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }
}

const OBS_COLUMNS: &str = "\
    observation_id, workspace_id, memory_id, subject_text, subject_type, \
    predicate, object_text, object_type, evidence_text, confidence, evidence_alpha, evidence_beta, \
    status, surprise_score, source_type, consolidated, created_at";

fn obs_params(obs: &Observation) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(obs.observation_id.clone()),
        Box::new(obs.workspace_id.clone()),
        Box::new(obs.memory_id.clone()),
        Box::new(obs.subject_text.clone()),
        Box::new(obs.subject_type.clone()),
        Box::new(obs.predicate.clone()),
        Box::new(obs.object_text.clone()),
        Box::new(obs.object_type.clone()),
        Box::new(obs.evidence_text.clone()),
        Box::new(obs.confidence),
        Box::new(obs.evidence_alpha),
        Box::new(obs.evidence_beta),
        Box::new(obs.status.as_str().to_string()),
        Box::new(obs.surprise_score),
        Box::new(obs.source_type.as_str().to_string()),
        Box::new(obs.consolidated),
        Box::new(obs.created_at.clone()),
    ]
}

impl ObservationStore for SqliteObservationStore {
    fn insert(&self, obs: &Observation) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!("INSERT INTO observation ({OBS_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"),
            params_from_iter(obs_params(obs)),
        )?;
        Ok(())
    }

    fn insert_batch(&self, observations: &[Observation]) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        {
            let sql = format!("INSERT INTO observation ({OBS_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)");
            for obs in observations {
                tx.execute(&sql, params_from_iter(obs_params(obs)))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get(&self, observation_id: &str) -> MemoryResult<Option<Observation>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            &format!("SELECT {OBS_COLUMNS} FROM observation WHERE observation_id = ?1"),
            [observation_id],
            row_to_observation,
        );
        match result {
            Ok(obs) => Ok(Some(obs)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_by_workspace(&self, workspace_id: &str, status: Option<&str>) -> MemoryResult<Vec<Observation>> {
        let conn = self.conn.lock().unwrap();
        let result = if let Some(status) = status {
            let sql = format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND status = ?2 ORDER BY created_at DESC");
            collect_rows(&conn, &sql, params![workspace_id, status])?
        } else {
            let sql = format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 ORDER BY created_at DESC");
            collect_rows(&conn, &sql, params![workspace_id])?
        };
        Ok(result)
    }

    fn update_status(&self, observation_id: &str, status: &str) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE observation SET status = ?1 WHERE observation_id = ?2",
            params![status, observation_id],
        )?;
        if changed == 0 {
            return Err(crate::error::MemoryError::ObservationNotFound {
                observation_id: observation_id.to_string(),
            });
        }
        Ok(())
    }

    fn update_confidence(&self, observation_id: &str, alpha: f64, beta: f64) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        let confidence = alpha / (alpha + beta);
        let changed = conn.execute(
            "UPDATE observation SET evidence_alpha = ?1, evidence_beta = ?2, confidence = ?3 WHERE observation_id = ?4",
            params![alpha, beta, confidence, observation_id],
        )?;
        if changed == 0 {
            return Err(crate::error::MemoryError::ObservationNotFound {
                observation_id: observation_id.to_string(),
            });
        }
        Ok(())
    }

    fn find_by_entity(&self, entity: &str, workspace_id: &str) -> MemoryResult<Vec<Observation>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND (subject_text = ?2 OR object_text = ?2) ORDER BY created_at DESC"
        );
        Ok(collect_rows(&conn, &sql, params![workspace_id, entity])?)
    }

    fn check_duplicate(&self, subject: &str, predicate: &str, object: Option<&str>, workspace_id: &str) -> MemoryResult<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = if let Some(object) = object {
            conn.query_row(
                "SELECT COUNT(*) FROM observation WHERE workspace_id = ?1 AND subject_text = ?2 AND predicate = ?3 AND object_text = ?4",
                params![workspace_id, subject, predicate, object],
                |r| r.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM observation WHERE workspace_id = ?1 AND subject_text = ?2 AND predicate = ?3 AND object_text IS NULL",
                params![workspace_id, subject, predicate],
                |r| r.get(0),
            )?
        };
        Ok(count > 0)
    }
}

fn collect_rows(conn: &Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) -> MemoryResult<Vec<Observation>> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<Observation> = stmt.query_map(p, row_to_observation)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_to_observation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Observation> {
    Ok(Observation {
        observation_id: row.get(0)?,
        workspace_id: row.get(1)?,
        memory_id: row.get(2)?,
        subject_text: row.get(3)?,
        subject_type: row.get(4)?,
        predicate: row.get(5)?,
        object_text: row.get(6)?,
        object_type: row.get(7)?,
        evidence_text: row.get(8)?,
        confidence: row.get(9)?,
        evidence_alpha: row.get(10)?,
        evidence_beta: row.get(11)?,
        status: parse_observation_status(&row.get::<_, String>(12)?),
        surprise_score: row.get(13)?,
        source_type: parse_observation_source_type(&row.get::<_, String>(14)?),
        consolidated: row.get(15)?,
        created_at: row.get(16)?,
    })
}

fn parse_observation_status(s: &str) -> ObservationStatus {
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

fn parse_observation_source_type(s: &str) -> ObservationSourceType {
    match s {
        "user_message" => ObservationSourceType::UserMessage,
        "user_confirm" => ObservationSourceType::UserConfirm,
        "user_negation" => ObservationSourceType::UserNegation,
        "assistant_guess" => ObservationSourceType::AssistantGuess,
        "file_evidence" => ObservationSourceType::FileEvidence,
        _ => ObservationSourceType::AssistantGuess,
    }
}
