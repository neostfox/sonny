//! P8: structural analogy — fingerprint concepts as causal graphs and find
//! isomorphic peers across workspaces for transfer recall.
//!
//! Full VF2 isomorphism is overkill for personal-knowledge scale. We use a
//! Weisfeiler-Lehman-style neighborhood hash: entity nodes, predicate-labeled
//! directed edges, iterated `rounds` times. Two modules with the same
//! fingerprint share structure even when entity names differ (TODO.md 原则 3).

use std::collections::{BTreeMap, BTreeSet};

use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::concept::Concept;
use crate::models::hierarchy::RelationType;
use crate::models::observation::Observation;
use crate::models::relation::ConceptRelation;
use crate::models::status::ObservationStatus;
use crate::store::traits::{ConceptStore, ObservationStore, RelationStore};

/// Default WL iterations. 2–3 captures most module-local structure.
pub const DEFAULT_WL_ROUNDS: usize = 2;

/// Minimum fingerprint match confidence to treat two modules as structural peers.
pub const DEFAULT_PEER_SIMILARITY: f64 = 0.85;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StructuralEdge {
    pub src: String,
    pub dst: String,
    pub predicate: String,
    pub directed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConceptGraph {
    pub nodes: BTreeSet<String>,
    pub edges: Vec<StructuralEdge>,
}

impl ConceptGraph {
    pub fn from_parts(nodes: impl IntoIterator<Item = String>, edges: Vec<StructuralEdge>) -> Self {
        Self {
            nodes: nodes.into_iter().collect(),
            edges,
        }
    }

    /// Build the local graph for one concept: its entities as nodes and
    /// `causes` observations among (or touching) those entities.
    /// Concept-level relation placeholders are intentionally omitted — the
    /// fingerprint should capture module *internal* causal structure so two
    /// differently-named modules can match (TODO.md 原则 3).
    pub fn for_concept(
        concept: &Concept,
        observations: &[Observation],
        _relations: &[ConceptRelation],
        _entity_index: &BTreeMap<String, BTreeSet<String>>,
    ) -> Self {
        let entities: BTreeSet<String> = parse_entities(&concept.related_entities_json)
            .into_iter()
            .map(|e| canonical_key_light(&e))
            .collect();
        let mut edges = Vec::new();

        for obs in observations {
            if !is_live(&obs.status) || obs.predicate != "causes" {
                continue;
            }
            let s = canonical_key_light(&obs.subject_text);
            let Some(o) = obs.object_text.as_deref() else {
                continue;
            };
            let o = canonical_key_light(o);
            if entities.contains(&s) || entities.contains(&o) {
                edges.push(StructuralEdge {
                    src: s,
                    dst: o,
                    predicate: obs.predicate.clone(),
                    directed: true,
                });
            }
        }

        Self::from_parts(entities, edges)
    }

    /// Weisfeiler-Lehman neighborhood histogram fingerprint.
    pub fn fingerprint(&self, rounds: usize) -> String {
        let mut labels: BTreeMap<String, String> = self
            .nodes
            .iter()
            .map(|n| (n.clone(), n.clone())) // start from identity, then collapse
            .collect();
        // Collapse initial labels to degree+predicate multiset so names do not matter.
        let mut init: BTreeMap<String, (usize, BTreeMap<String, usize>)> = BTreeMap::new();
        for n in &self.nodes {
            init.entry(n.clone()).or_default();
        }
        for e in &self.edges {
            if let Some(s) = init.get_mut(&e.src) {
                s.0 += 1;
                *s.1.entry(format!("out:{}", e.predicate)).or_default() += 1;
            }
            if let Some(d) = init.get_mut(&e.dst) {
                d.0 += 1;
                *d.1.entry(format!("in:{}", e.predicate)).or_default() += 1;
            }
        }
        for (node, (deg, preds)) in &init {
            let mut s = format!("d{deg}");
            for (p, c) in preds {
                s.push('|');
                s.push_str(p);
                s.push(':');
                s.push_str(&c.to_string());
            }
            labels.insert(node.clone(), s);
        }

        for _ in 0..rounds {
            let mut next: BTreeMap<String, String> = BTreeMap::new();
            for n in &self.nodes {
                let mut neigh = Vec::new();
                for e in &self.edges {
                    if e.src == *n {
                        neigh.push(format!("o:{}>{}", e.predicate, labels.get(&e.dst).unwrap_or(&e.dst)));
                    }
                    if e.dst == *n {
                        neigh.push(format!("i:{}>{}", e.predicate, labels.get(&e.src).unwrap_or(&e.src)));
                    }
                }
                neigh.sort();
                let mut s = labels.get(n).cloned().unwrap_or_default();
                s.push('#');
                s.push_str(&neigh.join(","));
                // Hash by length+sample to keep labels short.
                next.insert(n.clone(), short_hash(&s));
            }
            labels = next;
        }

        let mut bag: BTreeMap<String, usize> = BTreeMap::new();
        for lab in labels.values() {
            *bag.entry(lab.clone()).or_default() += 1;
        }
        let mut parts: Vec<String> = bag
            .into_iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        parts.sort();
        format!(
            "n{}|e{}|{}",
            self.nodes.len(),
            self.edges.len(),
            parts.join(";")
        )
    }

    /// Cosine-like similarity of two fingerprints' label bags (0..1).
    pub fn similarity(a: &str, b: &str) -> f64 {
        let ba = fingerprint_bag(a);
        let bb = fingerprint_bag(b);
        if ba.is_empty() && bb.is_empty() {
            return 1.0;
        }
        if ba.is_empty() || bb.is_empty() {
            return 0.0;
        }
        let mut dot = 0.0;
        let mut na = 0.0;
        let mut nb = 0.0;
        for (k, va) in &ba {
            na += (*va as f64) * (*va as f64);
            if let Some(vb) = bb.get(k) {
                dot += (*va as f64) * (*vb as f64);
            }
        }
        for vb in bb.values() {
            nb += (*vb as f64) * (*vb as f64);
        }
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na.sqrt() * nb.sqrt())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuralPeer {
    pub concept_id: String,
    pub workspace_id: String,
    pub similarity: f64,
    /// Causal edges on the peer worth transferring.
    pub transferable_causal_edges: Vec<ConceptRelation>,
}

#[derive(Debug, Clone, Default)]
pub struct TransferHit {
    pub source_concept_id: String,
    pub source_workspace_id: String,
    pub similarity: f64,
    pub warnings: Vec<String>,
}

/// P8-D/E: given a concept in `workspace_id`, find structurally similar concepts
/// in other workspaces and harvest their confirmed/validated causal edges.
pub fn find_structural_peers<C, O, R>(
    concepts: &C,
    observations: &O,
    relations: &R,
    workspace_id: &str,
    concept_id: &str,
    min_similarity: f64,
    rounds: usize,
) -> MemoryResult<Vec<StructuralPeer>>
where
    C: ConceptStore,
    O: ObservationStore,
    R: RelationStore,
{
    let Some(target) = concepts.get_concept(concept_id)? else {
        return Ok(vec![]);
    };
    let live = observations.list_by_workspace(workspace_id, None)?;
    let live: Vec<Observation> = live
        .into_iter()
        .filter(|o| is_live(&o.status))
        .collect();
    let local_relations = relations.list_by_workspace(workspace_id)?;

    // Entity → concept ids in the local workspace (for for_concept).
    let mut entity_index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in concepts.list_concepts(workspace_id, None)? {
        for e in parse_entities(&c.related_entities_json) {
            entity_index
                .entry(canonical_key_light(&e))
                .or_default()
                .insert(c.concept_id.clone());
        }
    }

    let target_graph = ConceptGraph::for_concept(&target, &live, &local_relations, &entity_index);
    let target_fp = target_graph.fingerprint(rounds);

    // Scan elevated + all other workspaces via listing visible elevated concepts
    // and every other workspace's active concepts we can cheaply reach through
    // Domain/Global, plus peers from list_visible when global is on.
    // Practical path: iterate concepts in other workspaces by scanning Global/Domain
    // first, then project-scope peers via entity overlap fallback.
    let mut peers = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // Broad scan: all active concepts in other workspaces (P8-C/D).
    // Personal-knowledge scale; fingerprint filter does the real work.
    let mut candidates: Vec<Concept> = concepts.list_other_workspace_active_concepts(workspace_id)?;
    // Also include elevated concepts even if somehow inactive (defensive).
    for c in concepts.list_visible_concepts(workspace_id, None, &[], true)? {
        if c.workspace_id != workspace_id {
            candidates.push(c);
        }
    }

    for candidate in candidates {
        if !seen.insert(candidate.concept_id.clone()) {
            continue;
        }
        if candidate.workspace_id == workspace_id {
            continue;
        }
        let peer_obs = observations.list_by_workspace(&candidate.workspace_id, None)?;
        let peer_obs: Vec<Observation> = peer_obs.into_iter().filter(|o| is_live(&o.status)).collect();
        let peer_rel = relations.list_by_workspace(&candidate.workspace_id)?;
        let mut peer_index: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for c in concepts.list_concepts(&candidate.workspace_id, None)? {
            for e in parse_entities(&c.related_entities_json) {
                peer_index
                    .entry(canonical_key_light(&e))
                    .or_default()
                    .insert(c.concept_id.clone());
            }
        }
        let graph = ConceptGraph::for_concept(&candidate, &peer_obs, &peer_rel, &peer_index);
        let fp = graph.fingerprint(rounds);
        let sim = ConceptGraph::similarity(&target_fp, &fp);
        if sim >= min_similarity {
            let transferable: Vec<ConceptRelation> = peer_rel
                .into_iter()
                .filter(|r| {
                    r.relation_type == RelationType::Causal
                        && (r.src_concept_id == candidate.concept_id
                            || r.dst_concept_id == candidate.concept_id)
                        && matches!(
                            r.lifecycle,
                            crate::models::relation::RelationLifecycle::Validated
                                | crate::models::relation::RelationLifecycle::Confirmed
                        )
                })
                .collect();
            peers.push(StructuralPeer {
                concept_id: candidate.concept_id.clone(),
                workspace_id: candidate.workspace_id.clone(),
                similarity: sim,
                transferable_causal_edges: transferable,
            });
        }
    }

    peers.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    Ok(peers)
}

/// P8-E: transfer-mode recall notes — what other projects' structurally similar
/// modules warn about.
pub fn format_transfer_notes(peers: &[StructuralPeer], max_notes: usize) -> Vec<String> {
    let mut notes = Vec::new();
    for peer in peers.iter().take(max_notes) {
        if peer.transferable_causal_edges.is_empty() {
            notes.push(format!(
                "[{}] 结构相似度 {:.2}，但无已验证因果边可迁移",
                peer.workspace_id, peer.similarity
            ));
            continue;
        }
        for edge in peer.transferable_causal_edges.iter().take(3) {
            notes.push(format!(
                "[{} · sim {:.2}] 因果边 {} → {}（{:?}，weight {:.2}）",
                peer.workspace_id,
                peer.similarity,
                edge.src_concept_id,
                edge.dst_concept_id,
                edge.lifecycle,
                edge.weight()
            ));
        }
    }
    notes
}

fn parse_entities(json: &Option<String>) -> Vec<String> {
    json.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// P8-E entry: for each local concept id, harvest transfer notes from structural peers.
pub fn transfer_notes_for_concepts<C, O, R>(
    concepts: &C,
    observations: &O,
    relations: &R,
    workspace_id: &str,
    concept_ids: &[String],
    max_notes: usize,
) -> MemoryResult<Vec<String>>
where
    C: ConceptStore,
    O: ObservationStore,
    R: RelationStore,
{
    let mut notes = Vec::new();
    for id in concept_ids {
        let peers = find_structural_peers(
            concepts,
            observations,
            relations,
            workspace_id,
            id,
            DEFAULT_PEER_SIMILARITY,
            DEFAULT_WL_ROUNDS,
        )?;
        notes.extend(format_transfer_notes(&peers, max_notes));
        if notes.len() >= max_notes {
            notes.truncate(max_notes);
            break;
        }
    }
    Ok(notes)
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

fn short_hash(s: &str) -> String {
    // FNV-1a 64
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn fingerprint_bag(fp: &str) -> BTreeMap<String, i64> {
    // Skip the n/e prefix; bag the label:count parts.
    let mut bag = BTreeMap::new();
    for part in fp.split('|').skip(2) {
        for item in part.split(';') {
            if item.is_empty() {
                continue;
            }
            if let Some((k, v)) = item.rsplit_once(':') {
                if let Ok(n) = v.parse::<i64>() {
                    *bag.entry(k.to_string()).or_default() += n;
                }
            }
        }
    }
    bag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(src: &str, dst: &str, pred: &str) -> StructuralEdge {
        StructuralEdge {
            src: src.into(),
            dst: dst.into(),
            predicate: pred.into(),
            directed: true,
        }
    }

    #[test]
    fn isomorphic_structures_match_despite_names() {
        let a = ConceptGraph::from_parts(
            ["x".into(), "y".into(), "z".into()],
            vec![edge("x", "y", "causes"), edge("y", "z", "causes")],
        );
        let b = ConceptGraph::from_parts(
            ["p".into(), "q".into(), "r".into()],
            vec![edge("p", "q", "causes"), edge("q", "r", "causes")],
        );
        let fa = a.fingerprint(2);
        let fb = b.fingerprint(2);
        assert!(ConceptGraph::similarity(&fa, &fb) > 0.99);
    }

    #[test]
    fn different_structure_scores_lower() {
        let a = ConceptGraph::from_parts(
            ["x".into(), "y".into(), "z".into()],
            vec![edge("x", "y", "causes"), edge("y", "z", "causes")],
        );
        let b = ConceptGraph::from_parts(
            ["p".into(), "q".into()],
            vec![edge("p", "q", "causes")],
        );
        let fa = a.fingerprint(2);
        let fb = b.fingerprint(2);
        assert!(ConceptGraph::similarity(&fa, &fb) < 0.9);
    }
}
