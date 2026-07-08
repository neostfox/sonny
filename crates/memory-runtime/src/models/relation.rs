//! Concept-to-concept edges (P5-A).
//!
//! An edge is the unit of causal/associative knowledge between concepts. Each
//! edge carries its own Beta(alpha, beta) posterior — the same evidence
//! machinery observations use — so edge strength is accumulated evidence, not
//! a fixed score. Design (TODO.md 断点四): heterogeneous sources feed the SAME
//! edge and progressively upgrade its lifecycle:
//! observed association → candidate, repeated/cross-source → validated,
//! user/human confirmation → confirmed.

use serde::{Deserialize, Serialize};

use super::hierarchy::RelationType;
use crate::confidence::{BetaConfidence, EvidenceType};

/// Evidence-driven lifecycle of an edge (candidate → validated → confirmed).
/// Never downgraded automatically; contradicting evidence lowers `weight()`
/// instead, so a disputed edge stays visible with a low score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationLifecycle {
    Candidate,
    Validated,
    Confirmed,
}

impl RelationLifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Validated => "validated",
            Self::Confirmed => "confirmed",
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Candidate => 0,
            Self::Validated => 1,
            Self::Confirmed => 2,
        }
    }
}

impl std::str::FromStr for RelationLifecycle {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "candidate" => Ok(Self::Candidate),
            "validated" => Ok(Self::Validated),
            "confirmed" => Ok(Self::Confirmed),
            _ => Err(()),
        }
    }
}

/// Promotion thresholds: an edge becomes `validated` once its posterior mean
/// clears `VALIDATED_MIN_WEIGHT` over at least `VALIDATED_MIN_EVIDENCE`
/// accumulation events. `confirmed` requires an explicitly human-authoritative
/// evidence type, never volume alone.
pub const VALIDATED_MIN_WEIGHT: f64 = 0.7;
pub const VALIDATED_MIN_EVIDENCE: i64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptRelation {
    pub relation_id: String,
    pub workspace_id: String,
    /// For directed types (`causal`, `temporal`): the cause / earlier side.
    /// For symmetric types the pair is stored with src < dst (canonical row).
    pub src_concept_id: String,
    pub dst_concept_id: String,
    pub relation_type: RelationType,
    pub lifecycle: RelationLifecycle,
    pub evidence_alpha: f64,
    pub evidence_beta: f64,
    /// Number of evidence accumulation events (any sign), NOT alpha+beta.
    pub evidence_count: i64,
    pub last_evidence_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ConceptRelation {
    pub fn new(
        workspace_id: &str,
        src_concept_id: &str,
        dst_concept_id: &str,
        relation_type: RelationType,
        now: &str,
    ) -> Self {
        let (src, dst) = canonical_pair(src_concept_id, dst_concept_id, relation_type);
        Self {
            relation_id: format!("rel-{}", uuid::Uuid::new_v4()),
            workspace_id: workspace_id.to_string(),
            src_concept_id: src.to_string(),
            dst_concept_id: dst.to_string(),
            relation_type,
            lifecycle: RelationLifecycle::Candidate,
            evidence_alpha: 1.0,
            evidence_beta: 1.0,
            evidence_count: 0,
            last_evidence_at: None,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    }

    /// Posterior mean — the edge strength used by spreading activation.
    pub fn weight(&self) -> f64 {
        self.evidence_alpha / (self.evidence_alpha + self.evidence_beta)
    }

    /// Accumulate one piece of evidence and apply lifecycle promotion rules.
    pub fn add_evidence(&mut self, evidence: &EvidenceType, now: &str) {
        let mut posterior = BetaConfidence::with_values(self.evidence_alpha, self.evidence_beta);
        posterior.update(evidence);
        // Zero-weight evidence (e.g. AssistantSpeculation) shifts neither α nor
        // β; counting it would still push evidence_count toward the Validated
        // threshold without any real support behind the edge. Only weighted
        // evidence advances the counter and the recency clock.
        let contributed =
            posterior.alpha != self.evidence_alpha || posterior.beta != self.evidence_beta;
        self.evidence_alpha = posterior.alpha;
        self.evidence_beta = posterior.beta;
        if contributed {
            self.evidence_count += 1;
            self.last_evidence_at = Some(now.to_string());
        }
        self.updated_at = now.to_string();
        self.promote(evidence);
    }

    fn promote(&mut self, evidence: &EvidenceType) {
        let target = if matches!(
            evidence,
            EvidenceType::UserConfirmation | EvidenceType::HumanReviewConfirm
        ) {
            RelationLifecycle::Confirmed
        } else if self.weight() >= VALIDATED_MIN_WEIGHT
            && self.evidence_count >= VALIDATED_MIN_EVIDENCE
        {
            RelationLifecycle::Validated
        } else {
            return;
        };
        if target.rank() > self.lifecycle.rank() {
            self.lifecycle = target;
        }
    }
}

/// Canonical storage order for an edge's endpoints: directed types keep the
/// given order; symmetric types sort so one unordered pair is one row.
pub fn canonical_pair<'a>(
    a: &'a str,
    b: &'a str,
    relation_type: RelationType,
) -> (&'a str, &'a str) {
    if relation_type.is_directed() || a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(relation_type: RelationType) -> ConceptRelation {
        ConceptRelation::new("ws", "c-b", "c-a", relation_type, "t0")
    }

    #[test]
    fn symmetric_edges_canonicalize_endpoint_order() {
        let e = edge(RelationType::SharedEntity);
        assert_eq!((e.src_concept_id.as_str(), e.dst_concept_id.as_str()), ("c-a", "c-b"));
    }

    #[test]
    fn directed_edges_preserve_endpoint_order() {
        let e = edge(RelationType::Causal);
        assert_eq!((e.src_concept_id.as_str(), e.dst_concept_id.as_str()), ("c-b", "c-a"));
    }

    #[test]
    fn weight_is_beta_posterior_mean() {
        let mut e = edge(RelationType::Causal);
        assert!((e.weight() - 0.5).abs() < 1e-9);
        e.add_evidence(&EvidenceType::FileEvidence, "t1"); // alpha += 1.5
        assert!((e.weight() - 2.5 / 3.5).abs() < 1e-9);
        assert_eq!(e.evidence_count, 1);
        assert_eq!(e.last_evidence_at.as_deref(), Some("t1"));
    }

    #[test]
    fn volume_promotes_to_validated_but_never_confirmed() {
        let mut e = edge(RelationType::SharedEntity);
        e.add_evidence(&EvidenceType::RepeatedOccurrence, "t1");
        e.add_evidence(&EvidenceType::RepeatedOccurrence, "t2");
        assert_eq!(e.lifecycle, RelationLifecycle::Candidate); // count < 3
        e.add_evidence(&EvidenceType::RepeatedOccurrence, "t3");
        assert_eq!(e.lifecycle, RelationLifecycle::Validated); // 4/5 >= 0.7, count = 3
        for i in 0..20 {
            e.add_evidence(&EvidenceType::RepeatedOccurrence, &format!("t{}", 4 + i));
        }
        assert_eq!(e.lifecycle, RelationLifecycle::Validated);
    }

    #[test]
    fn zero_weight_evidence_does_not_advance_the_counter() {
        // AssistantSpeculation carries (0, 0): it must not inflate evidence_count
        // toward the Validated threshold nor move the recency clock.
        let mut e = edge(RelationType::SharedEntity);
        e.add_evidence(&EvidenceType::AssistantSpeculation, "t1");
        assert_eq!(e.evidence_count, 0);
        assert_eq!(e.last_evidence_at, None);
        assert!((e.weight() - 0.5).abs() < 1e-9);
        assert_eq!(e.lifecycle, RelationLifecycle::Candidate);

        // Three speculations still cannot reach Validated (which needs 3 real
        // pieces of evidence); one real occurrence is the first that counts.
        e.add_evidence(&EvidenceType::AssistantSpeculation, "t2");
        e.add_evidence(&EvidenceType::AssistantSpeculation, "t3");
        assert_eq!(e.evidence_count, 0);
        e.add_evidence(&EvidenceType::RepeatedOccurrence, "t4");
        assert_eq!(e.evidence_count, 1);
        assert_eq!(e.last_evidence_at.as_deref(), Some("t4"));
    }

    #[test]
    fn human_confirmation_promotes_to_confirmed_immediately() {
        let mut e = edge(RelationType::Causal);
        e.add_evidence(&EvidenceType::UserConfirmation, "t1");
        assert_eq!(e.lifecycle, RelationLifecycle::Confirmed);
    }

    #[test]
    fn contradicting_evidence_lowers_weight_without_demotion() {
        let mut e = edge(RelationType::Causal);
        e.add_evidence(&EvidenceType::UserConfirmation, "t1");
        e.add_evidence(&EvidenceType::UserNegation, "t2"); // beta += 3
        assert_eq!(e.lifecycle, RelationLifecycle::Confirmed);
        assert!(e.weight() < 0.5);
    }

    #[test]
    fn lifecycle_round_trips() {
        for lc in [
            RelationLifecycle::Candidate,
            RelationLifecycle::Validated,
            RelationLifecycle::Confirmed,
        ] {
            assert_eq!(lc.as_str().parse::<RelationLifecycle>(), Ok(lc));
        }
        assert!("bogus".parse::<RelationLifecycle>().is_err());
    }
}
