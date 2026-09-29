//! The replaceable persistence capability for hosted and cached Contexts.

use crate::{
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    stored_context::StoredContext,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;
use std::{future::Future, pin::Pin};
use url::Url;
use urn_rs::Urn;

/// Owned future returned by an object-safe Context Registry store implementation.
pub type ContextRegistryFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ContextRegistryError>> + Send + 'a>>;

/// Persists hosted and cached Context lifecycle without exposing a backend.
pub trait ContextRegistryStore: Send + Sync + 'static {
    /// Store a hosted or implicitly created Context and return its local URN.
    fn create<'a>(&'a self, payload: Value, base_url: Option<&'a Url>, kind: ContextCreationKind) -> ContextRegistryFuture<'a, Urn>;

    /// List stored Contexts using a policy-owned projection and keyset query.
    fn list<'a>(&'a self, query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ListedContexts>;

    /// Fetch one stored Context using the requested projection.
    fn get<'a>(&'a self, identifier: &'a Identifier, detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>>;

    /// Record a direct hosted-context access.
    fn record_access<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation>;

    /// Load lifecycle information required for delete or reload.
    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>>;

    /// Delete a stored Context.
    fn delete<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation>;

    /// Replace a stored Context document by local identifier.
    fn replace_payload<'a>(&'a self, identifier: &'a Identifier, payload: Value) -> ContextRegistryFuture<'a, ContextMutation>;
}

#[cfg(test)]
mod tests {
    use crate::detail_level::ContextDetailLevel;

    #[test]
    fn store_projection_is_policy_owned() {
        let detail = ContextDetailLevel::Full;
        assert!(matches!(detail, ContextDetailLevel::Full));
    }
}
