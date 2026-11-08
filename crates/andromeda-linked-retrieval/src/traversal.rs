//! The one clause 4.5.23 traversal every consumer runs.
//!
//! The walk is breadth-first: the entities the roots name are resolved first,
//! then the entities those name, one level at a time. Each level is resolved in
//! two halves: the local store answers for what it holds, and the remainder
//! of the distributed system answers for the `objectType`-annotated ids it does
//! not (clause 4.5.23.1).
//!
//! Clause 4.5.23.1 asks retrieval to "avoid cascades of an excessive length,
//! duplicates or loops". The visited guard does all three: every identifier is
//! asked for at most once, a root or a `containedBy` identifier is never asked
//! for, and the work is bounded by the number of reachable entities rather than
//! by the client's `joinLevel`.
//!
//! The order is fixed so the same request against the same data always renders
//! the same body: levels in order, and within one level the local store's
//! answers in candidate order before the remote answers in candidate order.

use crate::{
    candidate::{LinkedCandidate, candidates_of},
    edge::{LinkedEdge, RemoteEligibility},
    graph::{LinkedGraph, RootEdges},
    join::TraversalBounds,
    representation::LinkedRepresentation,
    source::{LevelRead, LinkedSource, NodeOf, RemoteReach, RemoteRead},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use indexmap::{IndexMap, IndexSet};
use std::{collections::HashSet, num::NonZeroU32};

/// One level of the traversal, as resolved.
struct Level<N> {
    /// The level's nodes: local in candidate order, then remote.
    nodes: IndexMap<Identifier, N>,
    /// Every edge those nodes name.
    edges: Vec<LinkedEdge>,
    /// What the level could not resolve.
    failures: Vec<LinkedResolutionFailure>,
}

/// Walk from `roots` to every Linked Entity within `bounds`.
///
/// The graph never holds a root, a `containedBy` identifier or the same
/// identifier twice. An identifier nothing backs is simply absent, which
/// leaves the Relationship naming it a plain URI (clause 4.5.23); a remote
/// source that was reachable but unhelpful is named among the failures.
///
/// # Errors
///
/// Returns the source's error when the local store cannot be read.
pub async fn traverse<S: LinkedSource>(source: &S, roots: RootEdges, bounds: &TraversalBounds) -> Result<LinkedGraph<NodeOf<S>>, S::Error> {
    let RootEdges { ids, edges } = roots;
    // The guard keeps insertion order because it doubles as the list of
    // already-held identifiers a remote fetch hands on.
    let mut visited: IndexSet<Identifier> = bounds.contained_by.iter().cloned().collect(); // clone: the guard owns the ids it excludes
    visited.extend(ids.iter().cloned()); // clone: the graph keeps the roots it starts from
    let mut candidates = candidates_of(&edges, &visited);
    let mut graph = LinkedGraph {
        roots: ids,
        nodes: IndexMap::new(),
        edges,
        failures: Vec::new(),
        depth: bounds.depth,
    };
    let mut remaining = Some(bounds.depth);

    while let Some(depth) = remaining
        && !candidates.is_empty()
    {
        visited.extend(candidates.iter().map(|candidate| candidate.id.clone())); // clone: the guard outlives this level's candidates
        let level = resolve_level(source, &candidates, depth, &visited).await?;
        // Everything the level produced, the sub-graph a remote holder resolved
        // around a candidate included, is now held, so no later level asks for
        // it again.
        visited.extend(level.nodes.keys().cloned()); // clone: the guard outlives this level's nodes
        remaining = NonZeroU32::new(depth.get() - 1);
        // Only a candidate's own edges lead further: a remote holder's extra
        // entities were resolved with the depth that holder was given, and
        // walking them again would reach further than `joinLevel` allows.
        let walked: HashSet<&Identifier> = candidates.iter().map(|candidate| &candidate.id).collect();
        let next = match remaining {
            Some(_) => candidates_of(level.edges.iter().filter(|edge| walked.contains(&edge.from)), &visited),
            None => Vec::new(),
        };
        candidates = next;
        graph.nodes.extend(level.nodes);
        graph.edges.extend(level.edges);
        graph.failures.extend(level.failures);
    }
    Ok(graph)
}

/// Resolve one level from the local store and, for the eligible candidates it
/// does not hold, from the remainder of the system.
///
/// `depth` is the number of levels still to resolve, this one included.
async fn resolve_level<S: LinkedSource>(
    source: &S,
    candidates: &[LinkedCandidate],
    depth: NonZeroU32,
    visited: &IndexSet<Identifier>,
) -> Result<Level<NodeOf<S>>, S::Error> {
    let LevelRead { nodes, edges } = source.read_level(candidates).await?;
    let mut level = Level {
        nodes,
        edges,
        failures: Vec::new(),
    };
    if !source.federates() {
        return Ok(level);
    }
    for candidate in candidates {
        if candidate.eligibility == RemoteEligibility::LocalOnly || level.nodes.contains_key(&candidate.id) {
            continue;
        }
        // A holder that resolves its own sub-graph is asked for the whole
        // remaining depth in one round trip; one that answers for the entity
        // alone is asked for it alone, and its edges join the next level here.
        let asked = match source.remote_reach(candidate).await {
            RemoteReach::Subgraph => depth,
            RemoteReach::SingleEntity => NonZeroU32::MIN,
        };
        // The candidate is already in the guard; the holder is told about
        // everything except the entity it is asked for.
        let held: Vec<Identifier> = visited.iter().filter(|id| **id != candidate.id).cloned().collect(); // clone: the port takes the ids as an owned slice
        let read = source.fetch_remote(candidate, asked, &held).await;
        fold_remote::<S::Representation>(read, candidate, visited, &mut level);
    }
    Ok(level)
}

/// Fold what a remote holder answered into the level it belongs to.
///
/// Whatever it returns that the requester already holds is dropped, with the
/// edges it names: `containedBy` keeps those out of the result.
fn fold_remote<R: LinkedRepresentation>(read: RemoteRead<R::Node>, candidate: &LinkedCandidate, visited: &IndexSet<Identifier>, level: &mut Level<R::Node>) {
    let RemoteRead { nodes, edges, failures } = read;
    let mut kept: HashSet<Identifier> = HashSet::new();
    for node in nodes {
        let Some(id) = R::identifier(&node) else {
            continue;
        };
        if (id != candidate.id && visited.contains(&id)) || level.nodes.contains_key(&id) {
            continue;
        }
        kept.insert(id.clone()); // clone: the edge filter outlives the insertion
        level.nodes.insert(id, node);
    }
    level.edges.extend(edges.into_iter().filter(|edge| kept.contains(&edge.from)));
    level.failures.extend(failures);
}

#[cfg(test)]
mod tests;
