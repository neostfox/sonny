use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::MemoryResult;
use crate::models::raw_memory::{RawMemory, SourceType};

use super::traits::RawMemoryStore;

pub struct SqliteRawMemoryStore {
    conn: Mutex<Connection>,
}

impl SqliteRawMemoryStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }
}

impl RawMemoryStore for SqliteRawMemoryStore {
    fn insert(&self, raw: &RawMemory) -> MemoryResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO raw_memory (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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

    fn get_by_session(&self, session_id: &str) -> MemoryResult<Vec<RawMemory>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        _ => SourceType::Manual,
    }
}
