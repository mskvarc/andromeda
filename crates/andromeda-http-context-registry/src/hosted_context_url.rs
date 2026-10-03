//! The URL a stored `@context` is served under (clauses 5.13.1, 6.2 and 6.30).
//!
//! Clause 6.30 serves a stored `@context` at `/ngsi-ld/v1/jsonldContexts/{contextId}`
//! under the `apiRoot`, which clause 6.2 says "includes the scheme ("http" or
//! "https"), host and optional port, and an optional prefix string". Every URL
//! the broker mints for a Hosted or `ImplicitlyCreated` entry is built here, so a
//! response `Link`, a subscription `jsonldContext` and an entry's `url` member
//! all name the same absolute URL.
//!
//! The URL names no tenant. Clauses 4.14 and 6.3.14 make the `NGSILD-Tenant`
//! header the only way a request names a tenant, so an entry of a non-default
//! tenant is served to a request carrying that tenant's header, exactly as
//! every other resource of the tenant is.

use andromeda_context_registry::hosted_context_locator::HostedContextLocator;
use andromeda_http_extraction::api_root::ApiRoot;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use iri_rs::{InvalidIri, IriRefBuf};
use url::Url;

/// The path segments the clause 6.30 resource sits under, below the `apiRoot`.
pub(crate) const CONTEXTS_PATH: [&str; 3] = ["ngsi-ld", "v1", "jsonldContexts"];

/// The URLs stored `@context`s are served under, for one request.
///
/// Built only on a resolved `apiRoot`, so naming a URL cannot fail: a request
/// that reveals no `apiRoot` has no `HostedContextUrls` and is refused before
/// anything is hosted ([`crate::tenant_context_hosting`]).
#[derive(Debug, Clone, Copy)]
pub struct HostedContextUrls<'a> {
    /// The `apiRoot` the request is answered under.
    api_root: &'a Url,
}

impl<'a> HostedContextUrls<'a> {
    /// The URLs entries are served under below `api_root`.
    #[must_use]
    pub const fn new(api_root: &'a Url) -> HostedContextUrls<'a> {
        HostedContextUrls { api_root }
    }

    /// The URLs entries are served under for a request answered under
    /// `api_root`, or `None` when the request revealed no `apiRoot`.
    #[must_use]
    pub fn of_request(api_root: &'a ApiRoot) -> Option<HostedContextUrls<'a>> {
        api_root.url().map(HostedContextUrls::new)
    }
}

impl HostedContextLocator for HostedContextUrls<'_> {
    fn serving_url(&self, identifier: &JsonLdContextId) -> Url {
        hosted_context_url(self.api_root, identifier)
    }
}

/// `<apiRoot>/ngsi-ld/v1/jsonldContexts/<identifier>`, whichever tenant holds
/// the entry.
///
/// The `apiRoot`'s own path is kept as the prefix the resource sits below
/// (clause 6.2, "an optional prefix string"), with or without its trailing
/// slash; its query and fragment are dropped.
#[must_use]
pub fn hosted_context_url(api_root: &Url, identifier: &JsonLdContextId) -> Url {
    let mut url = api_root.clone(); // clone: the minted URL is built on its own copy of the apiRoot
    let prefix = api_root.path().trim_end_matches('/');
    url.set_path(&format!("{prefix}/{}/{identifier}", CONTEXTS_PATH.join("/")));
    url.set_query(None);
    url.set_fragment(None);
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
    use url::Url;

    const ID: &str = "urn:ngsi-ld:JsonLdContext:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10";

    fn identifier() -> JsonLdContextId {
        ID.parse().expect("an identifier")
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    /// Clauses 4.14 and 6.3.14: the tenant is named by the `NGSILD-Tenant`
    /// header alone, so the URL of any tenant's entry names no tenant.
    #[test]
    fn a_named_tenant_url_names_no_tenant() {
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://broker.example"), &identifier())),
            format!("https://broker.example/ngsi-ld/v1/jsonldContexts/{ID}")
        );
    }

    /// Clause 6.2: the `apiRoot` may carry a prefix, which every minted URL
    /// keeps, with or without a trailing slash.
    #[test]
    fn an_api_root_prefix_is_kept() {
        for root in ["https://proxy.example/broker", "https://proxy.example/broker/"] {
            pretty_assertions::assert_eq!(
                String::from(hosted_context_url(&url(root), &identifier())),
                format!("https://proxy.example/broker/ngsi-ld/v1/jsonldContexts/{ID}")
            );
        }
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://gw.example.com/a/b"), &identifier())),
            format!("https://gw.example.com/a/b/ngsi-ld/v1/jsonldContexts/{ID}")
        );
    }

    /// A query or fragment the configured `apiRoot` happened to carry is not
    /// part of any minted URL.
    #[test]
    fn an_api_root_query_and_fragment_are_dropped() {
        pretty_assertions::assert_eq!(
            String::from(hosted_context_url(&url("https://broker.example/p?x=1#f"), &identifier())),
            format!("https://broker.example/p/ngsi-ld/v1/jsonldContexts/{ID}")
        );
    }

    /// A request that revealed no `apiRoot` has no URLs to mint on.
    #[test]
    fn a_request_with_no_api_root_has_no_hosted_context_urls() {
        let api_root = ApiRoot::new(None);

        assert!(HostedContextUrls::of_request(&api_root).is_none());
    }

    #[test]
    fn a_request_with_an_api_root_mints_on_it() {
        let api_root = ApiRoot::new(Some(url("https://gw.example.com/broker")));

        pretty_assertions::assert_eq!(
            HostedContextUrls::of_request(&api_root).map(|urls| String::from(urls.serving_url(&identifier()))),
            Some(format!("https://gw.example.com/broker/ngsi-ld/v1/jsonldContexts/{ID}"))
        );
    }

    /// Clause 6.29.3.1: the `Location` is the local path, whatever the
    /// `apiRoot`.
    #[test]
    fn the_location_path_is_relative_and_names_the_identifier() {
        pretty_assertions::assert_eq!(
            hosted_context_path(&identifier()).map(|path| path.as_str().to_owned()).ok(),
            Some(format!("/ngsi-ld/v1/jsonldContexts/{ID}"))
        );
    }
}
