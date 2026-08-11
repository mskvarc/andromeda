//! The non-local half of Linked Entity resolution (clause 4.5.23.1).
//!
//! Clause 4.5.23.1 makes a Relationship's `objectType` annotation the statement
//! that its object is an Internal Linked Entity: an entity of the declared type
//! that exists somewhere in the distributed system, not necessarily in the
//! store the request reached. The traversal therefore has two halves — the
//! local store answers for what it holds, and whatever holds the rest answers
//! through this port.
//!
//! The port is deliberately blind to how the remainder is reached. A broker
//! composed without federation supplies no implementation at all and resolves
//! links purely locally; a federated composition supplies one that forwards to
//! the registered context sources.

use crate::error::EntityError;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use futures::future::BoxFuture;
use std::{fmt, num::NonZeroU32};

/// What one non-local resolution attempt yielded.
///
/// The two halves are independent: a source can answer with entities *and*
/// report that part of what was asked for is missing.
#[derive(Debug, Default)]
pub struct RemoteLinkedEntities {
    /// The entities the source resolved.
    pub entities: Vec<Entity>,
    /// Every Linked Entity the attempt left unresolved.
    pub failures: Vec<LinkedResolutionFailure>,
}

/// Fetches an `objectType`-annotated Linked Entity that is not stored locally
/// (clause 4.5.23.1: a Relationship annotated with an `objectType` declares its
/// object to be an Internal Linked Entity).
///
/// An implementation returns the entity itself together with the linked
/// entities it resolved for the depth that remains beyond it, so one round trip
/// answers for a whole remote sub-graph rather than one hop of it.
///
/// The signature is spelled in its desugared form — the same shape
/// `#[async_trait::async_trait]` expands to — because this crate carries
/// `async-trait` only as a development dependency, exactly as the store ports
/// in [`crate::port`] do. Implementors are free to write `#[async_trait]`.
pub trait RemoteLinkedEntitySource: fmt::Debug + Send + Sync + 'static {
    /// Fetch the entity `id` names, plus the linked entities reachable from it
    /// within the remaining depth.
    ///
    /// `remaining_depth` counts the traversal levels still to be resolved
    /// *including* the level `id` itself sits on, so a value of one means the
    /// entity alone is wanted and nothing it links to.
    ///
    /// `visited` lists, in traversal order, the identifiers the caller has
    /// already resolved or deliberately excluded, and never contains `id`. An
    /// implementation passes them on as the entities the requester already
    /// holds (clause 4.5.23 `containedBy`), which keeps a link back into the
    /// already-walked part of the graph from being fetched twice.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the remainder of the system cannot be
    /// consulted at all. A source that merely fails to produce an entity
    /// returns no entity and a [`LinkedResolutionFailure`] describing why:
    /// clause 4.5.23 leaves an unresolvable Relationship as a plain URI rather
    /// than failing the retrieval.
    fn fetch_linked<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        remaining_depth: NonZeroU32,
        visited: &'life2 [Uri],
        context: &'life3 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<RemoteLinkedEntities, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
}
