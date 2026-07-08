use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("LLM provider error: {0}")]
    LlmProvider(String),

    #[error("LLM returned invalid JSON: {source}")]
    LlmInvalidJson {
        #[source]
        source: serde_json::Error,
        raw: String,
    },

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Clustering error: {0}")]
    Clustering(String),

    #[error("Concept not found: {concept_id}")]
    ConceptNotFound { concept_id: String },

    #[error("Observation not found: {observation_id}")]
    ObservationNotFound { observation_id: String },

    #[error("Invalid status transition: {from} -> {to} for {object_type} {object_id}")]
    InvalidStatusTransition {
        from: String,
        to: String,
        object_type: String,
        object_id: String,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Session parse error: {details}")]
    SessionParse { details: String },

    #[error("Recall produced no results for workspace {workspace}")]
    EmptyRecall { workspace: String },

    #[error("Self-loop relation rejected: concept {concept_id} cannot relate to itself")]
    SelfLoopRelation { concept_id: String },
}

pub type MemoryResult<T> = Result<T, MemoryError>;
