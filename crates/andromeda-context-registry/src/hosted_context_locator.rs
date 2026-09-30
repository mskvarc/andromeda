//! Where a stored `@context` is served (clauses 5.13.1 and 6.30).
//!
//! A Hosted or `ImplicitlyCreated` entry records the URL it is served under as
//! its `url` member (clause 5.13.2), and that URL is the one a response `Link`
//! or a subscription `jsonldContext` names. It is an absolute URL built on the
//! `apiRoot` and qualified by the tenant, which only the HTTP binding knows, so
//! the registry asks this port rather than building it.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use url::Url;

/// Names the URL a stored `@context` is served under.
///
/// A locator exists only once the `apiRoot` it builds on is known, so naming a
/// URL cannot fail and no entry is ever stored without one.
pub trait HostedContextLocator: Send + Sync {
    /// The absolute URL the entry `identifier` is served under.
    fn serving_url(&self, identifier: &JsonLdContextId) -> Url;
}
