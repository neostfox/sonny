use rusqlite::Connection;

use crate::error::MemoryResult;

const CURRENT_VERSION: u32 = 1;

const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("../migrations/001_initial.sql"))];

pub fn run_migrations(conn: &Connection) -> MemoryResult<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    for &(version, sql) in MIGRATIONS {
        if version > current {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {sql}
                 PRAGMA user_version = {version};
                 COMMIT;"
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_database_gets_all_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }
}
