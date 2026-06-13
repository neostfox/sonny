use parking_lot::Mutex;
use std::sync::Arc;

use rusqlite::Connection;

use crate::error::MemoryResult;
use crate::models::raw_memory::{RawMemory, SourceType};

use super::traits::RawMemoryStore;

pub struct SqliteRawMemoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRawMemoryStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const RAW_INSERT_SQL: &str = "INSERT OR IGNORE INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

impl RawMemoryStore for SqliteRawMemoryStore {
    fn insert(&self, raw: &RawMemory) -> MemoryResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            RAW_INSERT_SQL,
            (
                &raw.memory_id,
                &raw.workspace_id,
                &raw.session_id,
                &raw.role,
                &raw.content,
                raw.source_type.as_str(),
                &raw.source_ref,
                &raw.created_at,
            ),
        )?;
        Ok(())
    }

    fn insert_batch(&self, raws: &[RawMemory]) -> MemoryResult<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        for raw in raws {
            tx.execute(
                RAW_INSERT_SQL,
                (
                    &raw.memory_id,
                    &raw.workspace_id,
                    &raw.session_id,
                    &raw.role,
                    &raw.content,
                    raw.source_type.as_str(),
                    &raw.source_ref,
                    &raw.created_at,
                ),
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    fn get_by_session(&self, session_id: &str) -> MemoryResult<Vec<RawMemory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at
             FROM raw_memory WHERE session_id = ?1 ORDER BY created_at"
        )?;
        let rows = stmt.query_map([session_id], |row| {
            Ok(RawMemory {
                memory_id: row.get(0)?,
                workspace_id: row.get(1)?,
                session_id: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                source_type: parse_source_type(&row.get::<_, String>(5)?),
                source_ref: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn list_by_workspace(&self, workspace_id: &str, limit: usize) -> MemoryResult<Vec<RawMemory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at
             FROM raw_memory WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map((workspace_id, limit as i64), |row| {
            Ok(RawMemory {
                memory_id: row.get(0)?,
                workspace_id: row.get(1)?,
                session_id: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                source_type: parse_source_type(&row.get::<_, String>(5)?),
                source_ref: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn parse_source_type(s: &str) -> SourceType {
    match s {
        "session_file" => SourceType::SessionFile,
        "trellis_journal" => SourceType::TrellisJournal,
        "trellis_task" => SourceType::TrellisTask,
        "user_input" => SourceType::UserInput,
        "manual" => SourceType::Manual,
        _ => {
            tracing::warn!("Unknown source type '{s}', defaulting to manual");
            SourceType::Manual
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::connection::Database;

    fn make_raw(id: &str, session: &str, secs: u32) -> RawMemory {
        RawMemory {
            memory_id: id.into(),
            workspace_id: "ws".into(),
            session_id: session.into(),
            role: "user".into(),
            content: format!("content {id}"),
            source_type: SourceType::SessionFile,
            source_ref: "test.json".into(),
            created_at: format!("2026-06-13T00:00:{secs:02}Z"),
        }
    }

    #[test]
    fn insert_batch_is_atomic_and_queryable() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteRawMemoryStore::new(db.conn.clone());

        let raws = vec![
            make_raw("m1", "s1", 1),
            make_raw("m2", "s1", 2),
            make_raw("m3", "s1", 3),
        ];
        store.insert_batch(&raws).unwrap();

        // All three land under the same session, ordered by created_at.
        let got = store.get_by_session("s1").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].memory_id, "m1");
        assert_eq!(got[2].memory_id, "m3");

        // INSERT OR IGNORE keeps the batch idempotent on duplicate memory_id.
        store.insert_batch(&raws).unwrap();
        assert_eq!(store.get_by_session("s1").unwrap().len(), 3);
    }
}
