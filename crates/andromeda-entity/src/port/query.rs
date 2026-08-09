//! Compiled entity-query execution.

use crate::{error::EntityError, query_outcome::QueryOutcome, storage_profile::StorageProfile};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use futures::future::BoxFuture;
use serde_json::Value;

/// Executes compiled entity and identifier queries with explicit residual outcomes.
pub trait EntityQueryStore: Send + Sync + 'static {
    /// Report this store's per-instance query capabilities.
    fn profile(&self) -> &StorageProfile;
    /// Execute a compiled query for expanded entity payloads.
    fn query_compiled<'life0, 'life1, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Value>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Execute a compiled query for entity identifiers.
    fn query_ids<'life0, 'life1, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledEntityQuery,
    ) -> BoxFuture<'async_trait, Result<QueryOutcome<Uri>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
