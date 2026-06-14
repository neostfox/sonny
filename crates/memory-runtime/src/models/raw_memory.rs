use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMemory {
    pub memory_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub source_type: SourceType,
    pub source_ref: String,
    /// P2-D: prompt version used by the last extraction over this memory's session.
    /// `None` until `reextract`/extract stamps it; enables detecting stale extractions.
    pub extraction_version: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    SessionFile,
    Journal,
    UserInput,
    Manual,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionFile => "session_file",
            Self::Journal => "journal",
            Self::UserInput => "user_input",
            Self::Manual => "manual",
        }
    }
}
impl std::str::FromStr for SourceType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "session_file" => Ok(Self::SessionFile),
            "journal" => Ok(Self::Journal),
            "user_input" => Ok(Self::UserInput),
            "manual" => Ok(Self::Manual),
            _ => Err(()),
        }
    }
}
