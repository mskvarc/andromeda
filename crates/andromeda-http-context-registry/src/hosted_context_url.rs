//! The URL a stored `@context` is served under (clauses 5.13.1, 6.2 and 6.30).
//!
//! Clause 6.30 serves a stored `@context` at `/ngsi-ld/v1/jsonldContexts/{contextId}`
//! under the `apiRoot`, which clause 6.2 says "includes the scheme ("http" or
//! "https"), host and optional port, and an optional prefix string". Every URL
//! the broker mints for a Hosted or `ImplicitlyCreated` entry is built here, so a
//! response `Link`, a subscription `jsonldContext` and an entry's `url` member
//! all name the same absolute URL.
//!
//! A JSON-LD processor dereferencing such a URL sends no `NGSILD-Tenant`
//! header, so the URL of a non-default tenant's entry names the tenant in the
//! vendor query parameter `andromedaTenant`, which the Serve operation reads in
//! the header's absence (clause 4.14 keeps the entry the tenant's own; clause
//! 6.3.14 omits the header for the default tenant, and so does the URL).

use andromeda_context_registry::hosted_context_locator::HostedContextLocator;
use andromeda_http_extraction::api_root::ApiRoot;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::{InvalidIri, IriRefBuf};
use url::Url;

/// The vendor query parameter naming the tenant of a stored `@context`.
pub(crate) const TENANT_PARAMETER: &str = "andromedaTenant";

/// The path segments the clause 6.30 resource sits under, below the `apiRoot`.
pub(crate) const CONTEXTS_PATH: [&str; 3] = ["ngsi-ld", "v1", "jsonldContexts"];

/// The URLs one tenant's stored `@context`s are served under, for one request.
///
/// Built only on a resolved `apiRoot`, so naming a URL cannot fail: a request
/// that reveals no `apiRoot` has no `HostedContextUrls` and is refused before
/// anything is hosted ([`crate::tenant_context_hosting`]).
#[derive(Debug, Clone, Copy)]
pub struct HostedContextUrls<'a> {
    /// The `apiRoot` the request is answered under.
    api_root: &'a Url,
    /// The tenant whose entries these are.
    tenant: &'a TenantRef,
}

impl<'a> HostedContextUrls<'a> {
    /// The URLs `tenant`'s entries are served under below `api_root`.
    #[must_use]
    pub const fn new(api_root: &'a Url, tenant: &'a TenantRef) -> HostedContextUrls<'a> {
        HostedContextUrls { api_root, tenant }
    }

    /// The URLs `tenant`'s entries are served under for a request answered
    /// under `api_root`, or `None` when the request revealed no `apiRoot`.
    #[must_use]
    pub fn of_request(api_root: &'a ApiRoot, tenant: &'a TenantRef) -> Option<HostedContextUrls<'a>> {
        api_root.url().map(|root| HostedContextUrls::new(root, tenant))
    }
}

impl HostedContextLocator for HostedContextUrls<'_> {
    fn serving_url(&self, identifier: &JsonLdContextId) -> Url {
        hosted_context_url(self.api_root, identifier, self.tenant)
    }
}

/// `<apiRoot>/ngsi-ld/v1/jsonldContexts/<identifier>`, with
/// `?andromedaTenant=<tenant>` for a non-default tenant.
///
/// The `apiRoot`'s own path is kept as the prefix the resource sits below
/// (clause 6.2, "an optional prefix string"), with or without its trailing
/// slash; its query and fragment are dropped.
#[must_use]
pub fn hosted_context_url(api_root: &Url, identifier: &JsonLdContextId, tenant: &TenantRef) -> Url {
    let mut url = api_root.clone(); // clone: the minted URL is built on its own copy of the apiRoot
    let prefix = api_root.path().trim_end_matches('/');
    url.set_path(&format!("{prefix}/{}/{identifier}", CONTEXTS_PATH.join("/")));
    url.set_query(None);
    url.set_fragment(None);
    if let Some(name) = tenant.tenant_name() {
        url.query_pairs_mut().append_pair(TENANT_PARAMETER, name.as_str());
    }
    url
}

/// `/ngsi-ld/v1/jsonldContexts/<identifier>`, the relative path the
/// `Location` of Add @context names (clause 6.29.3.1).
///
/// # Errors
///
/// Returns the rejected text when the path is not an IRI reference, which a
/// path of fixed segments and a URN identifier never is.
pub fn hosted_context_path(identifier: &JsonLdContextId) -> Result<IriRefBuf, InvalidIri<String>> {
    IriRefBuf::new(format!("/{}/{identifier}", CONTEXTS_PATH.join("/")))
}

#[cfg(test)]
mod tests {
    use crate::hosted_context_url::{HostedContextUrls, hosted_context_path, hosted_context_url};
    use andromeda_context_registry::hosted_context_locator::HostedContextLocator;
    use andromeda_http_extraction::api_root::ApiRoot;
    use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use url::Url;

    const ID: &str = "urn:ngsi-ld:JsonLdContext:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10";

    fn identifier() -> JsonLdContextId {
        ID.parse().expect("an identifier")
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    fn named(name: &str) -> TenantRef {
        TenantRef::Named(name.parse().expect("a tenant name"))
    }

    #[test]
    fn a_default_tenant_url_carries_no_tenant_parameter() {
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://broker.example"), &identifier(), &TenantRef::UnnamedDefault)),
            format!("https://broker.example/ngsi-ld/v1/jsonldContexts/{ID}")
        );
    }

    #[test]
    fn a_named_tenant_url_names_the_tenant() {
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://broker.example"), &identifier(), &named("acme"))),
            format!("https://broker.example/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme")
        );
    }

    /// Clause 6.2: the `apiRoot` may carry a prefix, which every minted URL
    /// keeps, with or without a trailing slash, prefixed or not, and with the
    /// tenant parameter of a named tenant.
    #[test]
    fn an_api_root_prefix_is_kept() {
        for root in ["https://proxy.example/broker", "https://proxy.example/broker/"] {
            pretty_assertions::assert_eq!(
                String::from(hosted_context_url(&url(root), &identifier(), &TenantRef::UnnamedDefault)),
                format!("https://proxy.example/broker/ngsi-ld/v1/jsonldContexts/{ID}")
            );
        }
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://gw.example.com/a/b"), &identifier(), &named("acme"))),
            format!("https://gw.example.com/a/b/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme")
        );
    }

    /// A query or fragment the configured `apiRoot` happened to carry is not
    /// part of any minted URL.
    #[test]
    fn an_api_root_query_and_fragment_are_dropped() {
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(
                &url("https://broker.example/p?x=1#f"),
                &identifier(),
                &TenantRef::UnnamedDefault
            )),
            format!("https://broker.example/p/ngsi-ld/v1/jsonldContexts/{ID}")
        );
    }

    /// A request that revealed no `apiRoot` has no URLs to mint on.
    #[test]
    fn a_request_with_no_api_root_has_no_hosted_context_urls() {
        let api_root = ApiRoot::new(None);
        let tenant = TenantRef::UnnamedDefault;

        assert!(HostedContextUrls::of_request(&api_root, &tenant).is_none());
    }

    #[test]
    fn a_request_with_an_api_root_mints_on_it() {
        let api_root = ApiRoot::new(Some(url("https://gw.example.com/broker")));
        let tenant = named("acme");

        pretty_assertions::assert_eq!(
            HostedContextUrls::of_request(&api_root, &tenant).map(|urls| String::from(urls.serving_url(&identifier()))),
            Some(format!("https://gw.example.com/broker/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"))
        );
    }

    /// Clause 6.29.3.1: the `Location` is the local path, whatever the
    /// `apiRoot`, and it names no tenant.
    #[test]
    fn the_location_path_is_relative_and_names_the_identifier() {
        pretty_assertions::assert_eq!(
            hosted_context_path(&identifier()).map(|path| path.as_str().to_owned()).ok(),
            Some(format!("/ngsi-ld/v1/jsonldContexts/{ID}"))
        );
    }
}
