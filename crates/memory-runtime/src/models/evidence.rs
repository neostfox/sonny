use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source_type: String,
    pub source_id: String,
    pub session_id: String,
    pub message_range: Option<Vec<i64>>,
    pub evidence_text: String,
    pub created_at: String,
    pub confidence: f64,
    pub status: String,
}
