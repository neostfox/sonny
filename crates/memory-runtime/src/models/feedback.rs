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

impl FeedbackType {
    /// Revision weights `(alpha_delta, beta_delta)` applied to the target
    /// concept's Beta evidence (quality-control.md §Revision Actions).
    pub fn revision_weights(&self) -> (f64, f64) {
        match self {
            Self::Confirm => (2.0, 0.0),
            Self::Negate => (0.0, 3.0),
            Self::Supplement => (0.5, 0.0),
            Self::Correct => (0.0, 2.0),
            Self::Preference => (1.0, 0.0),
            Self::General => (0.0, 0.0),
        }
    }

    /// P4-B closed loop: how this feedback resolves the pending recall.
    /// `Some(true)` counts a successful recall, `Some(false)` a failed one,
    /// `None` (General) leaves the recall outcome undetermined.
    pub fn recall_outcome(&self) -> Option<bool> {
        match self {
            Self::Confirm | Self::Supplement | Self::Preference => Some(true),
            Self::Negate | Self::Correct => Some(false),
            Self::General => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirm => "confirm",
            Self::Negate => "negate",
            Self::Supplement => "supplement",
            Self::Correct => "correct",
            Self::Preference => "preference",
            Self::General => "general",
        }
    }
}

impl std::str::FromStr for FeedbackType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "confirm" => Ok(Self::Confirm),
            "negate" => Ok(Self::Negate),
            "supplement" => Ok(Self::Supplement),
            "correct" => Ok(Self::Correct),
            "preference" => Ok(Self::Preference),
            "general" => Ok(Self::General),
            _ => Err(()),
        }
    }
}

/// One immutable feedback event as persisted in the `feedback` table (P4-B).
/// `alpha_delta`/`beta_delta` denormalize the applied revision weights so the
/// concept's confidence trajectory is auditable without replaying classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub feedback_id: String,
    pub workspace_id: String,
    pub concept_id: String,
    /// Set when the feedback targets a specific observation (Correct/Negate).
    pub observation_id: Option<String>,
    pub feedback_type: FeedbackType,
    pub feedback_text: String,
    pub alpha_delta: f64,
    pub beta_delta: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackResult {
    pub feedback_type: FeedbackType,
    pub concept_id: String,
    pub new_confidence: f64,
    pub status_changed: bool,
    pub new_status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_weights_match_quality_control_spec() {
        assert_eq!(FeedbackType::Confirm.revision_weights(), (2.0, 0.0));
        assert_eq!(FeedbackType::Negate.revision_weights(), (0.0, 3.0));
        assert_eq!(FeedbackType::Supplement.revision_weights(), (0.5, 0.0));
        assert_eq!(FeedbackType::Correct.revision_weights(), (0.0, 2.0));
        assert_eq!(FeedbackType::Preference.revision_weights(), (1.0, 0.0));
        assert_eq!(FeedbackType::General.revision_weights(), (0.0, 0.0));
    }

    #[test]
    fn recall_outcome_maps_positive_and_negative_feedback() {
        assert_eq!(FeedbackType::Confirm.recall_outcome(), Some(true));
        assert_eq!(FeedbackType::Supplement.recall_outcome(), Some(true));
        assert_eq!(FeedbackType::Preference.recall_outcome(), Some(true));
        assert_eq!(FeedbackType::Negate.recall_outcome(), Some(false));
        assert_eq!(FeedbackType::Correct.recall_outcome(), Some(false));
        assert_eq!(FeedbackType::General.recall_outcome(), None);
    }

    #[test]
    fn feedback_type_round_trips() {
        for ft in [
            FeedbackType::Confirm,
            FeedbackType::Negate,
            FeedbackType::Supplement,
            FeedbackType::Correct,
            FeedbackType::Preference,
            FeedbackType::General,
        ] {
            assert_eq!(ft.as_str().parse::<FeedbackType>().unwrap(), ft);
        }
        assert!("unknown".parse::<FeedbackType>().is_err());
    }
}
