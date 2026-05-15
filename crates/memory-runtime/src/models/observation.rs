use serde::{Deserialize, Serialize};

use super::status::ObservationStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub observation_id: String,
    pub workspace_id: String,
    pub memory_id: String,
    pub subject_text: String,
    pub subject_type: Option<String>,
    pub predicate: String,
    pub object_text: Option<String>,
    pub object_type: Option<String>,
    pub evidence_text: Option<String>,
    pub confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: ObservationStatus,
    pub surprise_score: f64,
    pub source_type: ObservationSourceType,
    pub consolidated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSourceType {
    UserMessage,
    UserConfirm,
    UserNegation,
    AssistantGuess,
    FileEvidence,
}

impl ObservationSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::UserConfirm => "user_confirm",
            Self::UserNegation => "user_negation",
            Self::AssistantGuess => "assistant_guess",
            Self::FileEvidence => "file_evidence",
        }
    }
}
