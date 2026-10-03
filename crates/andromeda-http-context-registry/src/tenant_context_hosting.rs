//! One tenant's hosting of `@context`s, handed to the handlers that render.
//!
//! A response `Link` (clause 6.3.6, Annex C.8) and a subscription
//! `jsonldContext` (clause 5.8.1.4) name a hosted wrapper whenever no single
//! URL names the context; both are hosted among the stored `@context`s of the
//! tenant the request addressed and named by the URL
//! [`crate::hosted_context_url`] builds on the request's `apiRoot`.

use crate::{hosted_context_url::HostedContextUrls, response_context_host::ResponseContextHost};
use andromeda_context_registry::service::ContextRegistryService;
use andromeda_http_extraction::api_root::ApiRoot;
use std::sync::Arc;

/// One tenant's stored `@context`s, as the host of the contexts its responses
/// and subscriptions name.
#[derive(Clone)]
pub struct TenantContextHosting {
    /// The tenant's stored `@context`s.
    contexts: Arc<ContextRegistryService>,
}

impl TenantContextHosting {
    /// Hosting among `contexts`, the stored `@context`s of one tenant.
    #[must_use]
    pub const fn new(contexts: Arc<ContextRegistryService>) -> TenantContextHosting {
        TenantContextHosting { contexts }
    }

    /// The tenant's stored `@context`s.
    #[must_use]
    pub fn contexts(&self) -> &ContextRegistryService {
        &self.contexts
    }

    /// The host for a request answered under `api_root`.
    ///
    /// A request that revealed no `apiRoot` gets a host that refuses to host
    /// anything, before storing anything, since a wrapper it stored could not be
    /// given a URL.
    #[must_use]
    pub fn for_request<'a>(&'a self, api_root: &'a ApiRoot) -> ResponseContextHost<'a> {
        ResponseContextHost::new(&self.contexts, self.urls(api_root))
    }

    /// The URLs the tenant's entries are served under for a request answered
    /// under `api_root`, or `None` when the request revealed no `apiRoot`.
    #[must_use]
    pub fn urls<'a>(&self, api_root: &'a ApiRoot) -> Option<HostedContextUrls<'a>> {
        HostedContextUrls::of_request(api_root)
    }
}
