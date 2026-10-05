//! The port a consumer reads its Linked Entities through: I/O only.
//!
//! A source answers "read these identifiers" from where its consumer's data
//! lives, and "fetch this one from the remainder of the system" (clause
//! 4.5.23.1). Which identifiers are asked for, in which order, to which depth
//! and with which already-held identifiers is the traversal's decision, never
//! the source's.
//!
//! The methods return `impl Future` rather than a boxed future, the same
//! desugared shape the query crate's term expander uses: the traversal is
//! generic over its source, so nothing needs a trait object.

use crate::{candidate::LinkedCandidate, edge::LinkedEdge, representation::LinkedRepresentation};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use indexmap::IndexMap;
use std::{future::Future, num::NonZeroU32};

/// The node type a source reads.
pub type NodeOf<S> = <<S as LinkedSource>::Representation as LinkedRepresentation>::Node;

/// What one local read of a level produced.
#[derive(Debug)]
pub struct LevelRead<N> {
    /// The candidates the source holds, keyed by identifier in candidate
    /// order.
    pub nodes: IndexMap<Identifier, N>,
    /// Every edge those nodes name, read before any narrowing of the nodes,
    /// so a node whose own spelling no longer shows a Relationship still
    /// reports it.
    pub edges: Vec<LinkedEdge>,
}

/// What one fetch from the remainder of the system produced.
///
/// A source that cannot be consulted at all reports that as a failure here: a
/// remote Linked Entity that cannot be reached degrades the join, never the
/// operation (clause 4.5.23).
#[derive(Debug)]
pub struct RemoteRead<N> {
    /// The fetched entity and whatever the holder resolved around it.
    pub nodes: Vec<N>,
    /// Every edge those nodes name.
    pub edges: Vec<LinkedEdge>,
    /// Every Linked Entity the fetch left unresolved.
    pub failures: Vec<LinkedResolutionFailure>,
}

/// How far the holder of a remote Linked Entity resolves on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteReach {
    /// The holder resolves the levels below the entity itself, so it is asked
    /// for the whole remaining depth in one round trip.
    Subgraph,
    /// The holder answers for the one entity only; its Relationships are
    /// walked here, edge by edge.
    SingleEntity,
}

/// Where one consumer's Linked Entities are read from.
pub trait LinkedSource: Sync {
    /// How the nodes this source reads are spelled.
    type Representation: LinkedRepresentation;
    /// Why a local read failed, which fails the whole operation.
    type Error;

    /// Read every candidate the local store holds, with the edges each names.
    fn read_level(&self, candidates: &[LinkedCandidate]) -> impl Future<Output = Result<LevelRead<NodeOf<Self>>, Self::Error>> + Send;

    /// Fetch one candidate from the remainder of the system, resolving `depth`
    /// levels counting the candidate's own, and passing on `held` as the
    /// identifiers the requester already holds.
    fn fetch_remote(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> impl Future<Output = RemoteRead<NodeOf<Self>>> + Send;

    /// Whether this source consults the remainder of the system at all.
    fn federates(&self) -> bool;

    /// How far the holder of `candidate` resolves on its own.
    fn remote_reach(&self, candidate: &LinkedCandidate) -> impl Future<Output = RemoteReach> + Send;
}
