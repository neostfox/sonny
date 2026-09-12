//! P6-C/E: promotion pipeline — lift Project concepts to Domain/Global when
//! independent workspaces corroborate them, and merge high-overlap peers.

use crate::error::MemoryResult;
use crate::models::scope::{
    apply_promotion, evaluate_promotion, LifecycleScope, PromotionEvidence, PromotionThresholds,
};
use crate::store::traits::{ConceptStore, ObservationStore};

/// Default entity-overlap required before two project concepts are treated as
/// the same knowledge for merge-on-promotion (P6-E).
pub const DEFAULT_MIN_ENTITY_OVERLAP: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionReport {
    pub concept_id: String,
    pub from: LifecycleScope,
    pub to: Option<LifecycleScope>,
    pub cross_project_count: i64,
    pub merged_alias: Option<String>,
}

/// Evaluate (and apply) promotion for one concept.
/// `cross_project_count` may be supplied by the caller after inspecting related
/// observations; when `None`, the store's max live `cross_project_count` among
/// the concept's entities' `causes` observations is not auto-derived (cheap path).
pub fn maybe_promote_concept<C: ConceptStore>(
    concepts: &C,
    concept_id: &str,
    cross_project_count: i64,
    conflict_count: i64,
    thresholds: &PromotionThresholds,
    domain_key: Option<&str>,
    now: &str,
) -> MemoryResult<PromotionReport> {
    let Some(mut concept) = concepts.get_concept(concept_id)? else {
        return Ok(PromotionReport {
            concept_id: concept_id.to_string(),
            from: LifecycleScope::Project,
            to: None,
            cross_project_count,
            merged_alias: None,
        });
    };
    let from = concept.lifecycle_scope;
    let evidence = PromotionEvidence::from_concept(&concept, cross_project_count, conflict_count);
    let target = evaluate_promotion(&evidence, thresholds);
    let mut merged_alias = None;

    if let Some(target) = target {
        let key = match target {
            LifecycleScope::Domain => domain_key.map(|s| s.to_string()),
            _ => None,
        };
        if apply_promotion(&mut concept, target, key) {
            concept.updated_at = now.to_string();
            concepts.update_concept(&concept)?;

            // P6-E: merge high-overlap peers in other workspaces into this primary.
            let entities = crate::store::concept_store::parse_entities(
                &concept.related_entities_json,
            );
            if !entities.is_empty() {
                for peer in concepts.find_cross_workspace_peers(
                    &concept.workspace_id,
                    &entities,
                    DEFAULT_MIN_ENTITY_OVERLAP,
                )? {
                    if peer.lifecycle_scope == LifecycleScope::Project {
                        concepts.link_alias(
                            &concept.concept_id,
                            &peer.concept_id,
                            "p6_promotion_peer_merge",
                        )?;
                        merged_alias = Some(peer.concept_id.clone());
                        break; // one alias per promotion is enough for the report
                    }
                }
            }
        }
        return Ok(PromotionReport {
            concept_id: concept_id.to_string(),
            from,
            to: Some(concept.lifecycle_scope),
            cross_project_count,
            merged_alias,
        });
    }

    Ok(PromotionReport {
        concept_id: concept_id.to_string(),
        from,
        to: None,
        cross_project_count,
        merged_alias,
    })
}

/// After observations for a triple are written, keep `cross_project_count`
/// coherent and try promoting any active project concepts that share the
/// triple's entities.
pub fn after_observation_write<O: ObservationStore, C: ConceptStore>(
    observations: &ObservationStoreWitness<'_, O>,
    concepts: &C,
    thresholds: &PromotionThresholds,
    domain_key: Option<&str>,
    now: &str,
) -> MemoryResult<Vec<PromotionReport>> {
    let mut reports = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for obs in observations.0.find_duplicate_any_workspace(
        &observations.1,
        &observations.2,
        observations.3.as_deref(),
    )? {
        if !seen.insert(obs.observation_id.clone()) {
            continue;
        }
        // Concepts that mention this observation's entities, in this workspace.
        let mut entities = vec![obs.subject_text.clone()];
        if let Some(o) = &obs.object_text {
            entities.push(o.clone());
        }
        for concept in concepts.find_by_entities(&entities, &obs.workspace_id)? {
            if concept.lifecycle_scope != LifecycleScope::Project {
                continue;
            }
            let report = maybe_promote_concept(
                concepts,
                &concept.concept_id,
                observations.0.sync_cross_project_count(
                    &observations.1,
                    &observations.2,
                    observations.3.as_deref(),
                )?,
                0,
                thresholds,
                domain_key,
                now,
            )?;
            reports.push(report);
        }
    }
    Ok(reports)
}

/// Helper so callers can pass a triple without cloning the store.
pub struct ObservationStoreWitness<'a, O: ObservationStore>(
    pub &'a O,
    pub String,
    pub String,
    pub Option<String>,
);
