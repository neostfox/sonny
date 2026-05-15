use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub workspace: String,
    pub current_concept: Option<String>,
    pub intent: String,
    pub user_preferences: Vec<String>,
    pub known_facts: Vec<String>,
    pub rejected_hypotheses: Vec<String>,
    pub task_state: Vec<String>,
    pub relevant_entities: Vec<String>,
    pub token_count: usize,
}

impl MemoryContext {
    pub fn to_prompt(&self) -> String {
        let mut parts = Vec::new();
        parts.push("[Memory Context]".to_string());
        parts.push(format!("Workspace: {}", self.workspace));

        if let Some(ref concept) = self.current_concept {
            parts.push(format!("Current Concept: {}", concept));
        }
        parts.push(format!("Intent: {}", self.intent));

        if !self.user_preferences.is_empty() {
            parts.push("\nUser Preferences:".to_string());
            for p in &self.user_preferences {
                parts.push(format!("- {}", p));
            }
        }

        if !self.known_facts.is_empty() {
            parts.push("\nKnown Facts:".to_string());
            for f in &self.known_facts {
                parts.push(format!("- {}", f));
            }
        }

        if !self.rejected_hypotheses.is_empty() {
            parts.push("\nRejected / Doubtful Hypotheses:".to_string());
            for h in &self.rejected_hypotheses {
                parts.push(format!("- {}", h));
            }
        }

        if !self.task_state.is_empty() {
            parts.push("\nCurrent Task State:".to_string());
            for t in &self.task_state {
                parts.push(format!("- {}", t));
            }
        }

        if !self.relevant_entities.is_empty() {
            parts.push("\nRelevant Entities:".to_string());
            for e in &self.relevant_entities {
                parts.push(format!("- {}", e));
            }
        }

        parts.push("\nInstructions:".to_string());
        parts.push("- Do not treat candidate hypotheses as confirmed facts.".to_string());
        parts.push("- Respect rejected hypotheses and user preferences.".to_string());

        parts.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct RecallScore {
    pub concept_id: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    ContinueInvestigation,
    VerifyFact,
    CorrectMistake,
    AddKnowledge,
    ReviewHistory,
    GeneralQuery,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContinueInvestigation => "continue_investigation",
            Self::VerifyFact => "verify_fact",
            Self::CorrectMistake => "correct_mistake",
            Self::AddKnowledge => "add_knowledge",
            Self::ReviewHistory => "review_history",
            Self::GeneralQuery => "general_query",
        }
    }
}
