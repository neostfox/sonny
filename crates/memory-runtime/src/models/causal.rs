//! P7-A/B/C: causal roles, do-statistics, and heterogeneous evidence weights.

use serde::{Deserialize, Serialize};

use crate::confidence::{BetaConfidence, EvidenceType};
use crate::models::observation::{Observation, ObservationSourceType};

/// How an observation contributes to causal inference (TODO.md P7-A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalRole {
    /// Deliberate intervention — strongest signal for P(effect|do(cause)).
    Intervention,
    /// Observed outcome side of a causal claim.
    Outcome,
    /// Pure co-occurrence / association, no intervention.
    ObservedAssociation,
    /// Known confound — should NOT strengthen the causal edge.
    Confound,
}

impl CausalRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Intervention => "intervention",
            Self::Outcome => "outcome",
            Self::ObservedAssociation => "observed_association",
            Self::Confound => "confound",
        }
    }

    /// Whether this role should accumulate supporting evidence on a causal edge.
    pub fn supports_causal_edge(&self) -> bool {
        matches!(self, Self::Intervention | Self::Outcome | Self::ObservedAssociation)
    }

    /// Relative strength when mapping onto the edge posterior.
    /// Intervention ≫ association (TODO.md principle 4).
    pub fn causal_strength(&self) -> f64 {
        match self {
            Self::Intervention => 2.0,
            Self::Outcome => 1.25,
            Self::ObservedAssociation => 1.0,
            Self::Confound => 0.0,
        }
    }
}

impl std::str::FromStr for CausalRole {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "intervention" => Ok(Self::Intervention),
            "outcome" => Ok(Self::Outcome),
            "observed_association" => Ok(Self::ObservedAssociation),
            "confound" => Ok(Self::Confound),
            _ => Err(()),
        }
    }
}

/// P7-B: sufficient statistics carried on a directed causal edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CausalStats {
    /// Approx. P(effect | do(cause)) — only Intervention evidence moves this.
    pub p_do: Option<f64>,
    /// Approx. P(effect | cause) — any supporting co-occurrence.
    pub p_given: Option<f64>,
    /// Approx. P(effect | ¬cause) — contrast / negative cases.
    pub p_not_given: Option<f64>,
}

impl Default for CausalStats {
    fn default() -> Self {
        Self {
            p_do: None,
            p_given: None,
            p_not_given: None,
        }
    }
}

impl CausalStats {
    /// Laplace-smoothed update of one conditional estimate.
    fn bump(p: &mut Option<f64>, success: bool, strength: f64) {
        // Beta(1,1) prior in probability space, exponentially weighted.
        let (alpha, beta) = match p {
            Some(p) => ((*p).max(1e-6) * 10.0, (1.0 - *p).max(1e-6) * 10.0),
            None => (1.0, 1.0),
        };
        let (da, db) = if success {
            (strength, 0.0)
        } else {
            (0.0, strength)
        };
        let a = alpha + da;
        let b = beta + db;
        *p = Some(a / (a + b));
    }

    /// Fold one observation into the edge's sufficient statistics.
    pub fn absorb_observation(&mut self, obs: &Observation) {
        let Some(role) = parse_causal_role(obs.causal_role.as_deref()) else {
            return;
        };
        if role == CausalRole::Confound {
            return;
        }
        let success = obs.evidence_alpha >= obs.evidence_beta;
        let strength = role.causal_strength();
        Self::bump(&mut self.p_given, success, strength);
        if role == CausalRole::Intervention {
            Self::bump(&mut self.p_do, success, strength);
        }
    }

    /// Fold a negative/contrast observation into P(effect|¬cause).
    pub fn absorb_contrast(&mut self, strength: f64) {
        Self::bump(&mut self.p_not_given, true, strength);
    }

    /// Intervention lift: how much do(cause) exceeds observational given-cause.
    /// Positive ⇒ interventions confirm the association is causal.
    pub fn intervention_lift(&self) -> Option<f64> {
        match (self.p_do, self.p_given) {
            (Some(d), Some(g)) => Some(d - g),
            _ => None,
        }
    }
}

fn parse_causal_role(raw: Option<&str>) -> Option<CausalRole> {
    raw?.parse().ok()
}

/// P7-C: evidence weight = type × source trust × reuse (cross-project count).
/// Returns (alpha_delta, beta_delta) to apply to a Beta posterior.
pub fn heterogeneous_evidence_weight(
    evidence: &EvidenceType,
    source: Option<ObservationSourceType>,
    cross_project_count: i64,
) -> (f64, f64) {
    let (a, b) = base_weight(evidence);
    let source_factor = match source {
        Some(ObservationSourceType::FileEvidence) => 1.25,
        Some(ObservationSourceType::UserConfirm) => 1.15,
        Some(ObservationSourceType::UserNegation) => 1.15,
        Some(ObservationSourceType::UserMessage) => 1.0,
        Some(ObservationSourceType::AssistantGuess) => 0.5,
        None => 1.0,
    };
    // Reuse: each additional independent workspace beyond the first adds 25%,
    // capped at 2.0× so a hot triple cannot dominate the graph.
    let reuse = ((cross_project_count.max(1) - 1) as f64 * 0.25 + 1.0).min(2.0);
    (a * source_factor * reuse, b * source_factor * reuse)
}

fn base_weight(evidence: &EvidenceType) -> (f64, f64) {
    match evidence {
        EvidenceType::UserConfirmation | EvidenceType::HumanReviewConfirm => (2.0, 0.0),
        EvidenceType::FileEvidence => (1.5, 0.0),
        EvidenceType::RepeatedOccurrence => (1.0, 0.0),
        EvidenceType::CrossSession3Plus => (2.0, 0.0),
        EvidenceType::CrossSession2 => (1.0, 0.0),
        EvidenceType::RecallNotCorrected => (0.3, 0.0),
        EvidenceType::UserNegation => (0.0, 3.0),
        EvidenceType::ConflictingEvidence => (0.0, 2.0),
        EvidenceType::RecallCorrected => (0.0, 1.5),
        EvidenceType::InternalConflict => (0.0, 1.0),
        EvidenceType::AssistantSpeculation => (0.0, 0.0),
    }
}

/// Apply heterogeneous weights to a Beta posterior (P7-C entry point).
pub fn update_with_heterogeneous_evidence(
    posterior: &mut BetaConfidence,
    evidence: &EvidenceType,
    source: Option<ObservationSourceType>,
    cross_project_count: i64,
) {
    let (da, db) = heterogeneous_evidence_weight(evidence, source, cross_project_count);
    posterior.alpha += da;
    posterior.beta += db;
}

/// Map a causal-role-bearing observation onto an EvidenceType for the edge.
pub fn causal_edge_evidence(obs: &Observation) -> Option<(EvidenceType, f64)> {
    let role = parse_causal_role(obs.causal_role.as_deref())?;
    if !role.supports_causal_edge() {
        return None;
    }
    let et = match (role, obs.source_type) {
        (CausalRole::Intervention, ObservationSourceType::FileEvidence) => {
            EvidenceType::HumanReviewConfirm
        }
        (CausalRole::Intervention, _) => EvidenceType::UserConfirmation,
        (_, ObservationSourceType::UserNegation) => EvidenceType::UserNegation,
        (_, ObservationSourceType::FileEvidence) => EvidenceType::FileEvidence,
        _ => EvidenceType::RepeatedOccurrence,
    };
    Some((et, role.causal_strength()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs_with(role: CausalRole, alpha: f64, beta: f64) -> Observation {
        Observation {
            observation_id: "o".into(),
            workspace_id: "ws".into(),
            memory_id: "m".into(),
            subject_text: "A".into(),
            subject_type: None,
            predicate: "causes".into(),
            object_text: Some("B".into()),
            object_type: None,
            evidence_text: None,
            extraction_confidence: 0.9,
            evidence_alpha: alpha,
            evidence_beta: beta,
            status: crate::models::status::ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: ObservationSourceType::FileEvidence,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: None,
            superseded_by: None,
            cross_project_count: 1,
            causal_role: Some(role.as_str().to_string()),
            consolidated: false,
            created_at: "t".into(),
        }
    }

    #[test]
    fn intervention_moves_p_do_association_does_not() {
        let mut stats = CausalStats::default();
        stats.absorb_observation(&obs_with(CausalRole::Intervention, 4.0, 1.0));
        assert!(stats.p_do.is_some());
        assert!(stats.p_given.is_some());

        let mut assoc_only = CausalStats::default();
        assoc_only.absorb_observation(&obs_with(CausalRole::ObservedAssociation, 4.0, 1.0));
        assert!(assoc_only.p_do.is_none());
        assert!(assoc_only.p_given.is_some());
    }

    #[test]
    fn confound_does_not_support_edge() {
        let mut stats = CausalStats::default();
        stats.absorb_observation(&obs_with(CausalRole::Confound, 5.0, 0.0));
        assert!(stats.p_do.is_none() && stats.p_given.is_none());
        assert!(causal_edge_evidence(&obs_with(CausalRole::Confound, 1.0, 1.0)).is_none());
    }

    #[test]
    fn reuse_increases_weight_capped() {
        let single = heterogeneous_evidence_weight(&EvidenceType::RepeatedOccurrence, None, 1);
        let multi = heterogeneous_evidence_weight(&EvidenceType::RepeatedOccurrence, None, 5);
        let huge = heterogeneous_evidence_weight(&EvidenceType::RepeatedOccurrence, None, 100);
        assert!(multi.0 > single.0);
        assert!(huge.0 <= single.0 * 2.0 + 1e-9);
    }

    #[test]
    fn assistant_guess_downweights() {
        let file = heterogeneous_evidence_weight(
            &EvidenceType::FileEvidence,
            Some(ObservationSourceType::FileEvidence),
            1,
        );
        let guess = heterogeneous_evidence_weight(
            &EvidenceType::FileEvidence,
            Some(ObservationSourceType::AssistantGuess),
            1,
        );
        assert!(guess.0 < file.0);
    }

    #[test]
    fn intervention_file_evidence_maps_to_human_confirm() {
        let o = obs_with(CausalRole::Intervention, 1.0, 1.0);
        let (et, s) = causal_edge_evidence(&o).unwrap();
        assert_eq!(et, EvidenceType::HumanReviewConfirm);
        assert!(s >= 2.0);
    }
}
