//! Deterministic merging of context-source temporal entity responses.
//!
//! Every holder's history is the entity's history in the expanded form, so
//! two holders' parts of one entity are put together as the model puts
//! histories together: instances are united, one carrying an `instanceId`
//! already held counting once, and an auxiliary holder fills only the instants
//! no other holder has (clauses 4.3.6.2 and 5.7.3.4), which makes an entity
//! only auxiliary holders answered for theirs whole. Clauses 5.7.3.4 and
//! 5.7.4.4 merge a source's data into the set the temporal query has already
//! narrowed, so what a source answers outside the request's window is left
//! out as it joins, as the local read leaves out its own: an instance with no
//! instant on the requested Temporal Property (clause 4.5.7), one outside the
//! interval (clause 4.11), and of an aggregate a period not lying wholly in it.
//! An attribute left with nothing is removed, and under a temporal query a
//! source's entity left with no data at all joins nothing: "if the time
//! series, for all the concerned Attributes of an Entity, does not include
//! data corresponding to the temporal query interval, then such Entity [...]
//! shall not appear in the final result set".

use crate::outcome::ForwardedTemporalEntities;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    registration::forwarding_mode::ForwardingMode,
    temporal::{temporal_document::MergeMode, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::temporal::window::TemporalWindow;
use indexmap::IndexMap;

/// How a holder's history joins the one gathered: an auxiliary holder
/// supplements, every other holder contributes.
const fn merge_mode(mode: ForwardingMode) -> MergeMode {
    match mode {
        ForwardingMode::Auxiliary => MergeMode::Auxiliary,
        ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => MergeMode::Inclusive,
    }
}

/// Order `received` so every auxiliary answer follows the others: an
/// auxiliary instance joins only where no other holder has one (clauses 5.7.3.4
/// and 5.7.4.4), which is known only once every other holder has been merged.
/// The sort is stable, so each group keeps its answer order.
fn supplements_last(received: &mut [ForwardedTemporalEntities]) {
    received.sort_by_key(|answer| matches!(merge_mode(answer.mode), MergeMode::Auxiliary));
}

/// Narrow a source's `entity` to `window`, and say whether it still joins:
/// without a temporal query it always does, under one only while it holds
/// data in the window.
fn in_window(entity: &mut TemporalHistory, window: TemporalWindow) -> bool {
    entity.retain_window(window);
    !window.interval.is_bounded() || entity.holds_data()
}

/// Merge local and forwarded temporal query entities by entity identifier,
/// each source's part narrowed to `window`, the instances of each ordered by
/// its time property.
#[must_use]
pub fn merge_temporal_query_results(
    local_entities: Vec<TemporalHistory>,
    mut remote_results: Vec<ForwardedTemporalEntities>,
    window: TemporalWindow,
) -> Vec<TemporalHistory> {
    if remote_results.is_empty() {
        return local_entities;
    }
    supplements_last(&mut remote_results);

    let mut entity_map: IndexMap<Identifier, TemporalHistory> = IndexMap::new();
    for entity in local_entities {
        if let Some(id) = entity.id().cloned() {
            // clone: the map is keyed by the identifier the history names
            entity_map.insert(id, entity);
        }
    }

    for forwarded in remote_results {
        let mode = merge_mode(forwarded.mode);
        for mut remote_entity in forwarded.entities {
            if !in_window(&mut remote_entity, window) {
                continue;
            }
            let Some(remote_id) = remote_entity.id().cloned() else {
                continue; // clone: the map is keyed by the identifier the history names
            };
            match entity_map.get_mut(&remote_id) {
                Some(local_entity) => local_entity.merge(remote_entity, mode, window.timeproperty),
                None => {
                    entity_map.insert(remote_id, remote_entity);
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge remote temporal entity results for a single entity retrieval, each
/// source's part narrowed to `window`.
///
/// Returns the merged temporal entity or None if no entity found from any source.
#[must_use]
pub fn merge_temporal_retrieve_results(
    local_entity: Option<TemporalHistory>,
    mut remote_results: Vec<ForwardedTemporalEntities>,
    window: TemporalWindow,
) -> Option<TemporalHistory> {
    let mut result = local_entity;
    supplements_last(&mut remote_results);

    for forwarded in remote_results {
        let mode = merge_mode(forwarded.mode);
        for mut remote_entity in forwarded.entities {
            if !in_window(&mut remote_entity, window) {
                continue;
            }
            match &mut result {
                Some(existing) => existing.merge(remote_entity, mode, window.timeproperty),
                None => result = Some(remote_entity),
            }
        }
    }

    result
}

#[cfg(test)]
mod tests;
