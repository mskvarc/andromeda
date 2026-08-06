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
    core::error::AndromedaError,
    service::entity::{
        atomic_write::{AtomicWrite, WritePart},
        write_outcome::WriteOutcome,
    },
};
use async_trait::async_trait;

/// A store that can commit the parts of an atomic write it owns, atomically.
#[async_trait]
pub trait AnchorStore: Send + Sync + 'static {
    /// The transaction this store opens.
    type Transaction: Send;

    /// Which parts this store persists inside its own transaction.
    fn native_parts(&self) -> &[WritePart];

    /// Open a transaction.
    async fn begin(&self) -> Result<Self::Transaction, AndromedaError>;

    /// Apply the entity mutation of every write, honouring each precondition,
    /// and report one outcome per write in the order they were given.
    async fn apply_entities(&self, tx: &mut Self::Transaction, writes: &[AtomicWrite]) -> Result<Vec<WriteOutcome>, AndromedaError>;

    /// Append the history of every write whose entity mutation landed.
    async fn apply_history(&self, tx: &mut Self::Transaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError>;

    /// Insert the notification outbox entries of every write that landed.
    async fn apply_outbox(&self, tx: &mut Self::Transaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError>;

    /// Enqueue the deferred side-effect jobs of every write that landed.
    async fn apply_side_effects(&self, tx: &mut Self::Transaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError>;

    /// Commit, and only then release whatever the commit made visible.
    async fn commit(&self, tx: Self::Transaction) -> Result<(), AndromedaError>;
}
