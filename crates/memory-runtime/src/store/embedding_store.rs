use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;

use crate::error::{MemoryError, MemoryResult};
use crate::models::embedding::{EmbeddingSearchResult, EmbeddingSourceType};

use super::traits::EmbeddingStore;

pub struct SqliteEmbeddingStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEmbeddingStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for &v in vector {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

fn decode_vector(bytes: &[u8]) -> MemoryResult<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(MemoryError::Embedding(format!(
            "embedding blob length {} is not divisible by 4",
            bytes.len()
        )));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn parse_source_type(raw: &str) -> MemoryResult<EmbeddingSourceType> {
    match raw {
        "observation" => Ok(EmbeddingSourceType::Observation),
        "concept" => Ok(EmbeddingSourceType::Concept),
        "concept_candidate" => Ok(EmbeddingSourceType::ConceptCandidate),
        other => Err(MemoryError::Embedding(format!(
            "unknown embedding source_type '{other}'"
        ))),
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let x = x as f64;
        let y = y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

impl EmbeddingStore for SqliteEmbeddingStore {
    fn store_embedding(
        &self,
        source_type: &str,
        source_id: &str,
        workspace_id: &str,
        text: &str,
        vector: &[f32],
    ) -> MemoryResult<()> {
        let conn = self.conn.lock();
        let blob = encode_vector(vector);
        conn.execute(
            "INSERT OR REPLACE INTO embedding \
             (source_type, source_id, workspace_id, text, vector, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                source_type,
                source_id,
                workspace_id,
                text,
                blob,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn search(
        &self,
        query_vector: &[f32],
        workspace_id: &str,
        top_k: usize,
        threshold: f32,
    ) -> MemoryResult<Vec<EmbeddingSearchResult>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT source_type, source_id, vector FROM embedding WHERE workspace_id = ?1",
        )?;
        let mut rows = stmt.query(params![workspace_id])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let source_type_raw: String = row.get(0)?;
            let source_id: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            let vector = decode_vector(&blob)?;
            let score = cosine(query_vector, &vector);
            if score >= threshold as f64 {
                results.push(EmbeddingSearchResult {
                    source_id,
                    source_type: parse_source_type(&source_type_raw)?,
                    score,
                });
            }
        }
        results.sort_by(|a, b| b.score.total_cmp(&a.score));
        results.truncate(top_k);
        Ok(results)
    }

    fn get_embedding(&self, source_type: &str, source_id: &str) -> MemoryResult<Option<Vec<f32>>> {
        let conn = self.conn.lock();
        let result = conn.query_row(
            "SELECT vector FROM embedding WHERE source_type = ?1 AND source_id = ?2",
            params![source_type, source_id],
            |row| row.get::<_, Vec<u8>>(0),
        );
        match result {
            Ok(blob) => Ok(Some(decode_vector(&blob)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, source_type: &str, source_id: &str) -> MemoryResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM embedding WHERE source_type = ?1 AND source_id = ?2",
            params![source_type, source_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migration::run_migrations;

    fn store() -> SqliteEmbeddingStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        SqliteEmbeddingStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn store_and_get_roundtrip() {
        let store = store();
        let vector = vec![1.0, 2.0, 3.0];
        store
            .store_embedding("observation", "obs1", "ws", "hello", &vector)
            .unwrap();
        assert_eq!(
            store.get_embedding("observation", "obs1").unwrap().unwrap(),
            vector
        );
    }

    #[test]
    fn search_returns_nearest() {
        let store = store();
        store
            .store_embedding("observation", "x", "ws", "x", &[1.0, 0.0, 0.0])
            .unwrap();
        store
            .store_embedding("concept", "y", "ws", "y", &[0.0, 1.0, 0.0])
            .unwrap();
        let hits = store.search(&[0.9, 0.1, 0.0], "ws", 2, 0.0).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source_id, "x");
        assert_eq!(hits[0].source_type, EmbeddingSourceType::Observation);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn delete_removes_embedding() {
        let store = store();
        store
            .store_embedding("observation", "obs1", "ws", "hello", &[1.0])
            .unwrap();
        store.delete("observation", "obs1").unwrap();
        assert!(store
            .get_embedding("observation", "obs1")
            .unwrap()
            .is_none());
    }
}
