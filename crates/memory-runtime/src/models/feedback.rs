use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    Confirm,
    Negate,
    Supplement,
    Correct,
    Preference,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackResult {
    pub feedback_type: FeedbackType,
    pub concept_id: String,
    pub new_confidence: f64,
    pub status_changed: bool,
    pub new_status: Option<String>,
}
