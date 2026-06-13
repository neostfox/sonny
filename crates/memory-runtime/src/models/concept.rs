use serde::{Deserialize, Serialize};

use super::status::{CandidateStatus, ConceptStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptCandidate {
    pub candidate_id: String,
    pub workspace_id: String,
    pub name: String,
    pub summary: Option<String>,
    pub source_terms_json: Option<String>,
    pub source_sessions_json: Option<String>,
    pub source_observations_json: Option<String>,
    pub known_facts_json: Option<String>,
    pub rejected_hypotheses_json: Option<String>,
    pub open_questions_json: Option<String>,
    pub evidence_json: Option<String>,
    pub evidence_count: i64,
    pub confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: CandidateStatus,
    pub last_recalled_at: Option<String>,
    pub recall_count: i64,
    pub successful_recall_count: i64,
    pub failed_recall_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptType {
    Architecture,
    BugFix,
    Troubleshooting,
    DataAsset,
    TaskState,
    Preference,
}

impl ConceptType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::BugFix => "bug_fix",
            Self::Troubleshooting => "troubleshooting",
            Self::DataAsset => "data_asset",
            Self::TaskState => "task_state",
            Self::Preference => "preference",
        }
    }
}
impl std::str::FromStr for ConceptType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "architecture" => Ok(Self::Architecture),
            "bug_fix" => Ok(Self::BugFix),
            "troubleshooting" => Ok(Self::Troubleshooting),
            "data_asset" => Ok(Self::DataAsset),
            "task_state" => Ok(Self::TaskState),
            "preference" => Ok(Self::Preference),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    pub concept_id: String,
    pub workspace_id: String,
    pub name: String,
    pub concept_type: Option<ConceptType>,
    pub definition: Option<String>,
    pub related_entities_json: Option<String>,
    pub known_facts_json: Option<String>,
    pub rejected_hypotheses_json: Option<String>,
    pub open_questions_json: Option<String>,
    pub evidence_json: Option<String>,
    pub confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: ConceptStatus,
    pub parent_concept_id: Option<String>,
    pub hierarchy_depth: i64,
    pub last_recalled_at: Option<String>,
    pub recall_count: i64,
    pub successful_recall_count: i64,
    pub failed_recall_count: i64,
    pub connection_count: i64,
    pub created_at: String,
    pub updated_at: String,
}
