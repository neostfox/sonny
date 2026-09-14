//! P14: SQLite-backed entity–property timeline store.

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;

use crate::error::MemoryResult;
use crate::models::timeline::{TimelineEntry, TimelineStatus};

use super::traits::TimelineStore;

pub struct SqliteTimelineStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteTimelineStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

const COLS: &str = "entry_id, workspace_id, entity, property, value, status, \
    observation_id, valid_from, valid_to, superseded_by, created_at";

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimelineEntry> {
    let status: String = row.get(5)?;
    Ok(TimelineEntry {
        entry_id: row.get(0)?,
        workspace_id: row.get(1)?,
        entity: row.get(2)?,
        property: row.get(3)?,
        value: row.get(4)?,
        status: status.parse().unwrap_or(TimelineStatus::Expired),
        observation_id: row.get(6)?,
        valid_from: row.get(7)?,
        valid_to: row.get(8)?,
        superseded_by: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn insert_entry(conn: &Connection, e: &TimelineEntry) -> MemoryResult<()> {
    conn.execute(
        &format!(
            "INSERT INTO entity_property_timeline ({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
        ),
        params![
            e.entry_id,
            e.workspace_id,
            e.entity,
            e.property,
            e.value,
            e.status.as_str(),
            e.observation_id,
            e.valid_from,
            e.valid_to,
            e.superseded_by,
            e.created_at,
        ],
    )?;
    Ok(())
}

impl TimelineStore for SqliteTimelineStore {
    fn get_active(
        &self,
        workspace_id: &str,
        entity: &str,
        property: &str,
    ) -> MemoryResult<Option<TimelineEntry>> {
        let conn = self.conn.lock();
        let sql = format!(
            "SELECT {COLS} FROM entity_property_timeline \
             WHERE workspace_id = ?1 AND entity = ?2 AND property = ?3 AND status = 'active'"
        );
        match conn.query_row(&sql, params![workspace_id, entity, property], row_to_entry) {
            Ok(e) => Ok(Some(e)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn history(
        &self,
        workspace_id: &str,
        entity: &str,
        property: &str,
    ) -> MemoryResult<Vec<TimelineEntry>> {
        let conn = self.conn.lock();
        let sql = format!(
            "SELECT {COLS} FROM entity_property_timeline \
             WHERE workspace_id = ?1 AND entity = ?2 AND property = ?3 \
             ORDER BY valid_from DESC, created_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![workspace_id, entity, property], row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn history_for_entity(
        &self,
        workspace_id: &str,
        entity: &str,
    ) -> MemoryResult<Vec<TimelineEntry>> {
        let conn = self.conn.lock();
        let sql = format!(
            "SELECT {COLS} FROM entity_property_timeline \
             WHERE workspace_id = ?1 AND entity = ?2 \
             ORDER BY property, valid_from DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![workspace_id, entity], row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Append a new active version; expire/supersede the previous active row.
    /// Returns the new entry.
    fn append_version(
        &self,
        workspace_id: &str,
        entity: &str,
        property: &str,
        value: Option<&str>,
        observation_id: Option<&str>,
        now: &str,
    ) -> MemoryResult<TimelineEntry> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        // Supersede any current active rows for this (entity, property).
        let sql = format!(
            "SELECT {COLS} FROM entity_property_timeline \
             WHERE workspace_id = ?1 AND entity = ?2 AND property = ?3 AND status = 'active'"
        );
        let prev: Vec<TimelineEntry> = {
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt
                .query_map(params![workspace_id, entity, property], row_to_entry)?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let mut entry = TimelineEntry::new(workspace_id, entity, property, value, observation_id, now);
        for old in &prev {
            tx.execute(
                "UPDATE entity_property_timeline \
                 SET status = ?1, valid_to = ?2, superseded_by = ?3 \
                 WHERE entry_id = ?4",
                params![
                    TimelineStatus::Superseded.as_str(),
                    now,
                    entry.entry_id,
                    old.entry_id
                ],
            )?;
        }
        insert_entry(&tx, &entry)?;
        tx.commit()?;
        Ok(entry)
    }

    /// Explicitly expire the current value (no replacement yet).
    fn expire_active(
        &self,
        workspace_id: &str,
        entity: &str,
        property: &str,
        now: &str,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE entity_property_timeline \
             SET status = 'expired', valid_to = ?1 \
             WHERE workspace_id = ?2 AND entity = ?3 AND property = ?4 AND status = 'active'",
            params![now, workspace_id, entity, property],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migration::run_migrations;

    fn store() -> SqliteTimelineStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        SqliteTimelineStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn append_then_history_has_one_active() {
        let s = store();
        let a = s
            .append_version("ws", "posmask", "has", Some("field_a"), Some("o1"), "t1")
            .unwrap();
        assert!(a.is_current());
        let b = s
            .append_version("ws", "posmask", "has", Some("field_b"), Some("o2"), "t2")
            .unwrap();
        assert_eq!(b.value.as_deref(), Some("field_b"));
        assert!(b.is_current());

        let hist = s.history("ws", "posmask", "has").unwrap();
        assert_eq!(hist.len(), 2);
        let actives = hist.iter().filter(|e| e.is_current()).count();
        assert_eq!(actives, 1);
        let old = s.get_active("ws", "posmask", "has").unwrap().unwrap();
        assert_eq!(old.entry_id, b.entry_id);
    }

    #[test]
    fn expire_clears_active() {
        let s = store();
        s.append_version("ws", "user", "prefers", Some("vim"), None, "t1")
            .unwrap();
        s.expire_active("ws", "user", "prefers", "t2").unwrap();
        assert!(s.get_active("ws", "user", "prefers").unwrap().is_none());
        let hist = s.history("ws", "user", "prefers").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].status, TimelineStatus::Expired);
    }
}
