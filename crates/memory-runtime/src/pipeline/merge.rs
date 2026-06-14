use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use uuid::Uuid;

use crate::error::{MemoryError, MemoryResult};
use crate::models::concept::ConceptCandidate;
use crate::models::observation::Observation;
use crate::models::status::CandidateStatus;
use crate::pipeline::cluster::ObservationCluster;

#[derive(Debug, Clone)]
pub struct MergeSplitConfig {
    pub entity_jaccard_threshold: f64,
    pub broad_candidate_min_observations: usize,
}

impl Default for MergeSplitConfig {
    fn default() -> Self {
        Self {
            entity_jaccard_threshold: 0.60,
            broad_candidate_min_observations: 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CandidateAliases {
    pub candidate_id: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MergeResult {
    pub candidates: Vec<ConceptCandidate>,
    pub aliases: Vec<CandidateAliases>,
}

#[derive(Debug, Clone)]
pub struct MergeSplitEngine {
    config: MergeSplitConfig,
}

impl MergeSplitEngine {
    pub fn new(config: MergeSplitConfig) -> Self {
        Self { config }
    }

    pub fn merge_overlapping_candidates(
        &self,
        candidates: &[ConceptCandidate],
    ) -> MemoryResult<MergeResult> {
        if candidates.is_empty() {
            return Ok(MergeResult {
                candidates: Vec::new(),
                aliases: Vec::new(),
            });
        }

        let mut parents: Vec<usize> = (0..candidates.len()).collect();
        let mut term_sets = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            term_sets.push(candidate_terms(candidate));
        }

        for left in 0..candidates.len() {
            for right in (left + 1)..candidates.len() {
                if jaccard(&term_sets[left], &term_sets[right])
                    > self.config.entity_jaccard_threshold
                {
                    union(&mut parents, left, right);
                }
            }
        }

        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for index in 0..candidates.len() {
            groups
                .entry(find(&mut parents, index))
                .or_default()
                .push(index);
        }

        let mut merged = Vec::with_capacity(groups.len());
        let mut aliases = Vec::new();
        for indexes in groups.values() {
            let group: Vec<&ConceptCandidate> =
                indexes.iter().map(|&index| &candidates[index]).collect();
            if group.len() == 1 {
                merged.push(group[0].clone());
                continue;
            }

            let candidate = merge_candidate_group(&group)?;
            let alias_names = alias_names(&group);
            aliases.push(CandidateAliases {
                candidate_id: candidate.candidate_id.clone(),
                aliases: alias_names,
            });
            merged.push(candidate);
        }

        Ok(MergeResult {
            candidates: merged,
            aliases,
        })
    }

    pub fn split_broad_candidate(
        &self,
        candidate: &ConceptCandidate,
        subclusters: &[ObservationCluster],
    ) -> MemoryResult<Vec<ConceptCandidate>> {
        if candidate.evidence_count < self.config.broad_candidate_min_observations as i64
            || subclusters.len() < 2
        {
            return Ok(vec![candidate.clone()]);
        }

        let source_observations =
            parse_json_set(&candidate.source_observations_json, "source_observations")?;
        let facts = parse_json_vec(&candidate.known_facts_json, "known_facts")?;
        let rejected = parse_json_vec(&candidate.rejected_hypotheses_json, "rejected_hypotheses")?;
        let questions = parse_json_vec(&candidate.open_questions_json, "open_questions")?;
        let evidence = parse_json_vec(&candidate.evidence_json, "evidence")?;
        let mut split = Vec::with_capacity(subclusters.len());

        for cluster in subclusters {
            let observations: Vec<&Observation> = cluster
                .observations
                .iter()
                .filter(|observation| {
                    source_observations.is_empty()
                        || source_observations.contains(&observation.observation_id)
                })
                .collect();
            if observations.is_empty() {
                continue;
            }
            split.push(candidate_from_observations(
                candidate,
                &observations,
                &facts,
                &rejected,
                &questions,
                &evidence,
            )?);
        }

        if split.len() < 2 {
            Ok(vec![candidate.clone()])
        } else {
            Ok(split)
        }
    }
}

impl Default for MergeSplitEngine {
    fn default() -> Self {
        Self::new(MergeSplitConfig::default())
    }
}

fn merge_candidate_group(candidates: &[&ConceptCandidate]) -> MemoryResult<ConceptCandidate> {
    let base = candidates[0];
    let now = Utc::now().to_rfc3339();
    let mut source_terms = BTreeSet::new();
    let mut source_sessions = BTreeSet::new();
    let mut source_observations = BTreeSet::new();
    let mut known_facts = BTreeSet::new();
    let mut rejected_hypotheses = BTreeSet::new();
    let mut open_questions = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    let mut evidence_count = 0_i64;
    let mut evidence_alpha = 0.0;
    let mut evidence_beta = 0.0;

    for candidate in candidates {
        source_terms.extend(candidate_terms(candidate));
        source_sessions.extend(parse_json_set(
            &candidate.source_sessions_json,
            "source_sessions",
        )?);
        source_observations.extend(parse_json_set(
            &candidate.source_observations_json,
            "source_observations",
        )?);
        known_facts.extend(parse_json_set(&candidate.known_facts_json, "known_facts")?);
        rejected_hypotheses.extend(parse_json_set(
            &candidate.rejected_hypotheses_json,
            "rejected_hypotheses",
        )?);
        open_questions.extend(parse_json_set(
            &candidate.open_questions_json,
            "open_questions",
        )?);
        evidence.extend(parse_json_set(&candidate.evidence_json, "evidence")?);
        evidence_count += candidate.evidence_count;
        evidence_alpha += candidate.evidence_alpha;
        evidence_beta += candidate.evidence_beta;
    }

    let evidence_count = if source_observations.is_empty() {
        evidence_count
    } else {
        source_observations.len() as i64
    };
    let confidence = confidence_from_evidence(evidence_alpha, evidence_beta);

    Ok(ConceptCandidate {
        candidate_id: base.candidate_id.clone(),
        workspace_id: base.workspace_id.clone(),
        name: base.name.clone(),
        summary: base.summary.clone(),
        source_terms_json: json_set(&source_terms, "source_terms")?,
        source_sessions_json: json_set(&source_sessions, "source_sessions")?,
        source_observations_json: json_set(&source_observations, "source_observations")?,
        known_facts_json: json_set(&known_facts, "known_facts")?,
        rejected_hypotheses_json: json_set(&rejected_hypotheses, "rejected_hypotheses")?,
        open_questions_json: json_set(&open_questions, "open_questions")?,
        evidence_json: json_set(&evidence, "evidence")?,
        evidence_count,
        confidence,
        evidence_alpha,
        evidence_beta,
        status: CandidateStatus::Candidate,
        last_recalled_at: base.last_recalled_at.clone(),
        recall_count: candidates
            .iter()
            .map(|candidate| candidate.recall_count)
            .sum(),
        successful_recall_count: candidates
            .iter()
            .map(|candidate| candidate.successful_recall_count)
            .sum(),
        failed_recall_count: candidates
            .iter()
            .map(|candidate| candidate.failed_recall_count)
            .sum(),
        created_at: base.created_at.clone(),
        updated_at: now,
    })
}

fn candidate_from_observations(
    parent: &ConceptCandidate,
    observations: &[&Observation],
    facts: &[String],
    rejected: &[String],
    questions: &[String],
    evidence: &[String],
) -> MemoryResult<ConceptCandidate> {
    let now = Utc::now().to_rfc3339();
    let mut terms = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    let mut observation_ids = BTreeSet::new();
    let mut evidence_alpha = 0.0;
    let mut evidence_beta = 0.0;

    for observation in observations {
        insert_non_empty(&mut terms, &observation.subject_text);
        if let Some(object) = &observation.object_text {
            insert_non_empty(&mut terms, object);
        }
        sessions.insert(observation.memory_id.clone());
        observation_ids.insert(observation.observation_id.clone());
        evidence_alpha += observation.evidence_alpha;
        evidence_beta += observation.evidence_beta;
    }

    let known_facts = filter_text_by_terms(facts, &terms);
    let rejected_hypotheses = filter_text_by_terms(rejected, &terms);
    let open_questions = filter_text_by_terms(questions, &terms);
    let evidence = filter_text_by_terms(evidence, &terms);
    let confidence = confidence_from_evidence(evidence_alpha, evidence_beta);

    Ok(ConceptCandidate {
        candidate_id: Uuid::new_v4().to_string(),
        workspace_id: parent.workspace_id.clone(),
        name: terms
            .first()
            .cloned()
            .unwrap_or_else(|| parent.name.clone()),
        summary: parent.summary.clone(),
        source_terms_json: json_set(&terms, "source_terms")?,
        source_sessions_json: json_set(&sessions, "source_sessions")?,
        source_observations_json: json_set(&observation_ids, "source_observations")?,
        known_facts_json: json_set(&known_facts, "known_facts")?,
        rejected_hypotheses_json: json_set(&rejected_hypotheses, "rejected_hypotheses")?,
        open_questions_json: json_set(&open_questions, "open_questions")?,
        evidence_json: json_set(&evidence, "evidence")?,
        evidence_count: observations.len() as i64,
        confidence,
        evidence_alpha,
        evidence_beta,
        status: CandidateStatus::Candidate,
        last_recalled_at: None,
        recall_count: 0,
        successful_recall_count: 0,
        failed_recall_count: 0,
        created_at: now.clone(),
        updated_at: now,
    })
}

fn candidate_terms(candidate: &ConceptCandidate) -> BTreeSet<String> {
    let mut terms =
        parse_json_set(&candidate.source_terms_json, "source_terms").unwrap_or_default();
    if terms.is_empty() {
        insert_non_empty(&mut terms, &candidate.name);
    }
    terms
}

fn alias_names(candidates: &[&ConceptCandidate]) -> Vec<String> {
    let mut aliases = BTreeSet::new();
    for candidate in candidates {
        insert_non_empty(&mut aliases, &candidate.name);
    }
    aliases.into_iter().collect()
}

fn filter_text_by_terms(values: &[String], terms: &BTreeSet<String>) -> BTreeSet<String> {
    values
        .iter()
        .filter(|value| terms.iter().any(|term| value.contains(term)))
        .cloned()
        .collect()
}

fn parse_json_vec(json: &Option<String>, field: &str) -> MemoryResult<Vec<String>> {
    let Some(raw) = json.as_deref() else {
        return Ok(Vec::new());
    };
    serde_json::from_str(raw)
        .map_err(|e| MemoryError::Clustering(format!("parse {field} json: {e}")))
}

fn parse_json_set(json: &Option<String>, field: &str) -> MemoryResult<BTreeSet<String>> {
    Ok(parse_json_vec(json, field)?.into_iter().collect())
}

fn json_set(values: &BTreeSet<String>, field: &str) -> MemoryResult<Option<String>> {
    if values.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(values)
        .map(Some)
        .map_err(|e| MemoryError::Clustering(format!("serialize {field}: {e}")))
}

fn confidence_from_evidence(alpha: f64, beta: f64) -> f64 {
    if alpha + beta > 0.0 {
        alpha / (alpha + beta)
    } else {
        0.0
    }
}

fn insert_non_empty(values: &mut BTreeSet<String>, value: &str) {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        values.insert(trimmed.to_string());
    }
}

fn jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count() as f64;
    let union = left.union(right).count() as f64;
    intersection / union
}

fn find(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = find(parents, parents[index]);
    }
    parents[index]
}

fn union(parents: &mut [usize], left: usize, right: usize) {
    let left_root = find(parents, left);
    let right_root = find(parents, right);
    parents[right_root] = left_root;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::observation::ObservationSourceType;
    use crate::models::status::ObservationStatus;

    fn candidate(
        id: &str,
        name: &str,
        terms: &[&str],
        observations: &[&str],
        facts: &[&str],
    ) -> ConceptCandidate {
        ConceptCandidate {
            candidate_id: id.to_string(),
            workspace_id: "ws".to_string(),
            name: name.to_string(),
            summary: None,
            source_terms_json: Some(serde_json::to_string(terms).unwrap()),
            source_sessions_json: Some(serde_json::to_string(&[format!("session_{id}")]).unwrap()),
            source_observations_json: Some(serde_json::to_string(observations).unwrap()),
            known_facts_json: Some(serde_json::to_string(facts).unwrap()),
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            evidence_count: observations.len() as i64,
            confidence: 0.5,
            evidence_alpha: observations.len() as f64,
            evidence_beta: 1.0,
            status: CandidateStatus::Candidate,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            created_at: "2026-06-14T00:00:00Z".to_string(),
            updated_at: "2026-06-14T00:00:00Z".to_string(),
        }
    }

    fn obs(id: &str, subject: &str, object: Option<&str>) -> Observation {
        Observation {
            observation_id: id.to_string(),
            workspace_id: "ws".to_string(),
            memory_id: format!("mem_{id}"),
            subject_text: subject.to_string(),
            subject_type: None,
            predicate: "related_to".to_string(),
            object_text: object.map(str::to_string),
            object_type: None,
            evidence_text: None,
            extraction_confidence: 0.8,
            evidence_alpha: 3.0,
            evidence_beta: 1.0,
            status: ObservationStatus::Candidate,
            surprise_score: 0.0,
            source_type: ObservationSourceType::UserMessage,
            consolidated: false,
            memory_type_candidate: None,
            observation_detail_json: None,
            extraction_batch_id: None,
            superseded_by: None,
            created_at: "2026-06-14T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn merges_overlapping_candidates_and_preserves_aliases() {
        let engine = MergeSplitEngine::new(MergeSplitConfig {
            entity_jaccard_threshold: 0.30,
            broad_candidate_min_observations: 4,
        });
        let left = candidate(
            "c1",
            "POSMASK machine investigation",
            &["POSMASK", "machine"],
            &["o1"],
            &["POSMASK lacks machine field"],
        );
        let right = candidate(
            "c2",
            "POSMASK field investigation",
            &["POSMASK", "field"],
            &["o2"],
            &["POSMASK uses field mapping"],
        );
        let unrelated = candidate("c3", "BOE DNS", &["BOE", "DNS"], &["o3"], &[]);

        let result = engine
            .merge_overlapping_candidates(&[left, right, unrelated])
            .unwrap();

        assert_eq!(result.candidates.len(), 2);
        let merged = result
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_id == "c1")
            .unwrap();
        let terms: BTreeSet<String> =
            serde_json::from_str(merged.source_terms_json.as_ref().unwrap()).unwrap();
        let facts: BTreeSet<String> =
            serde_json::from_str(merged.known_facts_json.as_ref().unwrap()).unwrap();
        assert_eq!(terms.len(), 3);
        assert!(terms.contains("machine"));
        assert!(terms.contains("field"));
        assert_eq!(facts.len(), 2);
        assert_eq!(merged.evidence_count, 2);
        assert_eq!(result.aliases.len(), 1);
        assert_eq!(result.aliases[0].candidate_id, "c1");
        assert_eq!(result.aliases[0].aliases.len(), 2);
    }

    #[test]
    fn splits_broad_candidate_by_subcluster_observations() {
        let engine = MergeSplitEngine::new(MergeSplitConfig {
            entity_jaccard_threshold: 0.60,
            broad_candidate_min_observations: 4,
        });
        let broad = candidate(
            "wide",
            "mixed troubleshooting",
            &["POSMASK", "machine", "BOE", "DNS"],
            &["o1", "o2", "o3", "o4"],
            &[
                "POSMASK lacks machine field",
                "BOE DNS needs internal resolver",
                "unmatched fact is not copied",
            ],
        );
        let clusters = vec![
            ObservationCluster {
                observations: vec![
                    obs("o1", "POSMASK", Some("machine")),
                    obs("o2", "POSMASK", Some("field")),
                ],
                centroid: vec![1.0, 0.0],
                average_confidence: 0.6,
            },
            ObservationCluster {
                observations: vec![obs("o3", "BOE", Some("DNS")), obs("o4", "DNS", None)],
                centroid: vec![0.0, 1.0],
                average_confidence: 0.6,
            },
        ];

        let split = engine.split_broad_candidate(&broad, &clusters).unwrap();

        assert_eq!(split.len(), 2);
        assert!(split.iter().all(|candidate| candidate.evidence_count == 2));
        let posmask = split
            .iter()
            .find(|candidate| {
                candidate
                    .source_terms_json
                    .as_ref()
                    .unwrap()
                    .contains("POSMASK")
            })
            .unwrap();
        let posmask_facts: BTreeSet<String> =
            serde_json::from_str(posmask.known_facts_json.as_ref().unwrap()).unwrap();
        assert_eq!(posmask_facts.len(), 1);
        assert!(posmask_facts.contains("POSMASK lacks machine field"));
        let dns = split
            .iter()
            .find(|candidate| {
                candidate
                    .source_terms_json
                    .as_ref()
                    .unwrap()
                    .contains("DNS")
            })
            .unwrap();
        let dns_facts: BTreeSet<String> =
            serde_json::from_str(dns.known_facts_json.as_ref().unwrap()).unwrap();
        assert_eq!(dns_facts.len(), 1);
        assert!(dns_facts.contains("BOE DNS needs internal resolver"));
    }
}
