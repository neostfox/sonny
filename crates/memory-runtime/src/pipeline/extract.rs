use serde::Deserialize;

use crate::error::{MemoryError, MemoryResult};
use crate::llm::traits::LlmProvider;
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::raw_memory::RawMemory;
use crate::models::status::ObservationStatus;

const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are an observation extraction system. Your task is to extract structured knowledge observations from conversation messages.

## Input
- Conversation messages between a user and a coding assistant
- Each message has a role: "user" or "assistant"
- Messages may contain Chinese and English mixed content

## Extraction Rules
1. Extract facts, preferences, technical decisions, and negative information
2. Each observation MUST have a clear evidence_text (verbatim quote from the input)
3. Distinguish user-stated facts from assistant guesses
4. Preserve negative information ("X does NOT have Y") as separate observations
5. Do NOT extract greetings, pleasantries, or meta-discussion
6. Do NOT speculate beyond what the evidence explicitly supports

## Output Schema
Output a JSON object with this structure:
{
  "observations": [
    {
      "subject_text": "entity name",
      "subject_type": "entity type or null",
      "predicate": "relationship (English)",
      "object_text": "target entity or value",
      "object_type": "target type or null",
      "evidence_text": "exact verbatim quote",
      "source_type": "user_message | user_confirm | user_negation | assistant_guess | file_evidence",
      "confidence": 0.0-1.0
    }
  ]
}

## Source Type Rules
- user_message: The user stated this directly
- user_confirm: The user confirmed something the assistant suggested
- user_negation: The user denied or corrected something
- assistant_guess: The assistant inferred or speculated this
- file_evidence: Derived from file/project content

## Language Handling
- Input may contain mixed Chinese and English text
- Preserve the original language in subject_text, object_text, and evidence_text
- Do NOT translate any text
- Predicates must always be in English
- Entity types must always be in English

## Output ONLY valid JSON. No markdown fences, no explanation.

## Examples

Input:
[user]: POSMASK 表没有机器字段

Output:
{"observations":[{"subject_text":"POSMASK","subject_type":"database_table","predicate":"not_has_field","object_text":"机器字段","object_type":"field","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message","confidence":0.9}]}

Input:
[user]: 好的，谢谢
[assistant]: 不客气。

Output:
{"observations":[]}
"#;

#[derive(Debug, Clone, Deserialize)]
struct ExtractionResult {
    observations: Vec<RawObservation>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawObservation {
    subject_text: String,
    #[serde(default)]
    subject_type: Option<String>,
    predicate: String,
    #[serde(default)]
    object_text: Option<String>,
    #[serde(default)]
    object_type: Option<String>,
    #[serde(default)]
    evidence_text: Option<String>,
    source_type: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
}

fn default_confidence() -> f64 {
    0.5
}

pub async fn extract_observations(
    raw_memories: &[RawMemory],
    llm: &(impl LlmProvider + Sync),
) -> MemoryResult<Vec<Observation>> {
    if raw_memories.is_empty() {
        return Ok(vec![]);
    }

    let user_prompt = format_messages(raw_memories);
    let response = llm.complete(&user_prompt, Some(EXTRACTION_SYSTEM_PROMPT)).await?;

    let cleaned = repair_json(&response);
    let parsed: ExtractionResult = serde_json::from_str(&cleaned).map_err(|e| {
        MemoryError::LlmInvalidJson {
            source: e,
            raw: response.clone(),
        }
    })?;

    let memory_id = &raw_memories[0].memory_id;
    let workspace_id = &raw_memories[0].workspace_id;

    let observations: Vec<Observation> = parsed
        .observations
        .into_iter()
        .map(|raw| raw_to_observation(raw, memory_id, workspace_id))
        .collect();

    Ok(observations)
}

fn format_messages(memories: &[RawMemory]) -> String {
    let mut prompt = String::from("Input messages:\n");
    for m in memories {
        prompt.push_str(&format!("[{}]: {}\n", m.role, m.content));
    }
    prompt.push_str("\nOutput:");
    prompt
}

fn repair_json(raw: &str) -> String {
    let s = raw.trim();
    // Strip markdown code fences
    if s.starts_with("```json") {
        let inner = s.trim_start_matches("```json").trim_start_matches('\n');
        return inner.trim_end_matches('`').trim().to_string();
    }
    if s.starts_with("```") {
        let inner = s.trim_start_matches("```").trim_start_matches('\n');
        return inner.trim_end_matches('`').trim().to_string();
    }
    s.to_string()
}

fn raw_to_observation(raw: RawObservation, memory_id: &str, workspace_id: &str) -> Observation {
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        memory_id: memory_id.to_string(),
        subject_text: raw.subject_text,
        subject_type: raw.subject_type,
        predicate: raw.predicate,
        object_text: raw.object_text,
        object_type: raw.object_type,
        evidence_text: raw.evidence_text,
        confidence: raw.confidence,
        evidence_alpha: 1.0,
        evidence_beta: 1.0,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type: parse_source_type(&raw.source_type),
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn parse_source_type(s: &str) -> ObservationSourceType {
    match s {
        "user_message" => ObservationSourceType::UserMessage,
        "user_confirm" => ObservationSourceType::UserConfirm,
        "user_negation" => ObservationSourceType::UserNegation,
        "assistant_guess" => ObservationSourceType::AssistantGuess,
        "file_evidence" => ObservationSourceType::FileEvidence,
        _ => ObservationSourceType::AssistantGuess,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_json_strips_fences() {
        let input = "```json\n{\"observations\":[]}\n```";
        assert_eq!(repair_json(input), "{\"observations\":[]}");
    }

    #[test]
    fn repair_json_handles_plain() {
        let input = r#"{"observations":[]}"#;
        assert_eq!(repair_json(input), input);
    }

    #[test]
    fn parse_extraction_result() {
        let json = r#"{"observations":[{"subject_text":"POSMASK","subject_type":"database_table","predicate":"not_has_field","object_text":"机器字段","object_type":"field","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message","confidence":0.9}]}"#;
        let result: ExtractionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.observations.len(), 1);
        assert_eq!(result.observations[0].subject_text, "POSMASK");
        assert_eq!(result.observations[0].predicate, "not_has_field");
    }

    #[test]
    fn parse_empty_result() {
        let json = r#"{"observations":[]}"#;
        let result: ExtractionResult = serde_json::from_str(json).unwrap();
        assert!(result.observations.is_empty());
    }

    #[test]
    fn format_messages_basic() {
        let memories = vec![
            RawMemory {
                memory_id: "m1".into(),
                workspace_id: "ws".into(),
                session_id: "s1".into(),
                role: "user".into(),
                content: "Hello".into(),
                source_type: crate::models::raw_memory::SourceType::SessionFile,
                source_ref: "test.json".into(),
                created_at: "2026-01-01".into(),
            },
        ];
        let result = format_messages(&memories);
        assert!(result.contains("[user]: Hello"));
    }
}
