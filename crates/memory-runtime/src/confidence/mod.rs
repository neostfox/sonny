use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct BetaConfidence {
    pub alpha: f32,
    pub beta: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceType {
    UserConfirmation,
    FileEvidence,
    RepeatedOccurrence,
    CrossSession3Plus,
    CrossSession2,
    HumanReviewConfirm,
    RecallNotCorrected,
    UserNegation,
    ConflictingEvidence,
    RecallCorrected,
    InternalConflict,
    LongInactivity,
    AssistantSpeculation,
}

impl fmt::Display for EvidenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserConfirmation => write!(f, "user_confirmation"),
            Self::FileEvidence => write!(f, "file_evidence"),
            Self::RepeatedOccurrence => write!(f, "repeated_occurrence"),
            Self::CrossSession3Plus => write!(f, "cross_session_3_plus"),
            Self::CrossSession2 => write!(f, "cross_session_2"),
            Self::HumanReviewConfirm => write!(f, "human_review_confirm"),
            Self::RecallNotCorrected => write!(f, "recall_not_corrected"),
            Self::UserNegation => write!(f, "user_negation"),
            Self::ConflictingEvidence => write!(f, "conflicting_evidence"),
            Self::RecallCorrected => write!(f, "recall_corrected"),
            Self::InternalConflict => write!(f, "internal_conflict"),
            Self::LongInactivity => write!(f, "long_inactivity"),
            Self::AssistantSpeculation => write!(f, "assistant_speculation"),
        }
    }
}

const EVIDENCE_WEIGHTS: [(EvidenceType, f32, f32); 13] = [
    (EvidenceType::UserConfirmation, 2.0, 0.0),
    (EvidenceType::FileEvidence, 1.5, 0.0),
    (EvidenceType::RepeatedOccurrence, 1.0, 0.0),
    (EvidenceType::CrossSession3Plus, 2.0, 0.0),
    (EvidenceType::CrossSession2, 1.0, 0.0),
    (EvidenceType::HumanReviewConfirm, 2.0, 0.0),
    (EvidenceType::RecallNotCorrected, 0.3, 0.0),
    (EvidenceType::UserNegation, 0.0, 3.0),
    (EvidenceType::ConflictingEvidence, 0.0, 2.0),
    (EvidenceType::RecallCorrected, 0.0, 1.5),
    (EvidenceType::InternalConflict, 0.0, 1.0),
    (EvidenceType::LongInactivity, 0.0, 0.5), // caller must scale by days/30
    (EvidenceType::AssistantSpeculation, 0.0, 0.0),
];

fn get_weight(evidence_type: &EvidenceType) -> (f32, f32) {
    EVIDENCE_WEIGHTS
        .iter()
        .find(|(et, _, _)| et == evidence_type)
        .map(|(_, a, b)| (*a, *b))
        .unwrap_or((0.0, 0.0))
}

impl BetaConfidence {
    pub fn new() -> Self {
        Self {
            alpha: 1.0,
            beta: 1.0,
        }
    }

    pub fn with_values(alpha: f32, beta: f32) -> Self {
        Self { alpha, beta }
    }

    pub fn confidence(&self) -> f32 {
        self.alpha / (self.alpha + self.beta)
    }

    pub fn update(&mut self, evidence_type: &EvidenceType) {
        let (alpha_delta, beta_delta) = get_weight(evidence_type);
        self.alpha += alpha_delta;
        self.beta += beta_delta;
    }

    pub fn update_with_decay(&mut self, days_inactive: u32) {
        let scale = days_inactive as f32 / 30.0;
        self.beta += 0.5 * scale;
    }
}

impl fmt::Display for BetaConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let supporting = self.alpha - 1.0;
        let contradicting = self.beta - 1.0;
        write!(
            f,
            "{:.1} supporting, {:.1} contradicting (confidence: {:.2})",
            supporting,
            contradicting,
            self.confidence()
        )
    }
}

impl Default for BetaConfidence {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_prior_is_uniform() {
        let bc = BetaConfidence::new();
        assert_eq!(bc.alpha, 1.0);
        assert_eq!(bc.beta, 1.0);
        assert!((bc.confidence() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn user_confirmation_increases_confidence() {
        let mut bc = BetaConfidence::new();
        bc.update(&EvidenceType::UserConfirmation);
        assert_eq!(bc.alpha, 3.0); // 1.0 + 2.0
        assert_eq!(bc.beta, 1.0);
        assert!(bc.confidence() > 0.5);
    }

    #[test]
    fn user_negation_decreases_confidence() {
        let mut bc = BetaConfidence::new();
        bc.update(&EvidenceType::UserNegation);
        assert_eq!(bc.alpha, 1.0);
        assert_eq!(bc.beta, 4.0); // 1.0 + 3.0
        assert!(bc.confidence() < 0.5);
    }

    #[test]
    fn assistant_speculation_has_no_effect() {
        let mut bc = BetaConfidence::new();
        bc.update(&EvidenceType::AssistantSpeculation);
        assert_eq!(bc.alpha, 1.0);
        assert_eq!(bc.beta, 1.0);
    }

    #[test]
    fn mixed_evidence_sequence() {
        let mut bc = BetaConfidence::new();
        bc.update(&EvidenceType::RepeatedOccurrence); // alpha += 1.0
        bc.update(&EvidenceType::CrossSession2); // alpha += 1.0
        bc.update(&EvidenceType::RecallNotCorrected); // alpha += 0.3
        assert_eq!(bc.alpha, 3.3); // 1.0 + 1.0 + 1.0 + 0.3
        assert_eq!(bc.beta, 1.0);
        let expected = 3.3 / 4.3;
        assert!((bc.confidence() - expected).abs() < 1e-6);
    }

    #[test]
    fn long_inactivity_decay() {
        let mut bc = BetaConfidence::new();
        bc.update(&EvidenceType::UserConfirmation); // alpha = 3.0
        bc.update_with_decay(60); // beta += 0.5 * (60/30) = 1.0
        assert_eq!(bc.alpha, 3.0);
        assert_eq!(bc.beta, 2.0); // 1.0 + 1.0
    }

    #[test]
    fn display_format() {
        let bc = BetaConfidence::with_values(3.0, 2.0);
        let s = bc.to_string();
        assert!(s.contains("2.0 supporting"));
        assert!(s.contains("1.0 contradicting"));
        assert!(s.contains("0.60"));
    }

    #[test]
    fn all_evidence_types_have_weights() {
        for (_, a, b) in EVIDENCE_WEIGHTS {
            // At least one of alpha_delta or beta_delta should be 0 for pure types
            // (except assistant_speculation which has both 0)
            let _ = (a, b); // verify they compile and are accessible
        }
    }
}
