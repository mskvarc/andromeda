//! The reads that let a deletion skip the payload it removes.

use crate::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use futures::future::BoxFuture;

/// Reads what a deletion needs short of the removed payload.
///
/// The answer about watchers comes from the store every replica shares, never
/// from one process's subscription cache alone: a replica's cache does not see
/// a subscription another replica created, and a deletion that skipped the
/// payload on such a cache's word would leave a matched subscription without
/// the state it is notified with.
pub trait DeletionOutlineReader: Send + Sync + 'static {
    /// The outline of the entity `id` names, or `None` when there is none.
    fn deletion_outline<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<DeletionOutline>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Whether any subscription a deletion in `target` can notify exists.
    ///
    /// The answer may be coarser than the matching it guards, never finer: it
    /// may name a subscription no deletion of a given entity matches, but it
    /// never answers [`DeletionWatchers::None`] while one exists.
    fn deletion_watchers<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<DeletionWatchers, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
