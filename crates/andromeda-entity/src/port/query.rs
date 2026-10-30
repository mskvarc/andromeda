//! Compiled entity-query execution.

use crate::{
    error::EntityError,
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query_outcome::QueryOutcome,
    storage_profile::StorageProfile,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, page_window::PageWindow};
use futures::future::BoxFuture;

/// Executes compiled entity and identifier queries with explicit residual outcomes.
///
/// Both reads are driven by the [`PageWindow`] the request boundary decided,
/// and both return the page in the order it is presented: ascending by
/// identifier for a cursor window, in the query's order for an offset one. A
/// store that reads a page descending to serve it from one index scan reverses
/// it before returning it.
pub trait EntityQueryStore: Send + Sync + 'static {
    /// Report this store's per-instance query capabilities.
    fn profile(&self) -> &StorageProfile;
    /// Execute a compiled query for the stored states of the matching
    /// entities `window` selects, each with its entity-level clause 4.8 system
    /// times read by the same statement, counting every match when `count`
    /// asks for it (clause 6.3.13).
    fn query_compiled<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        compiled: &'life2 CompiledEntityQuery,
        window: &'life3 PageWindow,
        count: CountPolicy,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<StoredEntry<RetrievedEntity>>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
    /// Execute a compiled query for the identifiers of the matching entities
    /// `window` selects.
    fn query_ids<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        compiled: &'life2 CompiledEntityQuery,
        window: &'life3 PageWindow,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Identifier>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
}
