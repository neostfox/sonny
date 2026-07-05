use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use uuid::Uuid;

use crate::config::ClusterConfig;
use crate::error::{MemoryError, MemoryResult};
use crate::models::concept::ConceptCandidate;
use crate::models::embedding::EmbeddingSourceType;
use crate::models::observation::Observation;
use crate::models::status::CandidateStatus;
use crate::store::traits::{EmbeddingStore, ObservationStore};

#[derive(Debug, Clone)]
pub struct ClusterEngine {
    config: ClusterConfig,
}

#[derive(Debug, Clone)]
pub struct ClusteredObservation {
    pub observation: Observation,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct ObservationCluster {
    pub observations: Vec<Observation>,
    pub centroid: Vec<f64>,
    pub average_confidence: f64,
}

impl ClusterEngine {
    pub fn new(config: ClusterConfig) -> Self {
        Self { config }
    }

    pub fn cluster_workspace<O, E>(
        &self,
        workspace_id: &str,
        observation_store: &O,
        embedding_store: &E,
    ) -> MemoryResult<Vec<ObservationCluster>>
    where
        O: ObservationStore,
        E: EmbeddingStore,
    {
        let observations = observation_store.list_by_workspace(workspace_id, None)?;
        let mut items = Vec::with_capacity(observations.len().min(self.config.incremental_max));

        for observation in observations.into_iter().take(self.config.incremental_max) {
            let Some(embedding) = embedding_store.get_embedding(
                EmbeddingSourceType::Observation.as_str(),
                &observation.observation_id,
            )?
            else {
                continue;
            };
            items.push(ClusteredObservation {
                observation,
                embedding,
            });
        }

        self.cluster_items(&items, observation_store)
    }

    pub fn cluster_items<O>(
        &self,
        items: &[ClusteredObservation],
        observation_store: &O,
    ) -> MemoryResult<Vec<ObservationCluster>>
    where
        O: ObservationStore,
    {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        validate_dimensions(items)?;

        let mut parents: Vec<usize> = (0..items.len()).collect();
        for left in 0..items.len() {
            for right in (left + 1)..items.len() {
                let distance = combined_distance(
                    &items[left].observation,
                    &items[right].observation,
                    cosine_distance(&items[left].embedding, &items[right].embedding),
                );
                if distance <= self.config.hac_threshold
                    || self.has_coclaim_edge(
                        &items[left].observation,
                        &items[right].observation,
                        observation_store,
                    )?
                {
                    union(&mut parents, left, right);
                }
            }
        }

        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for index in 0..items.len() {
            groups
                .entry(find(&mut parents, index))
                .or_default()
                .push(index);
        }

        let mut clusters = Vec::with_capacity(groups.len());
        for group in groups.values() {
            clusters.push(build_cluster(items, group)?);
        }
        clusters.sort_by(|a, b| {
            b.observations
                .len()
                .cmp(&a.observations.len())
                .then_with(|| b.average_confidence.total_cmp(&a.average_confidence))
        });

        Ok(self.merge_entity_clusters(clusters))
    }

    pub fn build_candidates(
        &self,
        clusters: &[ObservationCluster],
    ) -> MemoryResult<Vec<ConceptCandidate>> {
        let mut candidates = Vec::with_capacity(clusters.len());
        for cluster in clusters {
            if cluster.observations.is_empty() {
                continue;
            }
            candidates.push(candidate_from_cluster(cluster)?);
        }
        Ok(candidates)
    }

    fn has_coclaim_edge<O>(
        &self,
        left: &Observation,
        right: &Observation,
        observation_store: &O,
    ) -> MemoryResult<bool>
    where
        O: ObservationStore,
    {
        if left.extraction_batch_id.is_none()
            || left.extraction_batch_id != right.extraction_batch_id
        {
            return Ok(false);
        }
        let siblings = observation_store.find_coclaim(&left.observation_id)?;
        Ok(siblings
            .iter()
            .any(|sibling| sibling.observation_id == right.observation_id))
    }

    fn merge_entity_clusters(&self, clusters: Vec<ObservationCluster>) -> Vec<ObservationCluster> {
        let mut out: Vec<ObservationCluster> = Vec::new();
        'cluster: for cluster in clusters {
            for existing in &mut out {
                if entity_jaccard_cluster(existing, &cluster) > 0.0
                    && cosine_distance(&existing.centroid, &cluster.centroid)
                        <= self.config.entity_merge_threshold
                {
                    merge_cluster(existing, cluster);
                    continue 'cluster;
                }
            }
            out.push(cluster);
        }
        out
    }
}

impl Default for ClusterEngine {
    fn default() -> Self {
        Self::new(ClusterConfig {
            hac_threshold: 0.25,
            entity_merge_threshold: 0.40,
            incremental_max: 5000,
        })
    }
}

pub fn combined_distance(left: &Observation, right: &Observation, embedding_distance: f64) -> f64 {
    let entity_overlap = entity_jaccard_observation(left, right);
    if entity_overlap > 0.0 {
        embedding_distance * (1.0 - 0.5 * entity_overlap)
    } else {
        embedding_distance
    }
}

fn validate_dimensions(items: &[ClusteredObservation]) -> MemoryResult<()> {
    let Some(first) = items.first() else {
        return Ok(());
    };
    let dim = first.embedding.len();
    if dim == 0 {
        return Err(MemoryError::Clustering(
            "embedding vector must not be empty".to_string(),
        ));
    }
    if items.iter().any(|item| item.embedding.len() != dim) {
        return Err(MemoryError::Clustering(
            "embedding vectors must have equal dimensions".to_string(),
        ));
    }
    Ok(())
}

fn build_cluster(
    items: &[ClusteredObservation],
    indexes: &[usize],
) -> MemoryResult<ObservationCluster> {
    let dim = items[indexes[0]].embedding.len();
    let mut centroid = vec![0.0; dim];
    let mut observations = Vec::with_capacity(indexes.len());
    let mut confidence_sum = 0.0;

    for &index in indexes {
        let item = &items[index];
        for (out, value) in centroid.iter_mut().zip(item.embedding.iter()) {
            *out += f64::from(*value);
        }
        confidence_sum += item.observation.effective_confidence();
        observations.push(item.observation.clone());
    }

    let count = indexes.len() as f64;
    for value in &mut centroid {
        *value /= count;
    }

    Ok(ObservationCluster {
        observations,
        centroid,
        average_confidence: confidence_sum / count,
    })
}

fn candidate_from_cluster(cluster: &ObservationCluster) -> MemoryResult<ConceptCandidate> {
    let now = Utc::now().to_rfc3339();
    let source_terms: Vec<String> = cluster_entities(cluster).into_iter().collect();
    let source_observations: Vec<&str> = cluster
        .observations
        .iter()
        .map(|observation| observation.observation_id.as_str())
        .collect();
    let source_sessions: Vec<&str> = cluster
        .observations
        .iter()
        .map(|observation| observation.memory_id.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let evidence_alpha: f64 = cluster
        .observations
        .iter()
        .map(|observation| observation.evidence_alpha)
        .sum();
    let evidence_beta: f64 = cluster
        .observations
        .iter()
        .map(|observation| observation.evidence_beta)
        .sum();
    let confidence = if evidence_alpha + evidence_beta > 0.0 {
        evidence_alpha / (evidence_alpha + evidence_beta)
    } else {
        0.0
    };

    Ok(ConceptCandidate {
        candidate_id: Uuid::new_v4().to_string(),
        workspace_id: cluster.observations[0].workspace_id.clone(),
        name: source_terms
            .first()
            .cloned()
            .unwrap_or_else(|| "unnamed cluster".to_string()),
        summary: None,
        source_terms_json: Some(
            serde_json::to_string(&source_terms)
                .map_err(|e| MemoryError::Clustering(format!("serialize source terms: {e}")))?,
        ),
        source_sessions_json: Some(
            serde_json::to_string(&source_sessions)
                .map_err(|e| MemoryError::Clustering(format!("serialize source sessions: {e}")))?,
        ),
        source_observations_json: Some(
            serde_json::to_string(&source_observations).map_err(|e| {
                MemoryError::Clustering(format!("serialize source observations: {e}"))
            })?,
        ),
        known_facts_json: None,
        rejected_hypotheses_json: None,
        open_questions_json: None,
        evidence_json: None,
        evidence_count: cluster.observations.len() as i64,
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

fn merge_cluster(target: &mut ObservationCluster, source: ObservationCluster) {
    let target_count = target.observations.len() as f64;
    let source_count = source.observations.len() as f64;
    for (target_value, source_value) in target.centroid.iter_mut().zip(source.centroid.iter()) {
        *target_value = ((*target_value * target_count) + (*source_value * source_count))
            / (target_count + source_count);
    }
    target.observations.extend(source.observations);
    let total_count = target_count + source_count;
    let confidence_sum: f64 = target
        .observations
        .iter()
        .map(|observation| observation.effective_confidence())
        .sum();
    target.average_confidence = confidence_sum / total_count;
}

fn entity_jaccard_observation(left: &Observation, right: &Observation) -> f64 {
    let left_entities = observation_entities(left);
    let right_entities = observation_entities(right);
    jaccard(&left_entities, &right_entities)
}

fn entity_jaccard_cluster(left: &ObservationCluster, right: &ObservationCluster) -> f64 {
    let left_entities = cluster_entities(left);
    let right_entities = cluster_entities(right);
    jaccard(&left_entities, &right_entities)
}

fn observation_entities(observation: &Observation) -> BTreeSet<String> {
    let mut entities = BTreeSet::new();
    insert_non_empty(&mut entities, &observation.subject_text);
    if let Some(object) = &observation.object_text {
        insert_non_empty(&mut entities, object);
    }
    entities
}

fn cluster_entities(cluster: &ObservationCluster) -> BTreeSet<String> {
    let mut entities = BTreeSet::new();
    for observation in &cluster.observations {
        insert_non_empty(&mut entities, &observation.subject_text);
        if let Some(object) = &observation.object_text {
            insert_non_empty(&mut entities, object);
        }
    }
    entities
}

fn insert_non_empty(entities: &mut BTreeSet<String>, value: &str) {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        entities.insert(trimmed.to_string());
    }
}

fn jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    let intersection = left.intersection(right).count() as f64;
    let union = left.union(right).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

trait CosineValue: Copy {
    fn to_f64(self) -> f64;
}

impl CosineValue for f32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl CosineValue for f64 {
    fn to_f64(self) -> f64 {
        self
    }
}

fn cosine_distance<T, U>(left: &[T], right: &[U]) -> f64
where
    T: CosineValue,
    U: CosineValue,
{
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (&left_value, &right_value) in left.iter().zip(right.iter()) {
        let left_value = left_value.to_f64();
        let right_value = right_value.to_f64();
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 1.0;
    }
    1.0 - (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0)
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
    if left_root != right_root {
        parents[right_root] = left_root;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::observation::ObservationSourceType;
    use crate::models::status::ObservationStatus;

    struct EmptyObservationStore;

    impl crate::store::traits::ObservationStore for EmptyObservationStore {
        fn insert(&self, _obs: &Observation) -> MemoryResult<()> {
            unreachable!()
        }

        fn insert_batch(&self, _observations: &[Observation]) -> MemoryResult<()> {
            unreachable!()
        }

        fn get(&self, _observation_id: &str) -> MemoryResult<Option<Observation>> {
            unreachable!()
        }

        fn list_by_workspace(
            &self,
            _workspace_id: &str,
            _status: Option<ObservationStatus>,
        ) -> MemoryResult<Vec<Observation>> {
            unreachable!()
        }

        fn update_status(
            &self,
            _observation_id: &str,
            _status: ObservationStatus,
        ) -> MemoryResult<()> {
            unreachable!()
        }

        fn update_confidence(
            &self,
            _observation_id: &str,
            _alpha: f64,
            _beta: f64,
        ) -> MemoryResult<()> {
            unreachable!()
        }

        fn find_by_entity(
            &self,
            _entity: &str,
            _workspace_id: &str,
        ) -> MemoryResult<Vec<Observation>> {
            unreachable!()
        }

        fn find_coclaim(&self, _observation_id: &str) -> MemoryResult<Vec<Observation>> {
            Ok(Vec::new())
        }

        fn supersede(&self, _observation_id: &str, _superseded_by: &str) -> MemoryResult<()> {
            unreachable!()
        }

        fn find_duplicate(
            &self,
            _subject: &str,
            _predicate: &str,
            _object: Option<&str>,
            _workspace_id: &str,
        ) -> MemoryResult<Option<Observation>> {
            unreachable!()
        }

        fn replace_session_observations(
            &self,
            _session_id: &str,
            _new_observations: &[Observation],
        ) -> MemoryResult<usize> {
            unreachable!()
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

    fn item(observation: Observation, embedding: &[f32]) -> ClusteredObservation {
        ClusteredObservation {
            observation,
            embedding: embedding.to_vec(),
        }
    }

    #[test]
    fn combined_distance_rewards_shared_entities() {
        let left = obs("a", "posmask", Some("machine"));
        let right = obs("b", "posmask", Some("field"));

        assert!(combined_distance(&left, &right, 0.30) < 0.30);
    }

    #[test]
    fn cluster_items_groups_close_embeddings() {
        let engine = ClusterEngine::new(ClusterConfig {
            hac_threshold: 0.10,
            entity_merge_threshold: 0.40,
            incremental_max: 100,
        });
        let items = vec![
            item(obs("a", "posmask", Some("machine")), &[1.0, 0.0]),
            item(obs("b", "posmask", Some("field")), &[0.98, 0.02]),
            item(obs("c", "dns", Some("boe")), &[0.0, 1.0]),
        ];

        let clusters = engine
            .cluster_items(&items, &EmptyObservationStore)
            .unwrap();

        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].observations.len(), 2);
        assert_eq!(clusters[1].observations.len(), 1);
    }

    #[test]
    fn build_candidates_carries_cluster_sources_and_confidence() {
        let engine = ClusterEngine::default();
        let cluster = ObservationCluster {
            observations: vec![
                obs("a", "posmask", Some("machine")),
                obs("b", "posmask", None),
            ],
            centroid: vec![1.0, 0.0],
            average_confidence: 0.6,
        };

        let candidates = engine.build_candidates(&[cluster]).unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].workspace_id, "ws");
        assert_eq!(candidates[0].evidence_count, 2);
        assert_eq!(candidates[0].status, CandidateStatus::Candidate);
        assert!(candidates[0]
            .source_observations_json
            .as_ref()
            .unwrap()
            .contains("a"));
        assert!((candidates[0].confidence - 0.75).abs() < f64::EPSILON);
    }
}
