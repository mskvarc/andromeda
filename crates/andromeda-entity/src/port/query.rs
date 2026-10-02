//! Compiled entity-query execution.

use crate::{error::EntityError, port::retrieved_entity::RetrievedEntity, query_outcome::QueryOutcome, storage_profile::StorageProfile};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use futures::future::BoxFuture;

/// Executes compiled entity and identifier queries with explicit residual outcomes.
pub trait EntityQueryStore: Send + Sync + 'static {
    /// Report this store's per-instance query capabilities.
    fn profile(&self) -> &StorageProfile;
    /// Execute a compiled query for the stored states of the matching
    /// entities, each with its entity-level clause 4.8 system times read by the
    /// same statement.
    fn query_compiled<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        compiled: &'life2 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<RetrievedEntity>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Execute a compiled query for entity identifiers.
    fn query_ids<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        compiled: &'life2 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Identifier>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
