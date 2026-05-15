use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRef {
    pub id: i64,
    pub source_type: EmbeddingSourceType,
    pub source_id: String,
    pub workspace_id: Option<String>,
    pub text_content: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingSourceType {
    Observation,
    Concept,
    ConceptCandidate,
}

impl EmbeddingSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Concept => "concept",
            Self::ConceptCandidate => "concept_candidate",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingSearchResult {
    pub source_id: String,
    pub source_type: EmbeddingSourceType,
    pub score: f64,
}
