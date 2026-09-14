//! P10: Dreaming — entity-neighborhood offline consolidation.
//!
//! Inspired by MindMemOS Dreaming: online extract stays fast; dreaming
//! reorganizes accumulated live observations after the fact. Scope is
//! entity-centered (not global compare, not pure pairwise): each unconsolidated
//! observation seeds a neighborhood of observations sharing its subject/object
//! entities. Detect issues, then apply conservative mutations, then mark
//! consolidated so the same rows are not re-processed.

use std::collections::{BTreeMap, BTreeSet};

use crate::confidence::BetaConfidence;
use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::observation::Observation;
use crate::models::status::ObservationStatus;
use crate::store::traits::ObservationStore;

/// Issue kinds detected in a neighborhood (MindMemOS detect stage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DreamIssue {
    /// Live rows share the same (subject, predicate, object).
    DuplicateTriple,
    /// Same subject+object, opposite predicates (`has` vs `not_has`, …).
    Conflict,
    /// Low-trust source with weak posterior — archive candidate.
    LowValue,
}

/// Conservative mutation planned for one neighborhood (MindMemOS act stage).
#[derive(Debug, Clone, PartialEq)]
pub enum DreamAction {
    /// Merge `duplicate_ids` into `keep_id` (α accumulate).
    MergeDuplicates {
        keep_id: String,
        duplicate_ids: Vec<String>,
    },
    /// Raise β on conflicting counterparts around `subject`/`object`.
    SoftenConflict {
        observation_ids: Vec<String>,
        alpha: f64,
        beta: f64,
    },
    /// Archive a low-value observation.
    Archive { observation_id: String },
    /// No mutation; only mark consolidated.
    MarkOnly,
}

#[derive(Debug, Clone, Default)]
pub struct DreamReport {
    pub neighborhoods: usize,
    pub merges: usize,
    pub softenings: usize,
    pub archived: usize,
    pub marked: usize,
}

/// Predicate negation pairs used for conflict detection.
fn is_negation_pair(a: &str, b: &str) -> bool {
    matches!(
        (a, b),
        ("has", "not_has")
            | ("not_has", "has")
            | ("depends_on", "not_depends_on")
            | ("not_depends_on", "depends_on")
            | ("related_to", "not_related_to")
            | ("not_related_to", "related_to")
    )
}

fn is_live(status: &ObservationStatus) -> bool {
    matches!(
        status,
        ObservationStatus::Candidate
            | ObservationStatus::FastStored
            | ObservationStatus::Confirmed
            | ObservationStatus::AutoConfirmed
    )
}

fn entities_of(obs: &Observation) -> Vec<String> {
    let mut v = vec![canonical_key_light(&obs.subject_text)];
    if let Some(o) = obs.object_text.as_deref().filter(|s| !s.is_empty()) {
        v.push(canonical_key_light(o));
    }
    v
}

/// Detect issues inside one entity neighborhood (pure).
pub fn detect_neighborhood(cluster: &[Observation]) -> Vec<(DreamIssue, Vec<String>)> {
    let mut out = Vec::new();

    // Duplicate triples (live only).
    let mut by_triple: BTreeMap<(String, String, Option<String>), Vec<String>> = BTreeMap::new();
    for obs in cluster.iter().filter(|o| is_live(&o.status)) {
        by_triple
            .entry((
                obs.subject_text.clone(),
                obs.predicate.clone(),
                obs.object_text.clone(),
            ))
            .or_default()
            .push(obs.observation_id.clone());
    }
    for ids in by_triple.values() {
        if ids.len() > 1 {
            out.push((DreamIssue::DuplicateTriple, ids.clone()));
        }
    }

    // Conflicts: same subject+object, opposite predicates.
    let mut by_pair: BTreeMap<(String, Option<String>), Vec<&Observation>> = BTreeMap::new();
    for obs in cluster.iter().filter(|o| is_live(&o.status)) {
        by_pair
            .entry((obs.subject_text.clone(), obs.object_text.clone()))
            .or_default()
            .push(obs);
    }
    for group in by_pair.values() {
        let mut has_pos = Vec::new();
        let mut has_neg = Vec::new();
        for obs in group {
            if obs.predicate.starts_with("not_") {
                has_neg.push(obs);
            } else {
                has_pos.push(obs);
            }
        }
        // Only flag when both sides exist under a known negation pairing.
        let conflict = has_pos.iter().any(|p| {
            has_neg
                .iter()
                .any(|n| is_negation_pair(&p.predicate, &n.predicate))
        });
        if conflict {
            let ids: Vec<String> = group.iter().map(|o| o.observation_id.clone()).collect();
            out.push((DreamIssue::Conflict, ids));
        }
    }

    // Low value: assistant_guess with weak fact posterior.
    for obs in cluster.iter().filter(|o| is_live(&o.status)) {
        if obs.source_type.as_str() == "assistant_guess" && obs.fact_confidence() < 0.45 {
            out.push((DreamIssue::LowValue, vec![obs.observation_id.clone()]));
        }
    }

    out
}

/// Turn detected issues into conservative mutations (pure). Prefer merge and
/// soften over archive when identity is uncertain (MindMemOS planner rule).
pub fn plan_actions(
    issues: &[(DreamIssue, Vec<String>)],
    cluster: &[Observation],
) -> Vec<DreamAction> {
    let by_id: BTreeMap<&str, &Observation> = cluster
        .iter()
        .map(|o| (o.observation_id.as_str(), o))
        .collect();
    let mut actions = Vec::new();
    let mut merged: BTreeSet<String> = BTreeSet::new();

    for (issue, ids) in issues {
        match issue {
            DreamIssue::DuplicateTriple => {
                if ids.len() < 2 {
                    continue;
                }
                // Keep the highest effective-confidence live row.
                let mut ranked: Vec<&Observation> = ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).copied())
                    .filter(|o| is_live(&o.status))
                    .collect();
                ranked.sort_by(|a, b| {
                    b.effective_confidence()
                        .total_cmp(&a.effective_confidence())
                });
                if ranked.is_empty() {
                    continue;
                }
                let keep = ranked[0].observation_id.clone();
                let dups: Vec<String> = ranked
                    .iter()
                    .skip(1)
                    .map(|o| o.observation_id.clone())
                    .filter(|id| !merged.contains(id))
                    .collect();
                if dups.is_empty() {
                    continue;
                }
                for id in &dups {
                    merged.insert(id.clone());
                }
                actions.push(DreamAction::MergeDuplicates {
                    keep_id: keep,
                    duplicate_ids: dups,
                });
            }
            DreamIssue::Conflict => {
                // Soften all live counterparts: pull Beta toward 0.5 without archiving.
                let live: Vec<String> = ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()))
                    .filter(|o| is_live(&o.status))
                    .map(|o| o.observation_id.clone())
                    .filter(|id| !merged.contains(id))
                    .collect();
                if live.len() < 2 {
                    continue;
                }
                // Preserve current alphas, add equal conflicting mass to β.
                let mut alpha: f64 = 1.0;
                let mut beta: f64 = 1.0;
                for id in &live {
                    if let Some(o) = by_id.get(id.as_str()) {
                        if o.evidence_alpha > alpha {
                            alpha = o.evidence_alpha;
                        }
                        beta = o.evidence_beta + 1.5;
                    }
                }
                actions.push(DreamAction::SoftenConflict {
                    observation_ids: live,
                    alpha,
                    beta,
                });
            }
            DreamIssue::LowValue => {
                if let Some(id) = ids.first() {
                    if !merged.contains(id) {
                        if let Some(o) = by_id.get(id.as_str()) {
                            // Only archive when evidence is truly weak.
                            if o.evidence_alpha <= 1.2 && o.fact_confidence() < 0.45 {
                                actions.push(DreamAction::Archive {
                                    observation_id: id.clone(),
                                });
                                merged.insert(id.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if actions.is_empty() {
        actions.push(DreamAction::MarkOnly);
    }
    actions
}

/// Run dreaming over a workspace: seed from unconsolidated live observations,
/// expand by entity neighborhood, detect → plan → apply.
pub fn dream_workspace<O: ObservationStore>(
    observations: &O,
    workspace_id: &str,
) -> MemoryResult<DreamReport> {
    let all = observations.list_by_workspace(workspace_id, None)?;
    let live: Vec<Observation> = all
        .into_iter()
        .filter(|o| is_live(&o.status) && !o.consolidated)
        .collect();
    if live.is_empty() {
        return Ok(DreamReport::default());
    }

    // Entity → observation ids (index over all live rows, including already
    // consolidated ones so neighborhoods see the full current state).
    let all_live: Vec<Observation> = observations
        .list_by_workspace(workspace_id, None)?
        .into_iter()
        .filter(|o| is_live(&o.status))
        .collect();
    let mut entity_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for obs in &all_live {
        for e in entities_of(obs) {
            entity_index.entry(e).or_default().push(obs.observation_id.clone());
        }
    }
    let by_id: BTreeMap<String, Observation> = all_live
        .into_iter()
        .map(|o| (o.observation_id.clone(), o))
        .collect();

    let mut report = DreamReport::default();
    let mut visited: BTreeSet<String> = BTreeSet::new();

    for seed in &live {
        if visited.contains(&seed.observation_id) {
            continue;
        }
        // Entity-neighborhood expansion (1 hop).
        let mut members: BTreeSet<String> = BTreeSet::new();
        for e in entities_of(seed) {
            if let Some(ids) = entity_index.get(&e) {
                members.extend(ids.iter().cloned());
            }
        }
        members.insert(seed.observation_id.clone());
        // Skip pure singleton low-value handled below via detect.
        let cluster: Vec<Observation> = members
            .iter()
            .filter_map(|id| by_id.get(id))
            .cloned()
            .collect();
        if cluster.is_empty() {
            continue;
        }
        report.neighborhoods += 1;
        for id in &members {
            visited.insert(id.clone());
        }

        let issues = detect_neighborhood(&cluster);
        let actions = plan_actions(&issues, &cluster);

        for action in &actions {
            match action {
                DreamAction::MergeDuplicates {
                    keep_id,
                    duplicate_ids,
                } => {
                    let Some(keep) = by_id.get(keep_id) else {
                        continue;
                    };
                    let mut bc =
                        BetaConfidence::with_values(keep.evidence_alpha, keep.evidence_beta);
                    for dup_id in duplicate_ids {
                        if let Some(dup) = by_id.get(dup_id) {
                            bc.update(&crate::confidence::EvidenceType::RepeatedOccurrence);
                            observations.supersede(
                                dup_id,
                                &format!("dream-merge:{}", keep_id),
                            )?;
                            let _ = dup;
                        }
                    }
                    observations.update_confidence(keep_id, bc.alpha, bc.beta)?;
                    report.merges += 1;
                }
                DreamAction::SoftenConflict {
                    observation_ids,
                    alpha: _,
                    beta: _,
                } => {
                    for id in observation_ids {
                        if let Some(o) = by_id.get(id) {
                            let mut bc =
                                BetaConfidence::with_values(o.evidence_alpha, o.evidence_beta);
                            bc.update(&crate::confidence::EvidenceType::ConflictingEvidence);
                            observations.update_confidence(id, bc.alpha, bc.beta)?;
                        }
                    }
                    report.softenings += 1;
                }
                DreamAction::Archive { observation_id } => {
                    observations.update_status(
                        observation_id,
                        ObservationStatus::Deprecated,
                    )?;
                    report.archived += 1;
                }
                DreamAction::MarkOnly => {}
            }
        }

        // Mark every member consolidated so the next dream skips it.
        for id in &members {
            if let Err(err) = observations.set_consolidated(id, true) {
                tracing::debug!(observation_id = %id, %err, "dream mark consolidated failed");
            } else {
                report.marked += 1;
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::observation::ObservationSourceType;

    fn obs(id: &str, subj: &str, pred: &str, obj: Option<&str>, src: &str) -> Observation {
        Observation {
            observation_id: id.into(),
            workspace_id: "ws".into(),
            memory_id: format!("m-{id}"),
            subject_text: subj.into(),
            subject_type: None,
            predicate: pred.into(),
            object_text: obj.map(|s| s.to_string()),
            object_type: None,
            evidence_text: Some("ev".into()),
            extraction_confidence: 0.7,
            evidence_alpha: if src == "assistant_guess" { 1.0 } else { 3.0 },
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.5,
            source_type: match src {
                "assistant_guess" => ObservationSourceType::AssistantGuess,
                _ => ObservationSourceType::UserMessage,
            },
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
    fn detects_duplicate_triples() {
        let cluster = vec![
            obs("a", "posmask", "has", Some("field"), "user_message"),
            obs("b", "posmask", "has", Some("field"), "user_message"),
        ];
        let issues = detect_neighborhood(&cluster);
        assert!(issues
            .iter()
            .any(|(i, ids)| *i == DreamIssue::DuplicateTriple && ids.len() == 2));
    }

    #[test]
    fn detects_predicate_conflict() {
        let cluster = vec![
            obs("a", "posmask", "has", Some("field"), "user_message"),
            obs("b", "posmask", "not_has", Some("field"), "user_message"),
        ];
        let issues = detect_neighborhood(&cluster);
        assert!(issues
            .iter()
            .any(|(i, _)| *i == DreamIssue::Conflict));
    }

    #[test]
    fn weak_assistant_guess_is_low_value() {
        let mut o = obs("g", "x", "related_to", Some("y"), "assistant_guess");
        o.evidence_alpha = 1.0;
        o.evidence_beta = 2.0; // fact_confidence = 1/3 < 0.45
        let issues = detect_neighborhood(&[o]);
        assert!(issues.iter().any(|(i, _)| *i == DreamIssue::LowValue));
    }

    #[test]
    fn plan_merges_duplicates_keeping_strongest() {
        let mut strong = obs("s", "posmask", "has", Some("field"), "user_message");
        strong.evidence_alpha = 5.0;
        let weak = obs("w", "posmask", "has", Some("field"), "user_message");
        let cluster = vec![strong, weak];
        let issues = detect_neighborhood(&cluster);
        let actions = plan_actions(&issues, &cluster);
        assert!(actions.iter().any(|a| matches!(
            a,
            DreamAction::MergeDuplicates { keep_id, duplicate_ids }
                if keep_id == "s" && duplicate_ids == &vec!["w".to_string()]
        )));
    }

    #[test]
    fn strong_guess_is_not_archived() {
        let mut o = obs("g", "x", "related_to", Some("y"), "assistant_guess");
        o.evidence_alpha = 8.0;
        let issues = detect_neighborhood(&[o.clone()]);
        // fact_confidence high → not LowValue
        assert!(!issues.iter().any(|(i, _)| *i == DreamIssue::LowValue));
    }
}
