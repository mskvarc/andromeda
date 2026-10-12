//! Merging and classification of federated current-entity results.
//!
//! The merge runs on the model: every holder's version of an entity was read
//! under the context it was written in, so versions spelling one attribute
//! differently fold into one attribute, by expanded name (clause 4.5.5).

use crate::outcome::{ForwardedEntities, ForwardedTemporalEntities, ForwardedWriteResult};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_entity::operation::{transient_versions::differentiate_transient_versions, version_merge::MergePolicy};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use indexmap::IndexMap;

/// The clause 4.5.5 merge policy a registration mode calls for.
pub(crate) const fn policy_of(mode: ForwardingMode) -> MergePolicy {
    match mode {
        ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => MergePolicy::Instances,
        ForwardingMode::Auxiliary => MergePolicy::SupplementMissing,
    }
}

/// Fold `received` into `held`: the entity's versions merge on the model
/// (clause 4.5.5), the held version's entity-level system times stand, and
/// the Linked Entities each holder nested travel on together.
fn fold(held: &mut ReceivedState, received: ReceivedState, policy: MergePolicy) {
    let ReceivedState { state, linked, .. } = received;
    held.state.merge_version(state, policy);
    held.linked.extend(linked);
}

/// Differentiate the conflicting transient versions while the versions are
/// still separate (clause 4.5.5.2): the fold resolves the entity-level
/// `expiresAt` away, and after the first version is folded no value on the
/// base belongs to any one holder any more. An auxiliary holder is no party to
/// the conflict (clause 4.3.6.2).
fn differentiate(held: &mut [ReceivedState], received: &mut [ForwardedEntities]) {
    let remote = received
        .iter_mut()
        .filter(|answer| matches!(policy_of(answer.mode), MergePolicy::Instances))
        .flat_map(|answer| answer.entities.iter_mut());
    differentiate_transient_versions(held.iter_mut().chain(remote).map(|version| &mut version.state));
}

/// Merge local and remote entity query results (clause 5.7.2.4).
///
/// The local entities are the base. An entity another source also returned
/// has its attributes merged per clause 4.5.5; one only a source returned is
/// added, except from an auxiliary source, which only supplements entities
/// present already (clause 4.3.6.2).
#[must_use]
pub fn merge_query_results(mut local_entities: Vec<ReceivedState>, mut remote_results: Vec<ForwardedEntities>) -> Vec<ReceivedState> {
    if remote_results.is_empty() {
        return local_entities;
    }
    differentiate(&mut local_entities, &mut remote_results);
    let mut merged: IndexMap<Identifier, ReceivedState> = IndexMap::with_capacity(local_entities.len());
    for entity in local_entities {
        merged.insert(entity.state.id().clone(), entity); // clone: the index and entity each own the identifier
    }
    for forwarded in remote_results {
        let policy = policy_of(forwarded.mode);
        for remote_entity in forwarded.entities {
            match merged.get_mut(remote_entity.state.id()) {
                Some(local_entity) => fold(local_entity, remote_entity, policy),
                None => match policy {
                    MergePolicy::Instances => {
                        merged.insert(remote_entity.state.id().clone(), remote_entity); // clone: the index key and entity each own the identifier
                    }
                    MergePolicy::SupplementMissing => {}
                },
            }
        }
    }
    merged.into_values().collect()
}

/// Merge remote entity results for a single entity retrieval (clause 5.7.1.4).
///
/// Returns the merged entity, or `None` when no source held it.
#[must_use]
pub fn merge_retrieve_results(local_entity: Option<ReceivedState>, mut remote_results: Vec<ForwardedEntities>) -> Option<ReceivedState> {
    let mut result = local_entity;
    differentiate(result.as_mut_slice(), &mut remote_results);
    for forwarded in remote_results {
        let policy = policy_of(forwarded.mode);
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => fold(existing, remote_entity, policy),
                None => match policy {
                    MergePolicy::Instances => result = Some(remote_entity),
                    MergePolicy::SupplementMissing => {}
                },
            }
        }
    }
    result
}

/// Collect all warnings from forwarded entity results.
#[must_use]
pub fn collect_warnings(results: &[ForwardedEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect all warnings from forwarded temporal-entity results.
#[must_use]
pub fn collect_temporal_warnings(results: &[ForwardedTemporalEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// What each contacted source contributed to the `EntityMap` of a
/// current-entity read (clause 5.2.39), in result order.
pub fn contributions_of(results: &[ForwardedEntities]) -> impl Iterator<Item = SourceContribution> + '_ {
    results.iter().filter_map(ForwardedEntities::contribution)
}

/// What each contacted source contributed to the `EntityMap` of a temporal
/// read (clause 5.2.39), in result order.
pub fn temporal_contributions_of(results: &[ForwardedTemporalEntities]) -> impl Iterator<Item = SourceContribution> + '_ {
    results.iter().filter_map(ForwardedTemporalEntities::contribution)
}

/// The attributes every proxied registration in `results` takes out of local
/// processing, in result order.
#[must_use]
pub fn collect_proxied_attrs(results: &[ForwardedWriteResult]) -> Vec<AttributeName> {
    // A proxied registration owns its attributes whether or not the forward
    // succeeded (clause 4.3.6.3), so a failed or refused forward still keeps
    // them out of local processing.
    results.iter().flat_map(ForwardedWriteResult::proxied_attrs).cloned().collect()
}
