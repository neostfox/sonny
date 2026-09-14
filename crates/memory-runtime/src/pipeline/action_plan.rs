//! P12: Memory action planning (MindMemOS Add/Reinforce/Update/Merge/Skip).
//!
//! Before writing a freshly extracted observation, decide what it *is* relative
//! to what the store already knows — not "insert or accumulate α blindly".

use crate::entity::canonical_key_light;
use crate::models::observation::Observation;

/// Explicit write plan for one candidate observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAction {
    /// No matching live row — insert as a new observation.
    Add,
    /// Same triple already live — strengthen the existing row's α.
    Reinforce,
    /// Same subject+predicate, object changed — supersede old with candidate.
    Update,
    /// Same subject+object, related predicate variants — merge into one.
    Merge,
    /// Untrusted / empty / already superseded equivalent — drop.
    Skip,
}

impl MemoryAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Reinforce => "reinforce",
            Self::Update => "update",
            Self::Merge => "merge",
            Self::Skip => "skip",
        }
    }
}

/// Existing live observations relevant to a candidate (same subject or object).
#[derive(Debug, Clone)]
pub struct ExistingMatch {
    pub observation: Observation,
}

fn same_triple(a: &Observation, b: &Observation) -> bool {
    canonical_key_light(&a.subject_text) == canonical_key_light(&b.subject_text)
        && a.predicate == b.predicate
        && a.object_text.as_deref().map(canonical_key_light)
            == b.object_text.as_deref().map(canonical_key_light)
}

fn same_subject_predicate(a: &Observation, b: &Observation) -> bool {
    canonical_key_light(&a.subject_text) == canonical_key_light(&b.subject_text)
        && a.predicate == b.predicate
}

fn same_subject_object(a: &Observation, b: &Observation) -> bool {
    canonical_key_light(&a.subject_text) == canonical_key_light(&b.subject_text)
        && a.object_text.as_deref().map(canonical_key_light)
            == b.object_text.as_deref().map(canonical_key_light)
}

/// Deterministic action planner.
///
/// Priority:
/// 1. Skip when candidate has empty subject or zero-weight assistant guess with no evidence.
/// 2. Reinforce when an exact live triple exists.
/// 3. Update when subject+predicate match but object differs (fact revised).
/// 4. Merge when subject+object match under related predicates.
/// 5. Add otherwise.
pub fn plan_memory_action(candidate: &Observation, existing: &[Observation]) -> MemoryAction {
    if candidate.subject_text.trim().is_empty() {
        return MemoryAction::Skip;
    }
    let evidence_text = candidate.evidence_text.as_deref().unwrap_or("");
    if evidence_text.trim().is_empty() && candidate.source_type.as_str() == "assistant_guess" {
        return MemoryAction::Skip;
    }

    let live = || {
        existing.iter().filter(|o| {
            !matches!(
                o.status,
                crate::models::status::ObservationStatus::Superseded
                    | crate::models::status::ObservationStatus::Rejected
                    | crate::models::status::ObservationStatus::Deprecated
            )
        })
    };

    if live().any(|o| same_triple(candidate, o)) {
        return MemoryAction::Reinforce;
    }
    if live().any(|o| same_subject_predicate(candidate, o)) {
        return MemoryAction::Update;
    }
    if live().any(|o| same_subject_object(candidate, o)) {
        return MemoryAction::Merge;
    }
    MemoryAction::Add
}

/// MindMemOS recall-aware envelope: separate extractable evidence from
/// contextual memories used only for disambiguation / dedup / conflict.
#[derive(Debug, Clone)]
pub struct RecallEnvelope {
    /// Messages that may support new observation content.
    pub evidence_messages: Vec<String>,
    /// Existing memories handed to the extractor as context only.
    pub context_memories: Vec<String>,
}

/// Build a recall-aware extraction envelope.
pub fn build_recall_envelope(
    evidence_messages: impl IntoIterator<Item = String>,
    related_memories: impl IntoIterator<Item = String>,
) -> RecallEnvelope {
    RecallEnvelope {
        evidence_messages: evidence_messages.into_iter().collect(),
        context_memories: related_memories.into_iter().collect(),
    }
}

/// Render the envelope as a prompt suffix (for extract_observations callers).
pub fn envelope_prompt_block(envelope: &RecallEnvelope) -> String {
    let mut out = String::from("\n## Contextual existing memories (DO NOT restate unless confirmed by evidence)\n");
    if envelope.context_memories.is_empty() {
        out.push_str("(none)\n");
        return out;
    }
    for m in &envelope.context_memories {
        out.push_str("- ");
        out.push_str(m);
        out.push('\n');
    }
    out.push_str("\n## Extractable evidence messages\n");
    for m in &envelope.evidence_messages {
        out.push_str(m);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::observation::ObservationSourceType;
    use crate::models::status::ObservationStatus;

    fn obs(id: &str, subj: &str, pred: &str, obj: Option<&str>) -> Observation {
        Observation {
            observation_id: id.into(),
            workspace_id: "ws".into(),
            memory_id: "m".into(),
            subject_text: subj.into(),
            subject_type: None,
            predicate: pred.into(),
            object_text: obj.map(|s| s.to_string()),
            object_type: None,
            evidence_text: Some("quote".into()),
            extraction_confidence: 0.8,
            evidence_alpha: 2.0,
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: ObservationSourceType::UserMessage,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: None,
            superseded_by: None,
            cross_project_count: 1,
            causal_role: None,
            consolidated: false,
            created_at: "t".into(),
        }
    }

    #[test]
    fn empty_subject_is_skip() {
        let c = obs("c", "  ", "has", Some("x"));
        assert_eq!(plan_memory_action(&c, &[]), MemoryAction::Skip);
    }

    #[test]
    fn guess_without_evidence_is_skip() {
        let mut c = obs("c", "a", "has", Some("b"));
        c.source_type = ObservationSourceType::AssistantGuess;
        c.evidence_text = Some("   ".into());
        assert_eq!(plan_memory_action(&c, &[]), MemoryAction::Skip);
    }

    #[test]
    fn no_match_is_add() {
        let c = obs("c", "a", "has", Some("b"));
        assert_eq!(plan_memory_action(&c, &[]), MemoryAction::Add);
    }

    #[test]
    fn exact_triple_is_reinforce() {
        let existing = obs("e", "a", "has", Some("b"));
        let c = obs("c", "a", "has", Some("b"));
        assert_eq!(plan_memory_action(&c, &[existing]), MemoryAction::Reinforce);
    }

    #[test]
    fn object_change_is_update() {
        let existing = obs("e", "a", "has", Some("b"));
        let c = obs("c", "a", "has", Some("c"));
        assert_eq!(plan_memory_action(&c, &[existing]), MemoryAction::Update);
    }

    #[test]
    fn related_predicate_same_object_is_merge() {
        let existing = obs("e", "a", "related_to", Some("b"));
        let c = obs("c", "a", "depends_on", Some("b"));
        assert_eq!(plan_memory_action(&c, &[existing]), MemoryAction::Merge);
    }

    #[test]
    fn envelope_separates_evidence_from_context() {
        let env = build_recall_envelope(
            ["user: POSMASK 没有机器字段".to_string()],
            ["known: POSMASK is a table".to_string()],
        );
        let block = envelope_prompt_block(&env);
        assert!(block.contains("Contextual existing memories"));
        assert!(block.contains("POSMASK is a table"));
        assert!(block.contains("Extractable evidence"));
    }
}
