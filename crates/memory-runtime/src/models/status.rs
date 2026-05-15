use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Candidate,
    FastStored,
    Confirmed,
    AutoConfirmed,
    Rejected,
    Deprecated,
    Disputed,
    Orphan,
}

impl ObservationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::FastStored => "fast_stored",
            Self::Confirmed => "confirmed",
            Self::AutoConfirmed => "auto_confirmed",
            Self::Rejected => "rejected",
            Self::Deprecated => "deprecated",
            Self::Disputed => "disputed",
            Self::Orphan => "orphan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptStatus {
    Candidate,
    Active,
    Labile,
    Deprecated,
    Disputed,
}

impl ConceptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Active => "active",
            Self::Labile => "labile",
            Self::Deprecated => "deprecated",
            Self::Disputed => "disputed",
        }
    }
}
