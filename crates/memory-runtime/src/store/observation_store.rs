use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::LazyLock;

use rusqlite::{params, Connection};

use crate::error::MemoryResult;
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::status::ObservationStatus;

use super::traits::ObservationStore;

pub struct SqliteObservationStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteObservationStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const OBS_COLUMNS: &str = "\
    observation_id, workspace_id, memory_id, subject_text, subject_type, \
    predicate, object_text, object_type, evidence_text, confidence, evidence_alpha, evidence_beta, \
    status, surprise_score, source_type, consolidated, created_at";

static OBS_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO observation ({OBS_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)")
});
static OBS_GET: LazyLock<String> =
    LazyLock::new(|| format!("SELECT {OBS_COLUMNS} FROM observation WHERE observation_id = ?1"));
static OBS_LIST: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY created_at DESC")
});
static OBS_FIND_BY_ENTITY: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND (subject_text = ?2 OR object_text = ?2) ORDER BY created_at DESC")
});

impl ObservationStore for SqliteObservationStore {
    fn insert(&self, obs: &Observation) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let status = obs.status.as_str();
        let source = obs.source_type.as_str();
        conn.execute(
            &OBS_INSERT,
            params![
                &obs.observation_id,
                &obs.workspace_id,
                &obs.memory_id,
                &obs.subject_text,
                &obs.subject_type,
                &obs.predicate,
                &obs.object_text,
                &obs.object_type,
                &obs.evidence_text,
                &obs.confidence,
                &obs.evidence_alpha,
                &obs.evidence_beta,
                &status,
                &obs.surprise_score,
                &source,
                &obs.consolidated,
                &obs.created_at,
            ],
        )?;
        Ok(())
    }
    fn insert_batch(&self, observations: &[Observation]) -> MemoryResult<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let sql: &str = &OBS_INSERT;
            for obs in observations {
                let status = obs.status.as_str();
                let source = obs.source_type.as_str();
                tx.execute(
                    sql,
                    params![
                        &obs.observation_id,
                        &obs.workspace_id,
                        &obs.memory_id,
                        &obs.subject_text,
                        &obs.subject_type,
                        &obs.predicate,
                        &obs.object_text,
                        &obs.object_type,
                        &obs.evidence_text,
                        &obs.confidence,
                        &obs.evidence_alpha,
                        &obs.evidence_beta,
                        &status,
                        &obs.surprise_score,
                        &source,
                        &obs.consolidated,
                        &obs.created_at,
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get(&self, observation_id: &str) -> MemoryResult<Option<Observation>> {
        let conn = self.conn.lock();
        let result = conn.query_row(&OBS_GET, [observation_id], row_to_observation);
        match result {
            Ok(obs) => Ok(Some(obs)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_by_workspace(
        &self,
        workspace_id: &str,
        status: Option<ObservationStatus>,
    ) -> MemoryResult<Vec<Observation>> {
        let conn = self.conn.lock();
        let s = status.map(|st| st.as_str());
        let result = collect_rows(&conn, &OBS_LIST, params![workspace_id, s])?;
        Ok(result)
    }
    fn update_status(&self, observation_id: &str, status: ObservationStatus) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let s = status.as_str();
        let changed = conn.execute(
            "UPDATE observation SET status = ?1 WHERE observation_id = ?2",
            params![s, observation_id],
        )?;
        if changed == 0 {
            return Err(crate::error::MemoryError::ObservationNotFound {
                observation_id: observation_id.to_string(),
            });
        }
        Ok(())
    }

    fn update_confidence(&self, observation_id: &str, alpha: f64, beta: f64) -> MemoryResult<()> {
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
        collect_rows(&conn, &OBS_FIND_BY_ENTITY, params![workspace_id, entity])
    }

    fn check_duplicate(
        &self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        workspace_id: &str,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock();
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

fn collect_rows(
    conn: &Connection,
    sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> MemoryResult<Vec<Observation>> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<Observation> = stmt
        .query_map(p, row_to_observation)?
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
    s.parse().unwrap_or_else(|_| {
        tracing::warn!("Unknown observation status '{s}', defaulting to candidate");
        ObservationStatus::Candidate
    })
}

fn parse_observation_source_type(s: &str) -> ObservationSourceType {
    s.parse().unwrap_or_else(|_| {
        tracing::warn!("Unknown observation source type '{s}', defaulting to assistant_guess");
        ObservationSourceType::AssistantGuess
    })
}
