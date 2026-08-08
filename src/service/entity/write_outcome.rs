//! What one [`AtomicWrite`] did to the entity it named.
//!
//! [`WriteCoordinator::commit`] returns exactly one of these per submitted
//! write, in submission order. That is what lets a batch caller read its
//! per-entity results positionally instead of re-associating returned
//! identifiers with the payloads it sent.
//!
//! [`AtomicWrite`]: crate::service::entity::atomic_write::AtomicWrite
//! [`WriteCoordinator::commit`]: crate::service::entity::write_coordinator::WriteCoordinator::commit

use crate::core::error::AndromedaError;
use andromeda_postgresql_context_store::entity::deleted::DeletedEntity;
use derive_more::IsVariant;

/// The result of one entity's atomic write.
#[derive(Debug, IsVariant)]
pub enum WriteOutcome {
    /// The entity row was inserted.
    Created,
    /// The entity row was overwritten in place.
    Updated,
    /// The entity row was removed, and held this at the moment it went.
    Deleted(DeletedEntity),
    /// Nothing was written: an entity with this identifier already exists and
    /// the write required that it not (NGSI-LD 6.4.3.1).
    AlreadyExists,
    /// Nothing was written: no entity with this identifier exists and the write
    /// required one.
    NotFound,
    /// Nothing was written: the entity changed after the caller read it, so the
    /// write was computed against a payload that no longer applies.
    Superseded,
    /// Nothing was written and nothing was required: an unconditional insert
    /// met an existing row and left it alone.
    Skipped,
    /// The write asked for no entity mutation. Its history, outbox and
    /// side-effect parts still landed.
    NoEntityChange,
}

/// The single outcome a one-entity commit produces.
///
/// # Errors
///
/// Returns [`AndromedaError::internal`] if the coordinator returned anything
/// other than exactly one outcome, which its contract forbids.
pub fn single(outcomes: Vec<WriteOutcome>) -> Result<WriteOutcome, AndromedaError> {
    let mut outcomes = outcomes;
    match outcomes.len() {
        1 => outcomes.pop().ok_or_else(|| AndromedaError::internal("single write produced no outcome")),
        other => Err(AndromedaError::internal(format!("single write produced {other} outcomes"))),
    }
}
