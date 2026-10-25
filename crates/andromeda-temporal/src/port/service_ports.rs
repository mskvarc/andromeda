//! The caller-owned ports temporal policy is constructed over.

use crate::port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::port::reader::EntityReader;
use andromeda_retry::retry_delay::RetryDelay;
use std::sync::Arc;

/// The storage capabilities a temporal service answers requests through.
///
/// They are built as one set by the composition layer: reading history without
/// being able to append it, or appending without the barrier that orders an
/// immediate mutation against deferred work, is not a working service.
pub struct TemporalServicePorts {
    /// Base entity lookups a temporal read resolves its subjects against.
    pub entity_reader: Arc<dyn EntityReader>,
    /// Temporal history reads.
    pub reader: Arc<dyn TemporalHistoryReader>,
    /// Ordering of an immediate mutation against deferred temporal work.
    pub mutation_barrier: Arc<dyn TemporalMutationBarrier>,
    /// Temporal history mutations.
    pub mutator: Arc<dyn TemporalMutator>,
    /// Durable context commits carrying atomic append composition.
    pub writes: Arc<dyn ContextCommitter>,
    /// The wait between two attempts of a write a concurrent writer superseded.
    pub retry_delay: Arc<dyn RetryDelay>,
}
