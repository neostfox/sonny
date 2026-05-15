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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    SharedEntity,
    SharedSession,
    EmbeddingSimilarity,
    Temporal,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SharedEntity => "shared_entity",
            Self::SharedSession => "shared_session",
            Self::EmbeddingSimilarity => "embedding_similarity",
            Self::Temporal => "temporal",
        }
    }
}
