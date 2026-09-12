use serde::{Deserialize, Serialize};

use super::scope::LifecycleScope;
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
    /// P6-A: Project (private) → Domain → Global. Default Project.
    pub lifecycle_scope: LifecycleScope,
    /// Domain key when `lifecycle_scope == Domain` (e.g. "rust", "spring-boot").
    pub scope_key: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Concept {
    /// Distinct-session proxy used by promotion: recall diversity is not the
    /// same as contributing sessions, but it is the session-like counter we
    /// already track on Concept. Cluster/consolidate can raise it later.
    pub fn unique_session_count(&self) -> i64 {
        self.connection_count.max(self.recall_count).max(1)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(workspace_id: &str, concept_id: &str) -> Self {
        Self {
            concept_id: concept_id.to_string(),
            workspace_id: workspace_id.to_string(),
            name: "test".into(),
            concept_type: None,
            definition: None,
            related_entities_json: None,
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: 0.5,
            evidence_alpha: 1.0,
            evidence_beta: 1.0,
            status: ConceptStatus::Active,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            connection_count: 0,
            lifecycle_scope: LifecycleScope::Project,
            scope_key: None,
            created_at: "t0".into(),
            updated_at: "t0".into(),
        }
    }
}
