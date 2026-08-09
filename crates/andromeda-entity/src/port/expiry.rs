//! Entity and attribute-instance expiry candidates.

use crate::{entity_snapshot::EntitySnapshot, error::EntityError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use futures::future::BoxFuture;

/// Supplies expiry candidates without performing policy-owned deletion or rewriting.
pub trait EntityExpirySource: Send + Sync + 'static {
    /// List entities whose entity-level `expiresAt` has passed.
    fn entities_due_for_expiry<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<Uri>, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Read versioned entities carrying past-due attribute instances.
    fn entities_with_expired_attributes<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<(Uri, EntitySnapshot)>, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
