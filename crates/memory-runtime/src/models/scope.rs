//! P6: Concept lifecycle scope and cross-workspace promotion.
//!
//! Project-scoped knowledge is private. Once independent workspaces
//! corroborate the same causal pattern often enough, the concept can be
//! promoted to Domain (shared within a domain key) or Global (visible
//! everywhere). Promotion is evidence-driven, never volume-only.

use serde::{Deserialize, Serialize};

use crate::models::concept::Concept;

/// Visibility ladder for a concept. Higher scopes are strictly wider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleScope {
    Project,
    Domain,
    Global,
}

impl LifecycleScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Domain => "domain",
            Self::Global => "global",
        }
    }

    pub fn rank(&self) -> u8 {
        match self {
            Self::Project => 0,
            Self::Domain => 1,
            Self::Global => 2,
        }
    }
}

impl std::str::FromStr for LifecycleScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "project" => Ok(Self::Project),
            "domain" => Ok(Self::Domain),
            "global" => Ok(Self::Global),
            _ => Err(()),
        }
    }
}

/// Default promotion thresholds (TODO.md P6-C).
#[derive(Debug, Clone, Copy)]
pub struct PromotionThresholds {
    /// Distinct workspaces that independently observed the pattern.
    pub min_cross_project_count_domain: i64,
    pub min_cross_project_count_global: i64,
    /// Beta posterior confidence on the concept.
    pub min_confidence_domain: f64,
    pub min_confidence_global: f64,
    /// Distinct contributing sessions.
    pub min_unique_sessions_domain: i64,
    pub min_unique_sessions_global: i64,
}

impl Default for PromotionThresholds {
    fn default() -> Self {
        Self {
            min_cross_project_count_domain: 2,
            min_cross_project_count_global: 3,
            min_confidence_domain: 0.75,
            min_confidence_global: 0.85,
            min_unique_sessions_domain: 2,
            min_unique_sessions_global: 3,
        }
    }
}

/// Evidence snapshot used to decide whether a concept may leave `Project`.
#[derive(Debug, Clone, Copy)]
pub struct PromotionEvidence {
    pub cross_project_count: i64,
    pub conflict_count: i64,
    pub unique_session_count: i64,
    pub confidence: f64,
}

impl PromotionEvidence {
    pub fn from_concept(concept: &Concept, cross_project_count: i64, conflict_count: i64) -> Self {
        Self {
            cross_project_count,
            conflict_count,
            unique_session_count: concept.unique_session_count(),
            confidence: concept.confidence,
        }
    }
}

/// Returns the highest scope this evidence supports, if any.
/// Conflicts always block promotion. Structure-consistency is approximated by
/// requiring confidence and multi-session support (full graph check is P8).
pub fn evaluate_promotion(
    evidence: &PromotionEvidence,
    thresholds: &PromotionThresholds,
) -> Option<LifecycleScope> {
    if evidence.conflict_count > 0 {
        return None;
    }

    let global_ok = evidence.cross_project_count >= thresholds.min_cross_project_count_global
        && evidence.confidence >= thresholds.min_confidence_global
        && evidence.unique_session_count >= thresholds.min_unique_sessions_global;
    if global_ok {
        return Some(LifecycleScope::Global);
    }

    let domain_ok = evidence.cross_project_count >= thresholds.min_cross_project_count_domain
        && evidence.confidence >= thresholds.min_confidence_domain
        && evidence.unique_session_count >= thresholds.min_unique_sessions_domain;
    if domain_ok {
        return Some(LifecycleScope::Domain);
    }

    None
}

/// Whether a concept with `scope` is visible to a query in `workspace_id`
/// under the given visibility flags.
pub fn is_visible(
    concept: &Concept,
    workspace_id: &str,
    include_domain_keys: &[String],
    include_global: bool,
) -> bool {
    if concept.workspace_id == workspace_id {
        return true;
    }
    match concept.lifecycle_scope {
        LifecycleScope::Project => false,
        LifecycleScope::Domain => match &concept.scope_key {
            Some(key) => include_domain_keys.iter().any(|k| k == key),
            None => false,
        },
        LifecycleScope::Global => include_global,
    }
}

/// P6-C: apply a promotion decision to a concept (never demotes).
pub fn apply_promotion(concept: &mut Concept, target: LifecycleScope, scope_key: Option<String>) -> bool {
    if target.rank() <= concept.lifecycle_scope.rank() {
        return false;
    }
    concept.lifecycle_scope = target;
    concept.scope_key = match target {
        LifecycleScope::Domain => scope_key,
        LifecycleScope::Global => None,
        LifecycleScope::Project => None,
    };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(cross: i64, sessions: i64, confidence: f64, conflicts: i64) -> PromotionEvidence {
        PromotionEvidence {
            cross_project_count: cross,
            conflict_count: conflicts,
            unique_session_count: sessions,
            confidence,
        }
    }

    #[test]
    fn single_workspace_never_promotes() {
        let t = PromotionThresholds::default();
        assert_eq!(evaluate_promotion(&evidence(1, 5, 0.95, 0), &t), None);
    }

    #[test]
    fn two_workspaces_can_reach_domain() {
        let t = PromotionThresholds::default();
        assert_eq!(
            evaluate_promotion(&evidence(2, 2, 0.8, 0), &t),
            Some(LifecycleScope::Domain)
        );
    }

    #[test]
    fn three_workspaces_high_confidence_reach_global() {
        let t = PromotionThresholds::default();
        assert_eq!(
            evaluate_promotion(&evidence(3, 3, 0.9, 0), &t),
            Some(LifecycleScope::Global)
        );
    }

    #[test]
    fn conflicts_block_promotion() {
        let t = PromotionThresholds::default();
        assert_eq!(evaluate_promotion(&evidence(5, 5, 0.99, 1), &t), None);
    }

    #[test]
    fn visibility_ladder() {
        let mut c = Concept::new_for_test("ws-a", "c1");
        assert!(is_visible(&c, "ws-a", &[], true));
        assert!(!is_visible(&c, "ws-b", &[], true));

        c.lifecycle_scope = LifecycleScope::Global;
        assert!(is_visible(&c, "ws-b", &[], true));
        assert!(!is_visible(&c, "ws-b", &[], false));

        c.lifecycle_scope = LifecycleScope::Domain;
        c.scope_key = Some("rust".into());
        assert!(is_visible(&c, "ws-b", &["rust".into()], true));
        assert!(!is_visible(&c, "ws-b", &["python".into()], true));
    }

    #[test]
    fn promotion_never_downgrades() {
        let mut c = Concept::new_for_test("ws", "c");
        c.lifecycle_scope = LifecycleScope::Global;
        assert!(!apply_promotion(&mut c, LifecycleScope::Domain, None));
        assert_eq!(c.lifecycle_scope, LifecycleScope::Global);
    }
}
