//! Deterministic merging of context-source temporal entity responses.
//!
//! Every holder's history is the entity's history in the expanded form, so
//! two holders' parts of one entity are put together as the model puts
//! histories together: instances are united, one carrying an `instanceId`
//! already held counting once, and an auxiliary holder fills only the instants
//! no other holder has (clauses 4.3.6.2 and 5.7.3.4). A source's instance that
//! carries no instant on the requested Temporal Property is left out, as the
//! local read leaves out its own (clause 4.5.7).

use crate::outcome::ForwardedTemporalEntities;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::{
    registration::forwarding_mode::ForwardingMode,
    temporal::{temporal_document::MergeMode, temporal_history::TemporalHistory},
};
use indexmap::IndexMap;

/// How a holder's history joins the one gathered: an auxiliary holder
/// supplements, every other holder contributes.
const fn merge_mode(mode: ForwardingMode) -> MergeMode {
    match mode {
        ForwardingMode::Auxiliary => MergeMode::Auxiliary,
        ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => MergeMode::Inclusive,
    }
}

/// Merge local and forwarded temporal query entities by entity identifier,
/// the instances of each ordered by `timeproperty`.
#[must_use]
pub fn merge_temporal_query_results(
    local_entities: Vec<TemporalHistory>,
    remote_results: Vec<ForwardedTemporalEntities>,
    timeproperty: TimeProp,
) -> Vec<TemporalHistory> {
    if remote_results.is_empty() {
        return local_entities;
    }

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
            remote_entity.retain_positioned(timeproperty);
            let Some(remote_id) = remote_entity.id().cloned() else {
                continue; // clone: the map is keyed by the identifier the history names
            };
            match (entity_map.get_mut(&remote_id), mode) {
                (Some(local_entity), _) => local_entity.merge(remote_entity, mode, timeproperty),
                // An auxiliary source only supplements what is already held.
                (None, MergeMode::Auxiliary) => {}
                (None, MergeMode::Inclusive) => {
                    entity_map.insert(remote_id, remote_entity);
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge remote temporal entity results for a single entity retrieval.
///
/// Returns the merged temporal entity or None if no entity found from any source.
#[must_use]
pub fn merge_temporal_retrieve_results(
    local_entity: Option<TemporalHistory>,
    remote_results: Vec<ForwardedTemporalEntities>,
    timeproperty: TimeProp,
) -> Option<TemporalHistory> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let mode = merge_mode(forwarded.mode);
        for mut remote_entity in forwarded.entities {
            remote_entity.retain_positioned(timeproperty);
            match (&mut result, mode) {
                (Some(existing), _) => existing.merge(remote_entity, mode, timeproperty),
                (None, MergeMode::Inclusive) => result = Some(remote_entity),
                (None, MergeMode::Auxiliary) => {}
            }
        }
    }

    result
}

#[cfg(test)]
mod tests;
