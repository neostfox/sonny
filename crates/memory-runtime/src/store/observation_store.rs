use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::LazyLock;

use rusqlite::{params, Connection};

use crate::error::MemoryResult;
use crate::models::observation::{MemoryType, Observation, ObservationSourceType};
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
    predicate, object_text, object_type, evidence_text, extraction_confidence, evidence_alpha, evidence_beta, \
    status, surprise_score, source_type, consolidated, created_at, \
    memory_type_candidate, observation_detail_json, extraction_batch_id, superseded_by";

static OBS_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO observation ({OBS_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)")
});
static OBS_GET: LazyLock<String> =
    LazyLock::new(|| format!("SELECT {OBS_COLUMNS} FROM observation WHERE observation_id = ?1"));
static OBS_LIST: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY created_at DESC")
});
static OBS_FIND_BY_ENTITY: LazyLock<String> = LazyLock::new(|| {
    format!("SELECT {OBS_COLUMNS} FROM observation WHERE workspace_id = ?1 AND (subject_text = ?2 OR object_text = ?2) ORDER BY created_at DESC")
});
// P2-C: observations co-claimed with the given one (share an extraction batch edge).
// F2: exclude dead statuses so clustering (P3-B) never pulls superseded/rejected rows
// whose coclaim edges linger after re-extraction.
static OBS_FIND_COCLAIM: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {OBS_COLUMNS} FROM observation \
         WHERE status NOT IN ('superseded','rejected','deprecated') \
           AND observation_id IN (\
            SELECT CASE WHEN observation_a = ?1 THEN observation_b ELSE observation_a END \
            FROM observation_coclaim WHERE observation_a = ?1 OR observation_b = ?1\
        ) ORDER BY created_at ASC"
    )
});
// P3-D: dedup matches LIVE observations only — a superseded/rejected row is not a
// "duplicate" of a fresh extraction, and bumping a dead row's evidence would be wrong.
static OBS_FIND_DUP_OBJ: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {OBS_COLUMNS} FROM observation \
         WHERE workspace_id = ?1 AND subject_text = ?2 AND predicate = ?3 AND object_text = ?4 \
           AND status NOT IN ('superseded','rejected','deprecated') \
         ORDER BY created_at ASC LIMIT 1"
    )
});
static OBS_FIND_DUP_NULL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {OBS_COLUMNS} FROM observation \
         WHERE workspace_id = ?1 AND subject_text = ?2 AND predicate = ?3 AND object_text IS NULL \
           AND status NOT IN ('superseded','rejected','deprecated') \
         ORDER BY created_at ASC LIMIT 1"
    )
});

impl ObservationStore for SqliteObservationStore {
    fn insert(&self, obs: &Observation) -> MemoryResult<()> {
        let conn = self.conn.lock();
        insert_observation(&conn, obs)?;
        Ok(())
    }
    fn insert_batch(&self, observations: &[Observation]) -> MemoryResult<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            for obs in observations {
                insert_observation(&tx, obs)?;
            }
            // P2-C: record coclaim co-occurrence edges for observations sharing a batch.
            insert_coclaim_pairs(&tx, observations)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get(&self, observation_id: &str) -> MemoryResult<Option<Observation>> {
        let conn = self.conn.lock();
        get_observation_conn(&conn, observation_id)
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

    fn supersede(&self, observation_id: &str, superseded_by: &str) -> MemoryResult<()> {
        let conn = self.conn.lock();
        supersede_observation_conn(&conn, observation_id, superseded_by)
    }

    fn update_confidence(&self, observation_id: &str, alpha: f64, beta: f64) -> MemoryResult<()> {
        let conn = self.conn.lock();
        update_observation_confidence_conn(&conn, observation_id, alpha, beta)
    }

    fn find_by_entity(&self, entity: &str, workspace_id: &str) -> MemoryResult<Vec<Observation>> {
        let conn = self.conn.lock();
        collect_rows(&conn, &OBS_FIND_BY_ENTITY, params![workspace_id, entity])
    }

    fn find_duplicate(
        &self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        workspace_id: &str,
    ) -> MemoryResult<Option<Observation>> {
        let conn = self.conn.lock();
        let result = if let Some(object) = object {
            conn.query_row(
                &OBS_FIND_DUP_OBJ,
                params![workspace_id, subject, predicate, object],
                row_to_observation,
            )
        } else {
            conn.query_row(
                &OBS_FIND_DUP_NULL,
                params![workspace_id, subject, predicate],
                row_to_observation,
            )
        };
        match result {
            Ok(obs) => Ok(Some(obs)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn find_coclaim(&self, observation_id: &str) -> MemoryResult<Vec<Observation>> {
        let conn = self.conn.lock();
        collect_rows(&conn, &OBS_FIND_COCLAIM, params![observation_id])
    }

    /// P2-D (F1): atomically supersede a session's live observations and insert the
    /// replacement batch in ONE transaction. If any step fails, nothing is committed —
    /// the old observations stay live. `superseded_by` is the replacement batch id, or
    /// NULL when the new extraction is empty (F3). Returns the count superseded.
    fn replace_session_observations(
        &self,
        session_id: &str,
        new_observations: &[Observation],
    ) -> MemoryResult<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        // Replacement batch id (NULL when the new extraction yielded nothing).
        let new_batch: Option<String> = new_observations
            .first()
            .and_then(|o| o.extraction_batch_id.clone());
        // Supersede the old rows first; the new rows aren't inserted yet, so within
        // this transaction they can't be matched by the UPDATE.
        let superseded = tx.execute(
            "UPDATE observation SET status = 'superseded', superseded_by = ?1
             WHERE status != 'superseded'
               AND memory_id IN (SELECT memory_id FROM raw_memory WHERE session_id = ?2)",
            params![&new_batch, session_id],
        )?;
        for obs in new_observations {
            insert_observation(&tx, obs)?;
        }
        insert_coclaim_pairs(&tx, new_observations)?;
        tx.commit()?;
        Ok(superseded)
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

/// Read an observation by id on an already-held connection (feedback engine
/// reads inside its transaction scope).
pub(crate) fn get_observation_conn(
    conn: &Connection,
    observation_id: &str,
) -> MemoryResult<Option<Observation>> {
    match conn.query_row(&OBS_GET, [observation_id], row_to_observation) {
        Ok(obs) => Ok(Some(obs)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Update an observation's Beta evidence on an already-held connection.
pub(crate) fn update_observation_confidence_conn(
    conn: &Connection,
    observation_id: &str,
    alpha: f64,
    beta: f64,
) -> MemoryResult<()> {
    let changed = conn.execute(
        "UPDATE observation SET evidence_alpha = ?1, evidence_beta = ?2 WHERE observation_id = ?3",
        params![alpha, beta, observation_id],
    )?;
    require_observation_row(changed, observation_id)
}

/// Supersede an observation on an already-held connection.
pub(crate) fn supersede_observation_conn(
    conn: &Connection,
    observation_id: &str,
    superseded_by: &str,
) -> MemoryResult<()> {
    let changed = conn.execute(
        "UPDATE observation SET status = ?1, superseded_by = ?2 WHERE observation_id = ?3",
        params![
            ObservationStatus::Superseded.as_str(),
            superseded_by,
            observation_id
        ],
    )?;
    require_observation_row(changed, observation_id)
}

fn require_observation_row(changed: usize, observation_id: &str) -> MemoryResult<()> {
    if changed == 0 {
        return Err(crate::error::MemoryError::ObservationNotFound {
            observation_id: observation_id.to_string(),
        });
    }
    Ok(())
}

/// Insert a single observation row. Shared by `insert`, `insert_batch`,
/// `replace_session_observations`, and the feedback engine's Correct path, so
/// the 21-column param list lives in one place.
pub(crate) fn insert_observation(conn: &Connection, obs: &Observation) -> rusqlite::Result<()> {
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
            &obs.extraction_confidence,
            &obs.evidence_alpha,
            &obs.evidence_beta,
            &status,
            &obs.surprise_score,
            &source,
            &obs.consolidated,
            &obs.created_at,
            &obs.memory_type_candidate.as_ref().map(|t| t.as_str()),
            &obs.observation_detail_json,
            &obs.extraction_batch_id,
            &obs.superseded_by,
        ],
    )?;
    Ok(())
}

/// P2-C: write coclaim co-occurrence edges for observations sharing an extraction batch.
/// Pairs are stored canonically (`observation_a < observation_b`) so the PK is stable
/// regardless of input order; `INSERT OR IGNORE` keeps re-ingest idempotent.
fn insert_coclaim_pairs(conn: &Connection, observations: &[Observation]) -> rusqlite::Result<()> {
    // batch_id -> (workspace_id, observation_ids); only batches with >=2 members emit edges.
    let mut batches: std::collections::HashMap<&str, (&str, Vec<&str>)> =
        std::collections::HashMap::new();
    for obs in observations {
        if let Some(batch_id) = obs.extraction_batch_id.as_deref() {
            let entry = batches
                .entry(batch_id)
                .or_insert((&obs.workspace_id, Vec::new()));
            entry.1.push(&obs.observation_id);
        }
    }
    if batches.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO observation_coclaim \
         (observation_a, observation_b, batch_id, workspace_id, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (batch_id, (ws, ids)) in &batches {
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let (a, b) = if ids[i] <= ids[j] {
                    (ids[i], ids[j])
                } else {
                    (ids[j], ids[i])
                };
                stmt.execute(params![a, b, batch_id, ws, &now])?;
            }
        }
    }
    Ok(())
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
        extraction_confidence: row.get(9)?,
        evidence_alpha: row.get(10)?,
        evidence_beta: row.get(11)?,
        status: parse_observation_status(&row.get::<_, String>(12)?),
        surprise_score: row.get(13)?,
        source_type: parse_observation_source_type(&row.get::<_, String>(14)?),
        consolidated: row.get(15)?,
        created_at: row.get(16)?,
        memory_type_candidate: row
            .get::<_, Option<String>>(17)?
            .and_then(|s| parse_memory_type(&s)),
        observation_detail_json: row.get(18)?,
        extraction_batch_id: row.get(19)?,
        superseded_by: row.get(20)?,
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

fn parse_memory_type(s: &str) -> Option<MemoryType> {
    match s.parse::<MemoryType>() {
        Ok(t) => Some(t),
        Err(()) => {
            tracing::warn!("Unknown memory_type_candidate '{s}', defaulting to None");
            None
        }
    }
}
