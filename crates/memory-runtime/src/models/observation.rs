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
    /// Extraction-time confidence fixed by `source_type` provenance. Immutable after
    /// extraction; Validate/feedback never touch it.
    pub extraction_confidence: f64,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    pub status: ObservationStatus,
    pub surprise_score: f64,
    pub source_type: ObservationSourceType,
    pub consolidated: bool,
    /// Design §4.3: memory type is an Observation attribute, not a separate entity.
    pub memory_type_candidate: Option<MemoryType>,
    /// Type-specific structured detail for `memory_type_candidate` (JSON blob).
    pub observation_detail_json: Option<String>,
    pub created_at: String,
}

impl Observation {
    /// Beta-posterior confidence in the fact itself, updated by evidence over time.
    pub fn fact_confidence(&self) -> f64 {
        self.evidence_alpha / (self.evidence_alpha + self.evidence_beta)
    }

    /// Source-gated confidence used for recall ranking: an untrustworthy source cannot
    /// reach high confidence even if repeatedly uncorrected.
    pub fn effective_confidence(&self) -> f64 {
        self.extraction_confidence * self.fact_confidence()
    }
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
    /// Extraction-time confidence weight fixed by provenance.
    pub fn extraction_confidence(&self) -> f64 {
        match self {
            Self::FileEvidence => 0.9,
            Self::UserConfirm => 0.85,
            Self::UserNegation => 0.8,
            Self::UserMessage => 0.7,
            Self::AssistantGuess => 0.3,
        }
    }

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

impl std::str::FromStr for ObservationSourceType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_message" => Ok(Self::UserMessage),
            "user_confirm" => Ok(Self::UserConfirm),
            "user_negation" => Ok(Self::UserNegation),
            "assistant_guess" => Ok(Self::AssistantGuess),
            "file_evidence" => Ok(Self::FileEvidence),
            _ => Err(()),
        }
    }
}

/// Memory type taxonomy (design §4.3). Carried as an Observation attribute, not a
/// separate persisted entity. Type-specific detail lives in `observation_detail_json`.
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

impl std::str::FromStr for MemoryType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "architecture_memory" => Ok(Self::ArchitectureMemory),
            "bug_fix_memory" => Ok(Self::BugFixMemory),
            "troubleshooting_memory" => Ok(Self::TroubleshootingMemory),
            "data_asset_memory" => Ok(Self::DataAssetMemory),
            "user_preference_memory" => Ok(Self::UserPreferenceMemory),
            "rejected_hypothesis_memory" => Ok(Self::RejectedHypothesisMemory),
            "task_state_memory" => Ok(Self::TaskStateMemory),
            "decision_memory" => Ok(Self::DecisionMemory),
            "project_context_memory" => Ok(Self::ProjectContextMemory),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(extraction: f64, alpha: f64, beta: f64) -> Observation {
        Observation {
            observation_id: "o".into(),
            workspace_id: "ws".into(),
            memory_id: "m".into(),
            subject_text: "S".into(),
            subject_type: None,
            predicate: "p".into(),
            object_text: None,
            object_type: None,
            evidence_text: None,
            extraction_confidence: extraction,
            evidence_alpha: alpha,
            evidence_beta: beta,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: ObservationSourceType::UserMessage,
            consolidated: false,
            memory_type_candidate: None,
            observation_detail_json: None,
            created_at: "t".into(),
        }
    }

    #[test]
    fn extraction_weights_fixed_by_source_type() {
        assert_eq!(
            ObservationSourceType::FileEvidence.extraction_confidence(),
            0.9
        );
        assert_eq!(
            ObservationSourceType::UserConfirm.extraction_confidence(),
            0.85
        );
        assert_eq!(
            ObservationSourceType::UserNegation.extraction_confidence(),
            0.8
        );
        assert_eq!(
            ObservationSourceType::UserMessage.extraction_confidence(),
            0.7
        );
        assert_eq!(
            ObservationSourceType::AssistantGuess.extraction_confidence(),
            0.3
        );
    }

    #[test]
    fn fact_confidence_is_beta_posterior() {
        // alpha / (alpha + beta)
        assert!((obs(1.0, 3.0, 1.0).fact_confidence() - 0.75).abs() < 1e-9);
        assert!((obs(1.0, 1.0, 1.0).fact_confidence() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn effective_confidence_gates_untrustworthy_sources() {
        // A repeatedly-"confirmed" assistant guess cannot exceed 0.27 even at
        // fact_confidence 0.9, because the source ceiling is 0.3.
        let guess = obs(0.3, 9.0, 1.0);
        assert!((guess.fact_confidence() - 0.9).abs() < 1e-9);
        assert!((guess.effective_confidence() - 0.27).abs() < 1e-9);

        // A well-sourced fact rises with confirming evidence.
        let file_confirmed = obs(0.9, 9.0, 1.0);
        assert!((file_confirmed.effective_confidence() - 0.81).abs() < 1e-9);
        let file_fresh = obs(0.9, 1.0, 1.0);
        assert!((file_fresh.effective_confidence() - 0.45).abs() < 1e-9);
    }

    #[test]
    fn memory_type_round_trips() {
        assert_eq!(
            MemoryType::BugFixMemory.as_str(),
            "bug_fix_memory".parse::<MemoryType>().unwrap().as_str()
        );
        assert!("unknown_type".parse::<MemoryType>().is_err());
    }
}
