//! P14: Entity–Property–Time timeline.
//!
//! MindMemOS insight: successive records for the same `(entity, property)`
//! form a timeline. Updating a preference is not overwrite — it is a new
//! version with the previous value expired (still auditable).
//!
//! This is the *version-history* layer. Truthiness / trust stays on the
//! Observation Beta posterior; the timeline answers "what did we believe
//! about this property, and when?".

use serde::{Deserialize, Serialize};

/// Lifecycle of one timeline entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineStatus {
    /// Current value for (entity, property).
    Active,
    /// Older value kept for history (explicitly expired).
    Expired,
    /// Replaced by a newer version (`superseded_by` points at it).
    Superseded,
}

impl TimelineStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Superseded => "superseded",
        }
    }
}

impl std::str::FromStr for TimelineStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "expired" => Ok(Self::Expired),
            "superseded" => Ok(Self::Superseded),
            _ => Err(()),
        }
    }
}

/// One version of an entity property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub entry_id: String,
    pub workspace_id: String,
    /// Canonical entity key.
    pub entity: String,
    /// Property / predicate name (e.g. `has`, `uses_framework`).
    pub property: String,
    pub value: Option<String>,
    pub status: TimelineStatus,
    /// Observation that produced this version (provenance).
    pub observation_id: Option<String>,
    pub valid_from: String,
    /// Set when the entry stops being current.
    pub valid_to: Option<String>,
    pub superseded_by: Option<String>,
    pub created_at: String,
}

impl TimelineEntry {
    pub fn new(
        workspace_id: &str,
        entity: &str,
        property: &str,
        value: Option<&str>,
        observation_id: Option<&str>,
        now: &str,
    ) -> Self {
        Self {
            entry_id: format!("tl-{}", uuid::Uuid::new_v4()),
            workspace_id: workspace_id.to_string(),
            entity: entity.to_string(),
            property: property.to_string(),
            value: value.map(|s| s.to_string()),
            status: TimelineStatus::Active,
            observation_id: observation_id.map(|s| s.to_string()),
            valid_from: now.to_string(),
            valid_to: None,
            superseded_by: None,
            created_at: now.to_string(),
        }
    }

    pub fn is_current(&self) -> bool {
        self.status == TimelineStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entry_starts_active() {
        let e = TimelineEntry::new("ws", "posmask", "has", Some("field"), Some("obs1"), "t0");
        assert!(e.is_current());
        assert_eq!(e.status, TimelineStatus::Active);
        assert!(e.valid_to.is_none());
    }

    #[test]
    fn status_roundtrip() {
        for s in [
            TimelineStatus::Active,
            TimelineStatus::Expired,
            TimelineStatus::Superseded,
        ] {
            assert_eq!(s.as_str().parse::<TimelineStatus>().unwrap(), s);
        }
    }
}
