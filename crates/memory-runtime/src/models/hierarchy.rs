use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyType {
    IsSubconceptOf,
    IsPartOf,
    IsInstanceOf,
}

impl HierarchyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IsSubconceptOf => "is_subconcept_of",
            Self::IsPartOf => "is_part_of",
            Self::IsInstanceOf => "is_instance_of",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    SharedEntity,
    SharedSession,
    EmbeddingSimilarity,
    Temporal,
    /// Directed cause→effect edge derived from `causes` observations (P5-A).
    Causal,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SharedEntity => "shared_entity",
            Self::SharedSession => "shared_session",
            Self::EmbeddingSimilarity => "embedding_similarity",
            Self::Temporal => "temporal",
            Self::Causal => "causal",
        }
    }

    /// Directed relations preserve (src, dst) order; symmetric relations are
    /// stored with canonical src < dst ordering so a pair has one row.
    pub fn is_directed(&self) -> bool {
        matches!(self, Self::Temporal | Self::Causal)
    }
}

impl std::str::FromStr for RelationType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "shared_entity" => Ok(Self::SharedEntity),
            "shared_session" => Ok(Self::SharedSession),
            "embedding_similarity" => Ok(Self::EmbeddingSimilarity),
            "temporal" => Ok(Self::Temporal),
            "causal" => Ok(Self::Causal),
            _ => Err(()),
        }
    }
}
