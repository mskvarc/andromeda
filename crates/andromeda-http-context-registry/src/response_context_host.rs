//! Hosting the `@context` a `Link` header or a subscription names.
//!
//! [`ResponseContextHost`] is the tenant's implementation of the
//! [`CompoundContextHost`] port for one request: it stores the wrapper as an
//! `ImplicitlyCreated` entry of the tenant's registry and answers the absolute
//! URL it is served under ([`crate::hosted_context_url`]).

use crate::hosted_context_url::HostedContextUrls;
use andromeda_context_registry::{error::ContextRegistryError, hosted_context_locator::HostedContextLocator, service::ContextRegistryService};
use andromeda_jsonld::compound_context_host::{CompoundContextHost, CompoundContextHostFuture, ContextHostingError, WrapperUse};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use iri_rs::IriBuf;
use serde_json::{Map, Value};

/// Hosts wrappers among one tenant's stored `@context`s for one request.
#[derive(derive_more::Debug)]
pub struct ResponseContextHost<'a> {
    /// The tenant's stored `@context`s.
    #[debug(skip)]
    contexts: &'a ContextRegistryService,
    /// The URLs the tenant's entries are served under for this request, or
    /// `None` when the request revealed no `apiRoot` to mint them on.
    urls: Option<HostedContextUrls<'a>>,
}

impl<'a> ResponseContextHost<'a> {
    /// Host wrappers in `contexts`, served under `urls`.
    #[must_use]
    pub const fn new(contexts: &'a ContextRegistryService, urls: Option<HostedContextUrls<'a>>) -> ResponseContextHost<'a> {
        ResponseContextHost { contexts, urls }
    }
}

impl CompoundContextHost for ResponseContextHost<'_> {
    fn host<'b>(&'b self, context: &'b Value, usage: WrapperUse) -> CompoundContextHostFuture<'b> {
        Box::pin(async move {
            // Refused before anything is stored: a wrapper stored without a URL
            // could be named by nothing.
            let Some(urls) = self.urls else {
                return Err(ContextHostingError::NoRequestAuthority);
            };
            let mut document = Map::new();
            document.insert("@context".to_owned(), context.clone()); // clone: the stored document owns the context the caller keeps rendering
            let identifier = self
                .contexts
                .create_implicitly(Value::Object(document), &urls, usage)
                .await
                .map_err(|error| hosting_failure(&error))?;
            IriBuf::new(String::from(urls.serving_url(&identifier))).map_err(ContextHostingError::MintedUrlNotIri)
        })
    }
}

/// The hosting failure a Context Registry failure amounts to.
///
/// A saturated store stays backpressure the client can retry, an unreachable
/// store stays retryable and a store fault stays that fault; every other registry failure means the registry broke its
/// own contract while storing a wrapper the broker built, which is recorded
/// here with its detail and reported as such.
fn hosting_failure(error: &ContextRegistryError) -> ContextHostingError {
    match error {
        ContextRegistryError::StoreSaturated => ContextHostingError::Persistence(PersistenceFailure::Saturated),
        ContextRegistryError::StoreUnavailable => ContextHostingError::Persistence(PersistenceFailure::Unavailable),
        ContextRegistryError::Persistence(failure) => ContextHostingError::Persistence(*failure),
        ContextRegistryError::NoLocalContext(_)
        | ContextRegistryError::NotFound { .. }
        | ContextRegistryError::CoreContextDeletion
        | ContextRegistryError::CachedContextNotServed { .. }
        | ContextRegistryError::ReloadRequiresCached { .. }
        | ContextRegistryError::MissingSourceUrl { .. }
        | ContextRegistryError::ContextUnavailable { .. }
        | ContextRegistryError::Representation(_)
        | ContextRegistryError::Invariant(_) => ContextHostingError::Persistence(PersistenceFailure::ContractBroken.record(error)),
    }
}

#[cfg(test)]
mod tests {
    use crate::response_context_host::hosting_failure;
    use andromeda_context_registry::{error::ContextRegistryError, registry_invariant::RegistryInvariant};
    use andromeda_jsonld::compound_context_host::ContextHostingError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    #[test]
    fn a_saturated_or_unreachable_store_stays_retryable_and_a_store_fault_stays_that_fault() {
        assert!(matches!(
            hosting_failure(&ContextRegistryError::StoreSaturated),
            ContextHostingError::Persistence(PersistenceFailure::Saturated)
        ));
        assert!(matches!(
            hosting_failure(&ContextRegistryError::StoreUnavailable),
            ContextHostingError::Persistence(PersistenceFailure::Unavailable)
        ));
        assert!(matches!(
            hosting_failure(&ContextRegistryError::Persistence(PersistenceFailure::StatementRejected)),
            ContextHostingError::Persistence(PersistenceFailure::StatementRejected)
        ));
    }

    #[test]
    fn any_other_registry_failure_is_a_broken_contract() {
        assert!(matches!(
            hosting_failure(&ContextRegistryError::from(RegistryInvariant::MintedIdentifierTaken {
                identifier: "urn:ngsi-ld:JsonLdContext:x".parse().expect("an identifier"),
            })),
            ContextHostingError::Persistence(PersistenceFailure::ContractBroken)
        ));
    }
}
