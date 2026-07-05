//! SQLite-backed feedback ledger (P4-B).

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::LazyLock;

use rusqlite::{params, Connection};

use crate::error::MemoryResult;
use crate::models::feedback::{Feedback, FeedbackType};

use super::traits::FeedbackStore;

pub struct SqliteFeedbackStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteFeedbackStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const FEEDBACK_COLUMNS: &str = "\
    feedback_id, workspace_id, concept_id, observation_id, feedback_type, \
    feedback_text, alpha_delta, beta_delta, created_at";

static FEEDBACK_INSERT: LazyLock<String> = LazyLock::new(|| {
    format!("INSERT INTO feedback ({FEEDBACK_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
});
static FEEDBACK_LIST_BY_CONCEPT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {FEEDBACK_COLUMNS} FROM feedback \
         WHERE workspace_id = ?1 AND concept_id = ?2 ORDER BY created_at ASC, feedback_id ASC"
    )
});

fn row_to_feedback(row: &rusqlite::Row<'_>) -> rusqlite::Result<Feedback> {
    let feedback_type: String = row.get(4)?;
    // Strict parse: a type outside the vocabulary must surface, not coerce —
    // it would silently misreport which revision weights were applied.
    let feedback_type = feedback_type.parse::<FeedbackType>().map_err(|()| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("unknown feedback_type: {feedback_type}").into(),
        )
    })?;
    Ok(Feedback {
        feedback_id: row.get(0)?,
        workspace_id: row.get(1)?,
        concept_id: row.get(2)?,
        observation_id: row.get(3)?,
        feedback_type,
        feedback_text: row.get(5)?,
        alpha_delta: row.get(6)?,
        beta_delta: row.get(7)?,
        created_at: row.get(8)?,
    })
}

impl FeedbackStore for SqliteFeedbackStore {
    fn insert(&self, feedback: &Feedback) -> MemoryResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            &FEEDBACK_INSERT,
            params![
                feedback.feedback_id,
                feedback.workspace_id,
                feedback.concept_id,
                feedback.observation_id,
                feedback.feedback_type.as_str(),
                feedback.feedback_text,
                feedback.alpha_delta,
                feedback.beta_delta,
                feedback.created_at,
            ],
        )?;
        Ok(())
    }

    fn list_by_concept(
        &self,
        workspace_id: &str,
        concept_id: &str,
    ) -> MemoryResult<Vec<Feedback>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&FEEDBACK_LIST_BY_CONCEPT)?;
        let rows = stmt
            .query_map(params![workspace_id, concept_id], row_to_feedback)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::connection::Database;

    fn feedback(id: &str, concept: &str, ft: FeedbackType) -> Feedback {
        Feedback {
            feedback_id: id.to_string(),
            workspace_id: "ws".to_string(),
            concept_id: concept.to_string(),
            observation_id: None,
            feedback_type: ft,
            feedback_text: "text".to_string(),
            alpha_delta: 2.0,
            beta_delta: 0.0,
            created_at: "2026-07-04T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn insert_and_list_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteFeedbackStore::new(db.conn.clone());

        store
            .insert(&feedback("f1", "c1", FeedbackType::Confirm))
            .unwrap();
        store
            .insert(&feedback("f2", "c1", FeedbackType::Negate))
            .unwrap();
        store
            .insert(&feedback("f3", "c2", FeedbackType::Correct))
            .unwrap();

        let rows = store.list_by_concept("ws", "c1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].feedback_id, "f1");
        assert_eq!(rows[0].feedback_type, FeedbackType::Confirm);
        assert_eq!(rows[1].feedback_type, FeedbackType::Negate);
        assert!(store.list_by_concept("ws", "c3").unwrap().is_empty());
        assert!(store.list_by_concept("other", "c1").unwrap().is_empty());
    }

    #[test]
    fn duplicate_feedback_id_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteFeedbackStore::new(db.conn.clone());

        store
            .insert(&feedback("f1", "c1", FeedbackType::Confirm))
            .unwrap();
        assert!(store
            .insert(&feedback("f1", "c1", FeedbackType::Confirm))
            .is_err());
    }
}
