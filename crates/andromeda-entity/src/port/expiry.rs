//! Entity and attribute-instance expiry candidates.

use crate::{entity_snapshot::EntitySnapshot, error::EntityError, expiry::EntityExpiryBatchSize};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;

/// Supplies expiry candidates without performing policy-owned deletion or rewriting.
pub trait EntityExpirySource: Send + Sync + 'static {
    /// List at most `batch_size` entities whose entity-level `expiresAt` is at or before `cutoff`.
    fn entities_due_for_expiry<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> BoxFuture<'async_trait, Result<Vec<Identifier>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Read at most `batch_size` versioned entities carrying attribute instances due at `cutoff`.
    ///
    /// A candidate whose stored rows no longer read back is left out, with
    /// why recorded by the store, so the candidates after it are still read.
    fn entities_with_expired_attributes<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> BoxFuture<'async_trait, Result<Vec<(Identifier, EntitySnapshot)>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
