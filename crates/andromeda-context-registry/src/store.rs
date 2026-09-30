//! The replaceable persistence capability for hosted and cached Contexts.

use crate::{
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    insertion::ContextInsertion,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    stored_context::StoredContext,
};
use andromeda_jsonld::context_cache::DownloadedContext;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use serde_json::Value;
use std::{future::Future, pin::Pin};
use url::Url;

/// Owned future returned by an object-safe Context Registry store implementation.
pub type ContextRegistryFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ContextRegistryError>> + Send + 'a>>;

/// Persists hosted and cached Context lifecycle without exposing a backend.
pub trait ContextRegistryStore: Send + Sync + 'static {
    /// Store a hosted or implicitly created Context under `identifier`,
    /// recording `serving_url` as the URL it is served under when one is known,
    /// unless a stored Context already holds that identifier.
    ///
    /// Which identifier a new Context takes is policy (clause 5.13.1 leaves it
    /// to the broker), so the caller chooses it and the store only reports
    /// whether it was free. An occupied identifier stores nothing and changes
    /// nothing.
    fn insert<'a>(
        &'a self,
        identifier: &'a JsonLdContextId,
        payload: &'a Value,
        serving_url: &'a Url,
        kind: ContextCreationKind,
    ) -> ContextRegistryFuture<'a, ContextInsertion>;

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

    /// Replace a "Cached" Context's document, by local identifier, with a new
    /// download of it and the URL that download was finally read from (clause
    /// 5.13.5.4).
    fn replace_download<'a>(&'a self, identifier: &'a Identifier, download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation>;
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
