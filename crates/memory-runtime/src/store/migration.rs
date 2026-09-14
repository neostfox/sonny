use rusqlite::Connection;

use crate::error::MemoryResult;

const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../migrations/001_initial.sql")),
    (2, include_str!("../migrations/002_entity_concept.sql")),
    (3, include_str!("../migrations/003_p1_model_alignment.sql")),
    (
        4,
        include_str!("../migrations/004_p2c_observation_coclaim.sql"),
    ),
    (
        5,
        include_str!("../migrations/005_p2d_reverse_correction.sql"),
    ),
    (
        6,
        include_str!("../migrations/006_p3a_embedding_storage.sql"),
    ),
    (
        7,
        include_str!("../migrations/007_p5a_concept_relation.sql"),
    ),
    (8, include_str!("../migrations/008_p4b_feedback.sql")),
    (
        9,
        include_str!("../migrations/009_p4b_p5a_foreign_keys.sql"),
    ),
    (10, include_str!("../migrations/010_p6_layering.sql")),
    (11, include_str!("../migrations/011_p7_causal_fusion.sql")),
    (12, include_str!("../migrations/012_p14_entity_timeline.sql")),
];

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
        let latest = MIGRATIONS
            .last()
            .map(|(v, _)| *v)
            .expect("MIGRATIONS must contain at least one migration");
        assert_eq!(version, latest);
    }
}
