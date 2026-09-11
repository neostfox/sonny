use serde::Deserialize;

use crate::confidence::{BetaConfidence, EvidenceType};
use crate::entity::canonical_key_light;
use crate::error::{MemoryError, MemoryResult};
use crate::llm::traits::LlmProvider;
use crate::models::observation::{Observation, ObservationSourceType};
use crate::models::predicate::normalize_predicate;
use crate::models::raw_memory::RawMemory;
use crate::models::status::ObservationStatus;
use crate::store::traits::{ObservationStore, RawMemoryStore};

/// Prompt version stamped on every extraction. Bump when EXTRACTION_SYSTEM_PROMPT changes;
/// consumed by P2-D reverse-correction to detect stale extractions needing re-extraction.
pub const EXTRACTION_PROMPT_VERSION: &str = "2026-06-14.v1";

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
      "predicate": "has | not_has | depends_on | not_depends_on | related_to | not_related_to | causes",
      "object_text": "target entity or value",
      "object_type": "target type or null",
      "evidence_text": "exact verbatim quote",
      "source_type": "user_message | user_confirm | user_negation | assistant_guess | file_evidence"
    }
  ]
}

## Source Type Rules
- user_message: The user stated this directly
- user_confirm: The user confirmed something the assistant suggested
- user_negation: The user denied or corrected something
- assistant_guess: The assistant inferred or speculated this
- file_evidence: Derived from file/project content

## Predicate Vocabulary
Use ONLY these canonical predicates (snake_case):
- has: A contains/has field or part B
- not_has: A does NOT have B (always preserve negative information)
- depends_on: A depends on/requires B
- not_depends_on: A does NOT depend on B
- related_to: A is associated with / may relate to B
- not_related_to: A is NOT related to B
- causes: A is the root cause of / leads to B
Pick the most specific canonical predicate. If none fits, use "related_to".

## Language Handling
- Input may contain mixed Chinese and English text
- Preserve the original language in subject_text, object_text, and evidence_text
- Do NOT translate any text
- Predicates must always be one of the canonical values in Predicate Vocabulary above
- Entity types must always be in English

## Output ONLY valid JSON. No markdown fences, no explanation.

## Examples

Input:
[user]: POSMASK 表没有机器字段

Output:
{"observations":[{"subject_text":"POSMASK","subject_type":"database_table","predicate":"not_has","object_text":"机器字段","object_type":"field","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"}]}

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
}

/// Anti-hallucination gate: an observation's evidence_text must be a non-empty verbatim
/// substring of at least one source RawMemory, otherwise it is dropped (design §2.4).
fn evidence_is_supported(evidence: Option<&str>, raw_memories: &[RawMemory]) -> bool {
    match evidence {
        Some(ev) if !ev.is_empty() => raw_memories.iter().any(|m| m.content.contains(ev)),
        _ => false,
    }
}

pub async fn extract_observations(
    raw_memories: &[RawMemory],
    llm: &impl LlmProvider,
) -> MemoryResult<Vec<Observation>> {
    if raw_memories.is_empty() {
        return Ok(vec![]);
    }

    let user_prompt = format_messages(raw_memories);
    let response = llm
        .complete(&user_prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await?;

    let cleaned = repair_json(&response);
    let parsed: ExtractionResult =
        serde_json::from_str(&cleaned).map_err(|e| MemoryError::LlmInvalidJson {
            source: e,
            raw: response.clone(),
        })?;

    let total = parsed.observations.len();
    let memory_id = &raw_memories[0].memory_id;
    let workspace_id = &raw_memories[0].workspace_id;
    // P2-C: one batch id per extraction call groups co-claimed observations.
    let extraction_batch_id = uuid::Uuid::new_v4().to_string();

    let observations: Vec<Observation> = parsed
        .observations
        .into_iter()
        .filter(|raw| evidence_is_supported(raw.evidence_text.as_deref(), raw_memories))
        .map(|raw| raw_to_observation(raw, memory_id, workspace_id, &extraction_batch_id))
        .collect();

    let dropped = total - observations.len();
    if dropped > 0 {
        tracing::warn!(
            "extraction validation dropped {dropped} observation(s) with unsupported evidence_text (prompt {EXTRACTION_PROMPT_VERSION})"
        );
    }

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

fn raw_to_observation(
    raw: RawObservation,
    memory_id: &str,
    workspace_id: &str,
    extraction_batch_id: &str,
) -> Observation {
    let source_type = parse_source_type(&raw.source_type);
    // P3-D: seed fact_confidence from the source's evidence weight. Source *trust* is
    // already captured by extraction_confidence; this initializes the accumulated
    // *evidence* dimension. UserMessage seeds nothing — a raw claim awaits corroboration.
    let mut evidence = BetaConfidence::new();
    if let Some(et) = source_type.initial_evidence() {
        evidence.update(&et);
    }
    Observation {
        observation_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        memory_id: memory_id.to_string(),
        subject_text: canonical_key_light(&raw.subject_text),
        subject_type: raw.subject_type,
        predicate: normalize_predicate(&raw.predicate),
        object_text: raw.object_text.map(|o| canonical_key_light(&o)),
        object_type: raw.object_type,
        evidence_text: raw.evidence_text,
        extraction_confidence: source_type.extraction_confidence(),
        evidence_alpha: evidence.alpha,
        evidence_beta: evidence.beta,
        status: ObservationStatus::Candidate,
        surprise_score: 0.5,
        source_type,
        memory_type_candidate: None,
        observation_detail_json: None,
        extraction_batch_id: Some(extraction_batch_id.to_string()),
        superseded_by: None,
        cross_project_count: 1,
        causal_role: None,
        consolidated: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Extract observations and drop any already present in the store (matched by
/// normalized subject + predicate + object). Caller inserts the returned batch.
pub async fn extract_and_dedup<S: ObservationStore>(
    raw_memories: &[RawMemory],
    llm: &impl LlmProvider,
    store: &S,
) -> MemoryResult<Vec<Observation>> {
    let extracted = extract_observations(raw_memories, llm).await?;
    let mut kept = Vec::with_capacity(extracted.len());
    let mut duplicates = 0;
    for obs in extracted {
        if let Some(existing) = store.find_duplicate(
            &obs.subject_text,
            &obs.predicate,
            obs.object_text.as_deref(),
            &obs.workspace_id,
        )? {
            // Re-encountered fact: accumulate evidence on the existing observation
            // (P3-D). The fresh duplicate is dropped; the original strengthens.
            let mut bc =
                BetaConfidence::with_values(existing.evidence_alpha, existing.evidence_beta);
            bc.update(&EvidenceType::RepeatedOccurrence);
            store.update_confidence(&existing.observation_id, bc.alpha, bc.beta)?;
            duplicates += 1;
        } else {
            // P6-B: same triple may already live in another workspace — keep
            // cross_project_count coherent before the caller inserts this row.
            store.sync_cross_project_count(
                &obs.subject_text,
                &obs.predicate,
                obs.object_text.as_deref(),
            )?;
            kept.push(obs);
        }
    }
    if duplicates > 0 {
        tracing::info!(
            "dedup dropped {duplicates} duplicate observation(s) against existing store"
        );
    }
    Ok(kept)
}

/// P2-D: outcome of [`reextract`] — the fresh batch (already inserted) and how many
/// stale observations were superseded.
#[derive(Debug, Clone)]
pub struct ReextractOutcome {
    pub new_observations: Vec<Observation>,
    pub superseded: usize,
}

/// P2-D: re-extract a session and replace its prior observations.
///
/// Loads the session's raw memories, re-runs extraction with the *current* prompt,
/// then atomically supersedes the session's old observations and inserts the fresh
/// batch (one transaction — a failure leaves the old rows live). Finally stamps
/// [`EXTRACTION_PROMPT_VERSION`] onto the session. Old rows are retained; recall
/// filters them out by status.
///
/// Uses [`extract_observations`] (not `extract_and_dedup`) deliberately: the old
/// observations are still live during extraction, so dedup-against-store would
/// wrongly drop the replacements.
pub async fn reextract<R, O>(
    session_id: &str,
    llm: &impl LlmProvider,
    raw_store: &R,
    obs_store: &O,
) -> MemoryResult<ReextractOutcome>
where
    R: RawMemoryStore,
    O: ObservationStore,
{
    let raw_memories = raw_store.get_by_session(session_id)?;
    if raw_memories.is_empty() {
        return Ok(ReextractOutcome {
            new_observations: vec![],
            superseded: 0,
        });
    }

    // Re-extract with the current prompt; this mints a shared extraction_batch_id.
    let new_observations = extract_observations(&raw_memories, llm).await?;

    // Atomic supersede-and-replace: old rows flip to Superseded and the new batch is
    // inserted in one transaction, so a mid-step failure can't orphan the session.
    let superseded = obs_store.replace_session_observations(session_id, &new_observations)?;

    raw_store.set_session_extraction_version(session_id, EXTRACTION_PROMPT_VERSION)?;

    tracing::info!(
        "reextract(session={session_id}): superseded {superseded} observation(s), extracted {} new (prompt {EXTRACTION_PROMPT_VERSION})",
        new_observations.len()
    );

    Ok(ReextractOutcome {
        new_observations,
        superseded,
    })
}

fn parse_source_type(s: &str) -> ObservationSourceType {
    s.parse().unwrap_or_else(|_| {
        tracing::warn!("Unknown observation source type '{s}', defaulting to assistant_guess");
        ObservationSourceType::AssistantGuess
    })
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
        let memories = vec![RawMemory {
            memory_id: "m1".into(),
            workspace_id: "ws".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "Hello".into(),
            source_type: crate::models::raw_memory::SourceType::SessionFile,
            source_ref: "test.json".into(),
            extraction_version: None,
            created_at: "2026-01-01".into(),
        }];
        let result = format_messages(&memories);
        assert!(result.contains("[user]: Hello"));
    }

    /// Local mock LLM — avoids the two-versions-of-memory_runtime conflict that the
    /// memory_test_fixtures mock triggers inside lib unit tests.
    struct EchoLlm {
        response: String,
    }

    #[async_trait::async_trait]
    impl crate::llm::traits::LlmProvider for EchoLlm {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> MemoryResult<String> {
            Ok(self.response.clone())
        }
        async fn health_check(&self) -> MemoryResult<bool> {
            Ok(true)
        }
        fn name(&self) -> &str {
            "echo"
        }
    }

    #[tokio::test]
    async fn extract_drops_observations_with_unsupported_evidence() {
        // LLM returns one grounded observation and one hallucinated (evidence not in source).
        let mock = EchoLlm {
            response: r#"{"observations":[
                {"subject_text":"POSMASK","predicate":"not_has_field","object_text":"机器字段","evidence_text":"POSMASK 表没有机器字段","source_type":"user_message"},
                {"subject_text":"ORDERHDR","predicate":"has_field","object_text":"金额","evidence_text":"ORDERHDR 表里有金额字段","source_type":"user_message"}
            ]}"#
                .into(),
        };

        let raw = RawMemory {
            memory_id: "m1".into(),
            workspace_id: "ws".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "POSMASK 表没有机器字段".into(),
            source_type: crate::models::raw_memory::SourceType::SessionFile,
            source_ref: "t.json".into(),
            extraction_version: None,
            created_at: "2026-01-01".into(),
        };
        let observations = extract_observations(&[raw], &mock).await.unwrap();

        // Hallucinated evidence (not a substring of source) is filtered out.
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].subject_text, "posmask");
        // P2-C: every observation from one extraction call shares a batch id.
        assert!(observations[0].extraction_batch_id.is_some());
    }
}
