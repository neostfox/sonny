use serde::{Deserialize, Serialize};

use super::status::ObservationStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub memory_item_id: String,
    pub workspace_id: String,
    pub memory_type: MemoryType,
    pub title: Option<String>,
    pub content: String,
    pub entities_json: Option<String>,
    pub relations_json: Option<String>,
    pub evidence_json: Option<String>,
    pub confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: ObservationStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    ArchitectureMemory,
    BugFixMemory,
    TroubleshootingMemory,
    DataAssetMemory,
    UserPreferenceMemory,
    RejectedHypothesisMemory,
    TaskStateMemory,
    DecisionMemory,
    ProjectContextMemory,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ArchitectureMemory => "architecture_memory",
            Self::BugFixMemory => "bug_fix_memory",
            Self::TroubleshootingMemory => "troubleshooting_memory",
            Self::DataAssetMemory => "data_asset_memory",
            Self::UserPreferenceMemory => "user_preference_memory",
            Self::RejectedHypothesisMemory => "rejected_hypothesis_memory",
            Self::TaskStateMemory => "task_state_memory",
            Self::DecisionMemory => "decision_memory",
            Self::ProjectContextMemory => "project_context_memory",
        }
    }
}
