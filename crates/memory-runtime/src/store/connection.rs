use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;

use super::migration::run_migrations;
use crate::error::MemoryResult;

pub struct Database {
    pub conn: Arc<Mutex<Connection>>,
    pub has_vec: bool,
}

impl Database {
    pub fn open(db_path: &Path) -> MemoryResult<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let has_vec = false; // sqlite-vec loading deferred to Phase 2
        run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            has_vec,
        })
    }

    pub fn open_in_memory() -> MemoryResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            has_vec: false,
        })
    }
}
