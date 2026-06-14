//! Canonical predicate vocabulary for observations (design §4.2).
//!
//! Predicates are normalized to this closed set at extraction so dedup, clustering,
//! and conflict detection operate on consistent relation labels. The LLM extraction
//! prompt instructs the model to emit canonical values; [`Predicate::from_str`]
//! additionally maps common synonyms so legacy/variable output still canonicalizes.
//!
//! Observations whose predicate cannot be mapped are stored verbatim (lowercased,
//! spaces→underscores) rather than dropped — an unknown relation is not a
//! hallucination, and the underlying fact is still evidence-backed. Two such
//! unknowns spelled identically still dedup.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical relation between an observation's subject and object.
///
/// Memory *typing* (preference / task state / architecture / …) is handled by
/// [`crate::models::observation::MemoryType`], not by predicates — predicates
/// express relations between entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Predicate {
    /// A contains / has field or part B (包含).
    Has,
    /// A does NOT have B (不包含). Negative information is high-value — preserve it.
    NotHas,
    /// A depends on / requires B (依赖).
    DependsOn,
    /// A does NOT depend on B.
    NotDependsOn,
    /// A is associated with / may relate to B (可能关联). Generic association bucket.
    RelatedTo,
    /// A is NOT related to B.
    NotRelatedTo,
    /// A is the root cause of / leads to B (根因/导致).
    Causes,
}

impl Predicate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Has => "has",
            Self::NotHas => "not_has",
            Self::DependsOn => "depends_on",
            Self::NotDependsOn => "not_depends_on",
            Self::RelatedTo => "related_to",
            Self::NotRelatedTo => "not_related_to",
            Self::Causes => "causes",
        }
    }

    /// All canonical storage strings, in declaration order. Used by the extraction
    /// prompt builder and tests.
    pub fn all_canonical() -> &'static [&'static str] {
        &[
            "has",
            "not_has",
            "depends_on",
            "not_depends_on",
            "related_to",
            "not_related_to",
            "causes",
        ]
    }

    /// True if `self` and `other` are opposing relations on the same entity pair
    /// (e.g. [`Has`](Self::Has) vs [`NotHas`](Self::NotHas)). Used by conflict
    /// detection (design §3.3): two observations sharing subject+object with a
    /// negation pair conflict.
    pub fn is_negation_of(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Has, Self::NotHas)
                | (Self::NotHas, Self::Has)
                | (Self::DependsOn, Self::NotDependsOn)
                | (Self::NotDependsOn, Self::DependsOn)
                | (Self::RelatedTo, Self::NotRelatedTo)
                | (Self::NotRelatedTo, Self::RelatedTo)
        )
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Predicate {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Normalize: trim, lowercase, spaces→underscores. Makes matching robust to
        // the LLM emitting "not has" vs "not_has" vs "Not_Has".
        let key = s.trim().to_lowercase().replace(' ', "_");
        match key.as_str() {
            // has / containment
            "has" | "have" | "has_field" | "contains" | "contain" | "includes" | "include"
            | "owns" | "possesses" => Ok(Self::Has),
            // negation of has
            "not_has" | "not_have" | "not_has_field" | "lacks" | "lacks_field" | "missing"
            | "does_not_have" | "doesnt_have" | "does_not_contain" | "doesnt_contain"
            | "excludes" | "without" => Ok(Self::NotHas),
            // dependency
            "depends_on" | "depend_on" | "depends" | "depends_upon" | "requires" | "uses"
            | "dependency" => Ok(Self::DependsOn),
            // negation of dependency
            "not_depends_on" | "not_depend_on" | "does_not_depend_on" | "doesnt_depend_on"
            | "not_depends" => Ok(Self::NotDependsOn),
            // association
            "related_to"
            | "relate_to"
            | "related"
            | "possibly_related"
            | "possibly_related_to"
            | "may_relate_to"
            | "associated_with"
            | "associates_with"
            | "links_to"
            | "linked_to"
            | "references" => Ok(Self::RelatedTo),
            // negation of association
            "not_related_to"
            | "not_relate_to"
            | "unrelated"
            | "not_associated"
            | "not_associated_with" => Ok(Self::NotRelatedTo),
            // causation (active voice only — passive "caused_by" flips direction)
            "causes" | "cause" | "is_root_cause" | "root_cause" | "root_cause_of" | "leads_to"
            | "lead_to" | "triggers" | "results_in" => Ok(Self::Causes),
            _ => Err(()),
        }
    }
}

/// Normalize a raw predicate (from LLM output or legacy data) to its canonical
/// storage form. Canonical predicates map to [`Predicate::as_str`]; unmappable
/// predicates are returned lowercased with spaces→underscores so at least
/// case/whitespace variation collapses (identical unknowns still dedup).
///
/// Unknown predicates are NOT dropped — the observation is still evidence-backed.
pub fn normalize_predicate(raw: &str) -> String {
    match raw.parse::<Predicate>() {
        Ok(p) => p.as_str().to_string(),
        Err(_) => raw.trim().to_lowercase().replace(' ', "_"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_round_trips() {
        for &canon in Predicate::all_canonical() {
            let parsed: Predicate = canon.parse().unwrap();
            assert_eq!(parsed.as_str(), canon);
        }
    }

    #[test]
    fn synonyms_canonicalize() {
        let cases = [
            ("has_field", Predicate::Has),
            ("Contains", Predicate::Has),
            ("not_has_field", Predicate::NotHas),
            ("LACKS", Predicate::NotHas),
            ("does not have", Predicate::NotHas),
            ("depends_on", Predicate::DependsOn),
            ("requires", Predicate::DependsOn),
            ("possibly_related", Predicate::RelatedTo),
            ("is_root_cause", Predicate::Causes),
            ("leads_to", Predicate::Causes),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                raw.parse::<Predicate>().map(|p| p.as_str()),
                Ok(expected.as_str()),
                "failed for raw={raw:?}"
            );
        }
    }

    #[test]
    fn unknown_predicate_is_err() {
        assert!("defenestrates".parse::<Predicate>().is_err());
        assert!("".parse::<Predicate>().is_err());
    }

    #[test]
    fn normalize_canonical_and_unknown() {
        // Canonical synonyms collapse to the canonical string.
        assert_eq!(normalize_predicate("has_field"), "has");
        assert_eq!(normalize_predicate("Not_Has_Field"), "not_has");
        assert_eq!(normalize_predicate("depends on"), "depends_on");
        // Unknown predicates fall through, lowercased + snake_cased, NOT dropped.
        assert_eq!(normalize_predicate("Defenestrates"), "defenestrates");
        assert_eq!(normalize_predicate("Some Relation"), "some_relation");
    }

    #[test]
    fn negation_pairs() {
        assert!(Predicate::Has.is_negation_of(&Predicate::NotHas));
        assert!(Predicate::NotHas.is_negation_of(&Predicate::Has));
        assert!(Predicate::DependsOn.is_negation_of(&Predicate::NotDependsOn));
        assert!(Predicate::RelatedTo.is_negation_of(&Predicate::NotRelatedTo));
        // Non-negation pairs are not conflicts.
        assert!(!Predicate::Has.is_negation_of(&Predicate::DependsOn));
        assert!(!Predicate::Has.is_negation_of(&Predicate::Causes));
        assert!(!Predicate::Has.is_negation_of(&Predicate::Has));
    }
}
