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
    /// Estimated token count for [`MemoryContext::to_prompt`]. Recall builders must keep
    /// this at or below the requested budget.
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
    /// Final ranking score: `(semantic·w_s + entity·w_e) · recency`.
    pub score: f64,
    pub semantic_score: f64,
    pub entity_score: f64,
    /// P4-C: Ebbinghaus recency factor in (0, 1], multiplicative on the channel
    /// score. Salience only — never persisted, never touches evidence α/β.
    pub recency: f64,
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

#[derive(Debug, Clone, Copy)]
pub struct RecallBudget {
    pub max_tokens: usize,
    pub user_preferences: usize,
    pub known_facts: usize,
    pub rejected_hypotheses: usize,
    pub task_state: usize,
    pub relevant_entities: usize,
}

impl RecallBudget {
    pub fn for_intent(intent: &Intent, max_tokens: usize) -> Self {
        let available = max_tokens.saturating_sub(250).max(max_tokens / 2);
        let (user_preferences, known_facts, rejected_hypotheses, task_state, relevant_entities) =
            match intent {
                Intent::ContinueInvestigation => (10, 30, 10, 35, 15),
                Intent::VerifyFact => (10, 45, 20, 10, 15),
                Intent::CorrectMistake => (10, 30, 35, 10, 15),
                Intent::AddKnowledge => (15, 30, 10, 25, 20),
                Intent::ReviewHistory => (15, 40, 15, 15, 15),
                Intent::GeneralQuery => (15, 40, 15, 15, 15),
            };

        Self {
            max_tokens,
            user_preferences: pct(available, user_preferences),
            known_facts: pct(available, known_facts),
            rejected_hypotheses: pct(available, rejected_hypotheses),
            task_state: pct(available, task_state),
            relevant_entities: pct(available, relevant_entities),
        }
    }
}

const fn pct(total: usize, percent: usize) -> usize {
    total.saturating_mul(percent) / 100
}
