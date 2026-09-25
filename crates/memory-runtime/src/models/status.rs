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
    /// P2-D: replaced by a newer extraction from reextract(); kept for traceability.
    Superseded,
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
            Self::Superseded => "superseded",
        }
    }

    /// Planner-live set: rows that still carry writeable evidence.
    /// Matches `action_plan::plan_memory_action` (excludes superseded/rejected/deprecated).
    pub fn is_live(&self) -> bool {
        !matches!(
            self,
            Self::Superseded | Self::Rejected | Self::Deprecated
        )
    }
}

impl std::str::FromStr for ObservationStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "candidate" => Ok(Self::Candidate),
            "fast_stored" => Ok(Self::FastStored),
            "confirmed" => Ok(Self::Confirmed),
            "auto_confirmed" => Ok(Self::AutoConfirmed),
            "rejected" => Ok(Self::Rejected),
            "deprecated" => Ok(Self::Deprecated),
            "disputed" => Ok(Self::Disputed),
            "orphan" => Ok(Self::Orphan),
            "superseded" => Ok(Self::Superseded),
            _ => Err(()),
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

impl std::str::FromStr for ConceptStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "candidate" => Ok(Self::Candidate),
            "active" => Ok(Self::Active),
            "labile" => Ok(Self::Labile),
            "deprecated" => Ok(Self::Deprecated),
            "disputed" => Ok(Self::Disputed),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Candidate,
    Promoted,
    Rejected,
    Superseded,
}

impl CandidateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Promoted => "promoted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }
}

impl std::str::FromStr for CandidateStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "candidate" => Ok(Self::Candidate),
            "promoted" => Ok(Self::Promoted),
            "rejected" => Ok(Self::Rejected),
            "superseded" => Ok(Self::Superseded),
            _ => Err(()),
        }
    }
}
