//! P14 helpers: sync entity–property timeline from observations.

use crate::entity::canonical_key_light;
use crate::error::MemoryResult;
use crate::models::observation::Observation;
use crate::models::timeline::TimelineEntry;
use crate::store::traits::TimelineStore;

/// Record an observation as a timeline version for (subject, predicate).
/// Call after an observation is inserted / becomes live.
pub fn record_from_observation<T: TimelineStore>(
    timeline: &T,
    obs: &Observation,
    now: &str,
) -> MemoryResult<TimelineEntry> {
    let entity = canonical_key_light(&obs.subject_text);
    timeline.append_version(
        &obs.workspace_id,
        &entity,
        &obs.predicate,
        obs.object_text.as_deref(),
        Some(&obs.observation_id),
        now,
    )
}

/// Current value for (entity, property), if any.
pub fn current_value<T: TimelineStore>(
    timeline: &T,
    workspace_id: &str,
    entity: &str,
    property: &str,
) -> MemoryResult<Option<String>> {
    Ok(timeline
        .get_active(workspace_id, entity, property)?
        .and_then(|e| e.value))
}

/// Human-readable history lines (newest first).
pub fn format_history(entries: &[TimelineEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| {
            let mark = if e.is_current() { "●" } else { "○" };
            format!(
                "{mark} {}={} ({} → {})",
                e.property,
                e.value.as_deref().unwrap_or("?"),
                e.valid_from,
                e.valid_to.as_deref().unwrap_or("now")
            )
        })
        .collect()
}
