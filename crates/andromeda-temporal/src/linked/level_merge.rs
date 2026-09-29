//! Folding one level of a temporal join traversal together: merging the
//! candidates two sources named, choosing which resolved entities the next
//! level walks, and fetching a candidate this broker does not hold from the
//! remainder of the system (clause 4.5.23.1).

use crate::{
    linked::{
        LinkedTemporalRead,
        ResolvedTemporalLevel,
        candidates::{RemoteEligibility, TemporalLinkedCandidate, identifier_of},
    },
    remote_linked::RemoteTemporalLinkedSource,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use indexmap::IndexSet;
use serde_json::Value;
use std::num::NonZeroU32;

/// Fold two candidate lists into one, keeping first-encounter order, dropping
/// anything already walked, and letting one annotated sighting make an
/// identifier remotely eligible (clause 4.5.23.1).
pub(super) fn merge_candidates(
    first: &[TemporalLinkedCandidate],
    second: Vec<TemporalLinkedCandidate>,
    visited: &IndexSet<Identifier>,
) -> Vec<TemporalLinkedCandidate> {
    let mut seen: IndexSet<Identifier> = IndexSet::new();
    let mut merged: Vec<TemporalLinkedCandidate> = Vec::new();
    for candidate in first.iter().cloned().chain(second) {
        // clone: the merged list owns its candidates, and `first` is borrowed
        if visited.contains(&candidate.id) {
            continue;
        }
        if let Some(index) = seen.get_index_of(&candidate.id) {
            if candidate.eligibility == RemoteEligibility::Eligible
                && let Some(existing) = merged.get_mut(index)
            {
                existing.eligibility = RemoteEligibility::Eligible;
            }
        } else {
            seen.insert(candidate.id.clone()); // clone: the duplicate guard owns the ids it records
            merged.push(candidate);
        }
    }
    merged
}

/// The level's entities whose own Relationships the next level walks.
///
/// A candidate this level resolved — locally, or by being fetched from a remote
/// broker — is a next-level root like any other. The extra entities that came
/// back around a fetched candidate are not: they are the remote broker's
/// already-resolved sub-tree, walked with the depth budget it was given, and
/// walking them again here would spend levels twice and reach further than the
/// client's `joinLevel` allows.
pub(super) fn expandable<'a>(candidates: &'a [TemporalLinkedCandidate], level: &'a ResolvedTemporalLevel) -> impl Iterator<Item = &'a Value> {
    candidates.iter().filter_map(|candidate| level.entities.get(&candidate.id))
}

/// Ask the remainder of the system for one candidate and fold what it answers
/// into the level it belongs to.
///
/// A source that cannot answer degrades the join rather than the request
/// (clause 4.5.23): the candidate stays unresolved, leaving the Relationship
/// that named it a plain URI, and the level carries a description of what is
/// missing back to the caller.
pub(super) async fn fetch_remote_candidate(
    source: &dyn RemoteTemporalLinkedSource,
    candidate: &TemporalLinkedCandidate,
    depth: NonZeroU32,
    visited: &IndexSet<Identifier>,
    linked_read: LinkedTemporalRead<'_>,
    level: &mut ResolvedTemporalLevel,
) {
    // The candidate itself is already in the guard; the remainder of the system
    // is told about everything except the entity it is being asked for.
    let held: Vec<Identifier> = visited.iter().filter(|id| **id != candidate.id).cloned().collect(); // clone: the port takes the ids as an owned slice
    match source
        .fetch_linked_temporal(&candidate.id, depth, &held, linked_read.window, linked_read.context)
        .await
    {
        Ok(fetched) => {
            for entity in fetched.entities {
                let Some(id) = identifier_of(&entity) else {
                    continue;
                };
                // The remote broker resolved the deeper levels in the same
                // round trip. Whatever it returns that the requester already
                // holds is dropped: clause 4.5.23 `containedBy` keeps those out
                // of the result.
                if id != candidate.id && visited.contains(&id) {
                    continue;
                }
                level.entities.entry(id).or_insert(entity);
            }
            for failure in &fetched.failures {
                tracing::warn!("Linked entity {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
            }
            level.failures.extend(fetched.failures);
        }
        Err(error) => {
            tracing::warn!(
                "Failed to resolve the temporal evolution of linked entity {} outside this broker: {}",
                candidate.id,
                error
            );
            level.failures.push(LinkedResolutionFailure {
                id: candidate.id.clone(), // clone: the failure outlives this level's candidates
                kind: LinkedResolutionFailureKind::Unreachable,
                detail: error.to_string(),
            });
        }
    }
}
