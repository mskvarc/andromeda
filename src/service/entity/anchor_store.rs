//! The store that anchors an atomic write.
//!
//! One asymmetry makes a generic coordinator possible instead of one per
//! combination of stores: the context store has to be transactional, because
//! clauses 5.6.1 through 5.6.6 require read-your-write on entities and every
//! other part of a write hangs off the entity mutation. That store is the
//! anchor; everything else is either co-located with it or, once a second
//! store exists, deferred through it.
//!
//! `apply_entities` takes the whole slice rather than one write at a time.
//! Applying part-by-write would make fusion impossible — N writes times four
//! parts is 4N statements — and fusion is the reason the unit of work was worth
//! replacing.

use crate::{
    error::AndromedaError,
    service::entity::{
        atomic_write::{AtomicWrite, WritePart},
        write_outcome::WriteOutcome,
    },
};
use futures::future::BoxFuture;

/// A store that can commit the parts of an atomic write it owns, atomically.
pub trait AnchorStore: Send + Sync + 'static {
    /// The transaction this store opens.
    type Transaction: Send;

    /// Which parts this store persists inside its own transaction.
    fn native_parts(&self) -> &[WritePart];

    /// Open a transaction.
    fn begin<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Self::Transaction, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Apply the entity mutation of every write, honouring each precondition,
    /// and report one outcome per write in the order they were given.
    fn apply_entities<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        tx: &'life1 mut Self::Transaction,
        writes: &'life2 [AtomicWrite],
    ) -> BoxFuture<'async_trait, Result<Vec<WriteOutcome>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Append the history of every write whose entity mutation landed.
    fn apply_history<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        tx: &'life1 mut Self::Transaction,
        landed: &'life2 [&'life3 AtomicWrite],
    ) -> BoxFuture<'async_trait, Result<(), AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Insert the notification outbox entries of every write that landed.
    fn apply_outbox<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        tx: &'life1 mut Self::Transaction,
        landed: &'life2 [&'life3 AtomicWrite],
    ) -> BoxFuture<'async_trait, Result<(), AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Enqueue the deferred side-effect jobs of every write that landed.
    fn apply_side_effects<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        tx: &'life1 mut Self::Transaction,
        landed: &'life2 [&'life3 AtomicWrite],
    ) -> BoxFuture<'async_trait, Result<(), AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Commit, and only then release whatever the commit made visible.
    fn commit<'life0, 'async_trait>(&'life0 self, tx: Self::Transaction) -> BoxFuture<'async_trait, Result<(), AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
