use crate::error::MemoryResult;
use crate::models::concept::{Concept, ConceptCandidate};
use crate::models::embedding::EmbeddingSearchResult;
use crate::models::observation::Observation;
use crate::models::raw_memory::RawMemory;
use crate::models::status::{CandidateStatus, ConceptStatus, ObservationStatus};

pub trait RawMemoryStore: Send + Sync {
    fn insert(&self, raw: &RawMemory) -> MemoryResult<()>;
    fn insert_batch(&self, raws: &[RawMemory]) -> MemoryResult<()>;
    fn get_by_session(&self, session_id: &str) -> MemoryResult<Vec<RawMemory>>;
    fn list_by_workspace(&self, workspace_id: &str, limit: usize) -> MemoryResult<Vec<RawMemory>>;
    /// P2-D: prompt version of the last extraction over this session (`None` if never extracted).
    fn session_extraction_version(&self, session_id: &str) -> MemoryResult<Option<String>>;
    /// P2-D: stamp the prompt version onto every memory in a session after extraction.
    fn set_session_extraction_version(&self, session_id: &str, version: &str) -> MemoryResult<()>;
}

pub trait ObservationStore: Send + Sync {
    fn insert(&self, obs: &Observation) -> MemoryResult<()>;
    fn insert_batch(&self, observations: &[Observation]) -> MemoryResult<()>;
    fn get(&self, observation_id: &str) -> MemoryResult<Option<Observation>>;
    fn list_by_workspace(
        &self,
        workspace_id: &str,
        status: Option<ObservationStatus>,
    ) -> MemoryResult<Vec<Observation>>;
    fn update_status(&self, observation_id: &str, status: ObservationStatus) -> MemoryResult<()>;
    fn update_confidence(&self, observation_id: &str, alpha: f64, beta: f64) -> MemoryResult<()>;
    fn find_by_entity(&self, entity: &str, workspace_id: &str) -> MemoryResult<Vec<Observation>>;
    /// P2-C: observations co-claimed with `observation_id` in the same extraction batch.
    /// Returns siblings reachable via the `observation_coclaim` adjacency table.
    fn find_coclaim(&self, observation_id: &str) -> MemoryResult<Vec<Observation>>;
    /// Find an existing LIVE observation matching (subject, predicate, object) in the
    /// workspace. Used by dedup to decide whether a freshly extracted observation is a
    /// repeat — and to return the match so its evidence can be accumulated.
    /// Returns the oldest live match (excludes superseded/rejected/deprecated rows).
    fn find_duplicate(
        &self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        workspace_id: &str,
    ) -> MemoryResult<Option<Observation>>;
    /// P2-D: atomically supersede a session's live observations and insert the
    /// replacement batch in one transaction. `superseded_by` is derived from the new
    /// batch (NULL if empty). Returns the number of rows superseded.
    fn replace_session_observations(
        &self,
        session_id: &str,
        new_observations: &[Observation],
    ) -> MemoryResult<usize>;
}

pub trait ConceptStore: Send + Sync {
    fn insert_candidate(&self, candidate: &ConceptCandidate) -> MemoryResult<()>;
    fn get_candidate(&self, candidate_id: &str) -> MemoryResult<Option<ConceptCandidate>>;
    fn list_candidates(
        &self,
        workspace_id: &str,
        status: Option<CandidateStatus>,
    ) -> MemoryResult<Vec<ConceptCandidate>>;
    fn insert_concept(&self, concept: &Concept) -> MemoryResult<()>;
    fn get_concept(&self, concept_id: &str) -> MemoryResult<Option<Concept>>;
    fn list_concepts(
        &self,
        workspace_id: &str,
        status: Option<ConceptStatus>,
    ) -> MemoryResult<Vec<Concept>>;
    fn update_concept(&self, concept: &Concept) -> MemoryResult<()>;
    fn find_by_entities(
        &self,
        entities: &[String],
        workspace_id: &str,
    ) -> MemoryResult<Vec<Concept>>;
    fn update_recall_stats(&self, concept_id: &str, success: bool) -> MemoryResult<()>;
}

pub trait EmbeddingStore: Send + Sync {
    fn store_embedding(
        &self,
        source_type: &str,
        source_id: &str,
        workspace_id: &str,
        text: &str,
        vector: &[f32],
    ) -> MemoryResult<()>;
    fn search(
        &self,
        query_vector: &[f32],
        workspace_id: &str,
        top_k: usize,
        threshold: f32,
    ) -> MemoryResult<Vec<EmbeddingSearchResult>>;
    fn get_embedding(&self, source_type: &str, source_id: &str) -> MemoryResult<Option<Vec<f32>>>;
    fn delete(&self, source_type: &str, source_id: &str) -> MemoryResult<()>;
}
