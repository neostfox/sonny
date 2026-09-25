//! P13: Compact Search — hybrid multi-path retrieval over the concept graph.
//!
//! MindMemOS-inspired: compact means restricting traversal to task-relevant
//! entity / property / relational associations rather than exhaustive expansion.
//!
//! Pipeline (deterministic controller; an LLM planner can refine later):
//! 1. Hybrid seed: lexical (token match) ⊕ dense (embedding) fused by RRF.
//! 2. Forward expand: seed concepts → `concept_relation` neighbors.
//! 3. Reverse expand: seed concepts → other concepts sharing entities.
//! 4. Stop at max steps / top_k — never flood the graph.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::Utc;

use crate::embed::traits::EmbeddingProvider;
use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::concept::Concept;
use crate::models::embedding::EmbeddingSourceType;
use crate::models::hierarchy::RelationType;
use crate::models::recall::RecallScore;
use crate::models::status::ConceptStatus;
use crate::store::traits::{ConceptStore, EmbeddingStore, RelationStore};
use crate::text::keyword_hit;

/// Reciprocal Rank Fusion constant (Cormack et al.); 60 is the usual default.
pub const RRF_K: f64 = 60.0;
/// Default controller steps (seed + forward + reverse).
pub const DEFAULT_MAX_STEPS: usize = 3;
/// Max neighbors pulled per concept per relation type.
const NEIGHBOR_CAP: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    /// Entity → concept (find modules that mention query entities).
    Forward,
    /// Concept fact / property text → concept (find modules by what they claim).
    Reverse,
}

#[derive(Debug, Clone)]
pub struct CompactSearchHit {
    pub concept_id: String,
    pub score: f64,
    pub lexical_rank: Option<usize>,
    pub dense_rank: Option<usize>,
    pub via: Vec<String>,
}

/// Lightweight lexical scorer over concept text fields (name, definition, entities, facts).
/// Replaces a full BM25 index for personal-scale stores.
pub fn lexical_score(query: &str, concept: &Concept) -> f64 {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    if keyword_hit(&concept.name.to_lowercase(), &q) || concept.name.to_lowercase().contains(&q) {
        score += 3.0;
    }
    if let Some(def) = concept.definition.as_deref() {
        let d = def.to_lowercase();
        if d.contains(&q) {
            score += 2.0;
        }
        // Token overlap. Filter by char count so single CJK characters
        // (3 UTF-8 bytes) are not treated as multi-char tokens.
        let tokens: Vec<&str> = q.split_whitespace().collect();
        for t in tokens {
            if t.chars().count() >= 2 && d.contains(t) {
                score += 0.5;
            }
        }
    }
    if let Some(json) = concept.related_entities_json.as_deref() {
        let lower = json.to_lowercase();
        if lower.contains(&q) {
            score += 2.5;
        }
        for t in q.split_whitespace() {
            if t.chars().count() >= 2 && lower.contains(t) {
                score += 0.8;
            }
        }
    }
    if let Some(facts) = concept.known_facts_json.as_deref() {
        let lower = facts.to_lowercase();
        if lower.contains(&q) {
            score += 1.5;
        }
    }
    score
}

/// Reciprocal Rank Fusion of two ranked id lists.
pub fn rrf_fuse(
    lexical: &[(String, usize)],
    dense: &[(String, usize)],
) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for (id, rank) in lexical {
        *scores.entry(id.clone()).or_default() += 1.0 / (RRF_K + *rank as f64 + 1.0);
    }
    for (id, rank) in dense {
        *scores.entry(id.clone()).or_default() += 1.0 / (RRF_K + *rank as f64 + 1.0);
    }
    let mut out: Vec<(String, f64)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

fn concept_entities(concept: &Concept) -> Vec<String> {
    concept
        .related_entities_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|e| canonical_key_light(&e))
        .collect()
}

fn is_recallable(status: ConceptStatus) -> bool {
    matches!(status, ConceptStatus::Active | ConceptStatus::Disputed)
}

/// Seed ranking: lexical top-k fused with dense top-k via RRF.
async fn hybrid_seed<C, E, P>(
    concepts: &C,
    embeddings: &E,
    provider: &P,
    workspace_id: &str,
    query: &str,
    top_k: usize,
    include_domain_keys: &[String],
    include_global: bool,
) -> MemoryResult<Vec<CompactSearchHit>>
where
    C: ConceptStore,
    E: EmbeddingStore,
    P: EmbeddingProvider,
{
    let candidates = concepts.list_visible_concepts(
        workspace_id,
        Some(ConceptStatus::Active),
        include_domain_keys,
        include_global,
    )?;

    // Lexical ranking over visible active concepts.
    let mut lex: Vec<(String, f64)> = Vec::new();
    for c in &candidates {
        let s = lexical_score(query, c);
        if s > 0.0 {
            lex.push((c.concept_id.clone(), s));
        }
    }
    lex.sort_by(|a, b| b.1.total_cmp(&a.1));
    let lex_ranked: Vec<(String, usize)> = lex
        .into_iter()
        .take(top_k)
        .enumerate()
        .map(|(i, (id, _))| (id, i))
        .collect();

    // Dense ranking.
    let mut dense_ranked: Vec<(String, usize)> = Vec::new();
    let qvec = provider.embed(query).await?;
    let qs = qvec.as_slice().unwrap_or(&[]);
    if !qs.is_empty() {
        let mut hits = embeddings.search_including_elevated(
            qs,
            workspace_id,
            top_k,
            0.0,
            include_domain_keys,
            include_global,
        )?;
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        dense_ranked = hits
            .into_iter()
            .filter(|h| h.source_type == EmbeddingSourceType::Concept)
            .enumerate()
            .map(|(i, h)| (h.source_id, i))
            .collect();
    }

    let fused = rrf_fuse(&lex_ranked, &dense_ranked);
    let mut hits = Vec::new();
    for (id, score) in fused.into_iter().take(top_k) {
        let lex_r = lex_ranked.iter().position(|(x, _)| x == &id);
        let den_r = dense_ranked.iter().position(|(x, _)| x == &id);
        hits.push(CompactSearchHit {
            concept_id: id,
            score,
            lexical_rank: lex_r,
            dense_rank: den_r,
            via: vec!["hybrid_seed".into()],
        });
    }
    Ok(hits)
}

/// Forward: expand seeds along causal/shared_entity/shared_session edges.
fn forward_expand<C, R>(
    concepts: &C,
    relations: &R,
    workspace_id: &str,
    seeds: &[String],
    include_domain_keys: &[String],
    include_global: bool,
    decay: f64,
) -> MemoryResult<Vec<CompactSearchHit>>
where
    C: ConceptStore,
    R: RelationStore,
{
    let mut hits: HashMap<String, CompactSearchHit> = HashMap::new();
    for seed in seeds {
        for edge in relations.neighbors(workspace_id, seed)?.into_iter().take(NEIGHBOR_CAP) {
            // Prefer directed causal / undirected associative edges alike.
            let neighbor = if edge.src_concept_id == *seed {
                edge.dst_concept_id.clone()
            } else {
                edge.src_concept_id.clone()
            };
            if seeds.contains(&neighbor) {
                continue;
            }
            let Some(concept) = concepts.get_concept(&neighbor)? else {
                continue;
            };
            if concept.workspace_id != workspace_id
                && !crate::models::scope::is_visible(
                    &concept,
                    workspace_id,
                    include_domain_keys,
                    include_global,
                )
            {
                continue;
            }
            if !is_recallable(concept.status) {
                continue;
            }
            let w = edge.weight() * decay;
            hits.entry(neighbor.clone())
                .and_modify(|h| {
                    h.score += w;
                    if !h.via.contains(&edge.relation_type.as_str().to_string()) {
                        h.via.push(edge.relation_type.as_str().to_string());
                    }
                })
                .or_insert_with(|| CompactSearchHit {
                    concept_id: neighbor,
                    score: w,
                    lexical_rank: None,
                    dense_rank: None,
                    via: vec![format!("forward:{}", edge.relation_type.as_str())],
                });
        }
    }
    let mut out: Vec<_> = hits.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(out)
}

/// Reverse: expand via shared entities (property/fact → entity → other concepts).
fn reverse_expand<C>(
    concepts: &C,
    workspace_id: &str,
    seeds: &[String],
    include_domain_keys: &[String],
    include_global: bool,
    decay: f64,
) -> MemoryResult<Vec<CompactSearchHit>>
where
    C: ConceptStore,
{
    let mut entity_owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut seed_entities: BTreeSet<String> = BTreeSet::new();
    for seed in seeds {
        let Some(c) = concepts.get_concept(seed)? else {
            continue;
        };
        for e in concept_entities(&c) {
            seed_entities.insert(e.clone());
            entity_owners.entry(e).or_default().insert(seed.clone());
        }
    }

    // Concepts sharing those entities in the visible set.
    let mut hits: HashMap<String, CompactSearchHit> = HashMap::new();
    for e in &seed_entities {
        for concept in concepts.find_by_entities(&[e.clone()], workspace_id)? {
            if seeds.contains(&concept.concept_id) {
                continue;
            }
            hits.entry(concept.concept_id.clone())
                .and_modify(|h| h.score += decay)
                .or_insert_with(|| CompactSearchHit {
                    concept_id: concept.concept_id.clone(),
                    score: decay,
                    lexical_rank: None,
                    dense_rank: None,
                    via: vec![format!("reverse_entity:{e}")],
                });
        }
        // Elevated foreign concepts if requested.
        if include_global || !include_domain_keys.is_empty() {
            for concept in concepts.list_visible_concepts(
                workspace_id,
                Some(ConceptStatus::Active),
                include_domain_keys,
                include_global,
            )? {
                if concept.workspace_id == workspace_id || seeds.contains(&concept.concept_id) {
                    continue;
                }
                if concept_entities(&concept).contains(e) {
                    hits.entry(concept.concept_id.clone())
                        .and_modify(|h| h.score += decay * 0.8)
                        .or_insert_with(|| CompactSearchHit {
                            concept_id: concept.concept_id.clone(),
                            score: decay * 0.8,
                            lexical_rank: None,
                            dense_rank: None,
                            via: vec![format!("reverse_elevated:{e}")],
                        });
                }
            }
        }
    }
    let _ = entity_owners;
    let mut out: Vec<_> = hits.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(out)
}

fn merge_hits(base: &mut Vec<CompactSearchHit>, extra: Vec<CompactSearchHit>, weight: f64) {
    let mut index: HashMap<String, usize> = base
        .iter()
        .enumerate()
        .map(|(i, h)| (h.concept_id.clone(), i))
        .collect();
    for mut h in extra {
        h.score *= weight;
        if let Some(i) = index.get(&h.concept_id).copied() {
            base[i].score += h.score;
            for v in h.via {
                if !base[i].via.contains(&v) {
                    base[i].via.push(v);
                }
            }
        } else {
            index.insert(h.concept_id.clone(), base.len());
            base.push(h);
        }
    }
}

/// Compact search entry: multi-step hybrid + bidirectional expansion.
pub async fn compact_search<C, E, P, R>(
    concepts: &C,
    embeddings: &E,
    provider: &P,
    relations: &R,
    workspace_id: &str,
    query: &str,
    top_k: usize,
    max_steps: usize,
    include_domain_keys: &[String],
    include_global: bool,
) -> MemoryResult<Vec<CompactSearchHit>>
where
    C: ConceptStore,
    E: EmbeddingStore,
    P: EmbeddingProvider,
    R: RelationStore,
{
    let mut hits = hybrid_seed(
        concepts,
        embeddings,
        provider,
        workspace_id,
        query,
        top_k,
        include_domain_keys,
        include_global,
    )
    .await?;
    if hits.is_empty() || max_steps <= 1 {
        hits.truncate(top_k);
        return Ok(hits);
    }

    let seed_ids: Vec<String> = hits
        .iter()
        .take(5)
        .map(|h| h.concept_id.clone())
        .collect();
    // Down-weight expansions so hybrid seeds stay dominant.
    let mut step_weight = 0.45;
    for _step in 1..max_steps {
        let fwd = forward_expand(
            concepts,
            relations,
            workspace_id,
            &seed_ids,
            include_domain_keys,
            include_global,
            step_weight,
        )?;
        merge_hits(&mut hits, fwd, 1.0);
        let rev = reverse_expand(
            concepts,
            workspace_id,
            &seed_ids,
            include_domain_keys,
            include_global,
            step_weight * 0.75,
        )?;
        merge_hits(&mut hits, rev, 1.0);
        step_weight *= 0.5;
    }

    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.dedup_by(|a, b| a.concept_id == b.concept_id);
    hits.truncate(top_k);
    Ok(hits)
}

/// Convert compact hits into the RecallEngine score shape (for context building).
pub fn hits_to_recall_scores(hits: &[CompactSearchHit]) -> Vec<RecallScore> {
    hits.iter()
        .map(|h| RecallScore {
            concept_id: h.concept_id.clone(),
            score: h.score,
            semantic_score: h.dense_rank.map(|r| 1.0 / (r as f64 + 1.0)).unwrap_or(0.0),
            entity_score: h.lexical_rank.map(|r| 1.0 / (r as f64 + 1.0)).unwrap_or(0.0),
            recency: 1.0,
        })
        .collect()
}

/// Compact-search notes for MemoryContext (`via` provenance).
pub fn format_hit_provenance(hits: &[CompactSearchHit], limit: usize) -> Vec<String> {
    hits.iter()
        .take(limit)
        .map(|h| {
            format!(
                "{} (score {:.4}, via: {})",
                h.concept_id,
                h.score,
                h.via.join(", ")
            )
        })
        .collect()
}

/// Helper for tests / callers: now timestamp string.
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

/// Relation types considered strongest for forward expansion preference.
pub fn is_strong_edge(relation_type: RelationType) -> bool {
    matches!(
        relation_type,
        RelationType::Causal | RelationType::SharedEntity | RelationType::SharedSession
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_prefers_items_ranked_by_both_channels() {
        let lex = vec![("a".to_string(), 0), ("b".to_string(), 1)];
        let dense = vec![("b".to_string(), 0), ("c".to_string(), 1)];
        let fused = rrf_fuse(&lex, &dense);
        assert_eq!(fused[0].0, "b");
    }

    #[test]
    fn rrf_single_channel_still_present() {
        let lex = vec![("only-lex".to_string(), 0)];
        let dense = vec![];
        let fused = rrf_fuse(&lex, &dense);
        assert_eq!(fused.len(), 1);
    }

    #[test]
    fn lexical_scores_name_higher_than_definition() {
        let concept = Concept {
            concept_id: "c".into(),
            workspace_id: "ws".into(),
            name: "POSMASK 字段排查".into(),
            concept_type: None,
            definition: Some("排查 POSMASK 相关问题".into()),
            related_entities_json: Some(r#"["posmask"]"#.into()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: 0.8,
            evidence_alpha: 4.0,
            evidence_beta: 1.0,
            status: ConceptStatus::Active,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            connection_count: 0,
            lifecycle_scope: crate::models::scope::LifecycleScope::Project,
            scope_key: None,
            created_at: "t".into(),
            updated_at: "t".into(),
        };
        assert!(lexical_score("POSMASK", &concept) > 0.0);
    }

    fn concept_with_def(def: &str, entities_json: &str) -> Concept {
        Concept {
            concept_id: "c".into(),
            workspace_id: "ws".into(),
            name: "c".into(),
            concept_type: None,
            definition: Some(def.into()),
            related_entities_json: Some(entities_json.into()),
            known_facts_json: None,
            rejected_hypotheses_json: None,
            open_questions_json: None,
            evidence_json: None,
            confidence: 0.8,
            evidence_alpha: 4.0,
            evidence_beta: 1.0,
            status: ConceptStatus::Active,
            parent_concept_id: None,
            hierarchy_depth: 0,
            last_recalled_at: None,
            recall_count: 0,
            successful_recall_count: 0,
            failed_recall_count: 0,
            connection_count: 0,
            lifecycle_scope: crate::models::scope::LifecycleScope::Project,
            scope_key: None,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    /// H1 regression: token overlap must filter by char count, not UTF-8 bytes.
    #[test]
    fn lexical_token_overlap_skips_single_cjk_chars() {
        let c = concept_with_def("查询服务", "[]");
        // "查"/"服" are 1 char (3 bytes) — no token-overlap bonus; whole query
        // "查 服" is not a substring either, so the score stays 0.
        assert_eq!(lexical_score("查 服", &c), 0.0);
        // Multi-char token still earns definition overlap (contains 2.0 + token 0.5).
        assert!(lexical_score("查询", &c) >= 2.5);
    }
}
