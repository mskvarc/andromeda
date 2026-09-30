//! The `apiRoot` the broker mints its own absolute URLs under.
//!
//! Clause 6.2: "The apiRoot includes the scheme ("http" or "https"), host and
//! optional port, and an optional prefix string." A broker behind a reverse
//! proxy that mounts it under a prefix cannot learn that prefix from the
//! request, so an operator who configures the URL peers reach the broker at
//! (`[identity] advertised_url`) configures the `apiRoot` itself, path included,
//! and every URL the broker mints is built on it. Without one, the origin the
//! request reveals is the `apiRoot`, with no prefix.

use crate::{addressed_uri::addressed_uri, classification::classify_api_root_error};
use andromeda_http_headers::{
    api_root_scheme::ApiRootScheme,
    forwarded_header_trust::ForwardedHeaderTrust,
    forwarded_protocol::forwarded_protocol,
    listener_scheme::ListenerScheme,
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use std::future::{Future, ready};
use thiserror::Error;
use url::Url;

/// A request authority that cannot identify the broker origin.
#[derive(Debug, Error)]
pub enum ApiRootError {
    /// The header text cannot form an HTTP origin.
    #[error("host header does not identify a valid origin: {0}")]
    InvalidOrigin(#[source] url::ParseError),
}

/// The configured `apiRoot`, installed as a request extension by the API
/// assembly when `[identity] advertised_url` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvertisedApiRoot(Url);

impl AdvertisedApiRoot {
    /// The configured `apiRoot`, path prefix included.
    #[must_use]
    pub const fn new(url: Url) -> AdvertisedApiRoot {
        AdvertisedApiRoot(url)
    }

    /// The configured `apiRoot`.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.0
    }
}

/// The `apiRoot` the broker mints its own absolute URLs under.
///
/// The configured [`AdvertisedApiRoot`] when one is installed; otherwise the
/// origin the request reveals, or `None` for a request naming no authority,
/// under which nothing can be given an absolute URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRoot(Option<Url>);

impl ApiRoot {
    /// The `apiRoot` `url` names, path prefix included.
    #[must_use]
    pub const fn new(url: Option<Url>) -> ApiRoot {
        ApiRoot(url)
    }

    /// The `apiRoot`, when one is known.
    #[must_use]
    pub const fn url(&self) -> Option<&Url> {
        self.0.as_ref()
    }
}

/// The protocol a trusted reverse proxy forwarded, or `None` when the edge is
/// not trusted or forwarded nothing readable.
///
/// The trust decision arrives as a request extension the process stack installs
/// from configuration. A request that carries no such extension is treated as
/// untrusted, so a router assembled without that stack -- a unit test, a
/// fragment mounted on its own -- can never be talked into minting an `https`
/// URL by a client-supplied field.
fn forwarded_scheme(parts: &Parts) -> Option<ApiRootScheme> {
    match parts.extensions.get::<ForwardedHeaderTrust>() {
        Some(ForwardedHeaderTrust::Trusted) => forwarded_protocol(&parts.headers),
        Some(ForwardedHeaderTrust::Untrusted) | None => None,
    }
}

/// The scheme the listener the request arrived on speaks.
///
/// Each listener installs its [`ListenerScheme`] as a request extension. A
/// request carrying none is treated as plaintext, so a router assembled without
/// a listener -- a unit test, a fragment mounted on its own -- never mints an
/// `https` URL it was not served over.
fn listener_scheme(parts: &Parts) -> ApiRootScheme {
    parts
        .extensions
        .get::<ListenerScheme>()
        .map_or(ApiRootScheme::Http, |scheme| ApiRootScheme::from(*scheme))
}

/// The authority the request addresses.
///
/// HTTP/2 and HTTP/3 carry it in the `:authority` pseudo-header rather than a
/// `Host` field (RFC 9113 clause 8.3.1, RFC 9114 clause 4.3.1), and it arrives
/// as the authority of the request target; so does the authority of an
/// HTTP/1.1 absolute-form target, which RFC 9112 clause 3.2.2 has override
/// `Host`. Only an origin-form HTTP/1.1 request names its authority in `Host`
/// alone.
fn request_authority(parts: &Parts) -> Option<headers::Host> {
    // clone: the authority is a shared, reference-counted buffer the request keeps.
    parts
        .uri
        .authority()
        .cloned()
        .map(headers::Host::from)
        .or_else(|| parts.headers.typed_get::<headers::Host>())
}

/// Reconstruct the origin from the authority the request addresses and, when
/// the edge is trusted, from the protocol it forwarded; otherwise from the
/// scheme of the listener the request arrived on.
///
/// This is the origin the request itself reveals, whatever `apiRoot` is
/// configured: a relative reference the request carries resolves against the
/// URI the client addressed (IETF RFC 8288 section 3.1), not against the name
/// the broker advertises for itself.
///
/// # Errors
///
/// Returns [`ApiRootError::InvalidOrigin`] for an authority no origin can be
/// built from.
pub(crate) fn request_origin(parts: &Parts) -> Result<Option<Url>, ApiRootError> {
    let Some(host) = request_authority(parts) else {
        return Ok(None);
    };
    let scheme = forwarded_scheme(parts).unwrap_or_else(|| listener_scheme(parts));
    Url::parse(&format!("{}://{host}", scheme.as_scheme()))
        .map(Some)
        .map_err(ApiRootError::InvalidOrigin)
}

/// The configured `apiRoot` when one is installed, else the request's origin.
fn resolve_api_root(parts: &Parts) -> Result<ApiRoot, ApiRootError> {
    match parts.extensions.get::<AdvertisedApiRoot>() {
        Some(advertised) => Ok(ApiRoot(Some(advertised.url().clone()))), // clone: the extension stays installed for the rest of the request
        None => request_origin(parts).map(ApiRoot),
    }
}

impl<S> FromRequestParts<S> for ApiRoot
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_api_root(parts).map_err(|error| ExtractorRejection::Classified(classify_api_root_error(&error).with_instance(addressed_uri(parts)))))
    }
}

#[cfg(test)]
mod tests {
    use crate::api_root::{AdvertisedApiRoot, ApiRoot, ApiRootError, resolve_api_root};
    use andromeda_http_headers::{api_root_scheme::ApiRootScheme, forwarded_header_trust::ForwardedHeaderTrust, listener_scheme::ListenerScheme};
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use axum::{
        extract::FromRequestParts,
        http::{Request, request::Parts},
    };
    use std::error::Error;
    use url::{ParseError, Url};

    /// The parts of a request carrying `fields`, optionally under a declared
    /// proxy trust.
    fn parts(fields: &[(&'static str, &str)], trust: Option<ForwardedHeaderTrust>) -> Result<Parts, Box<dyn Error>> {
        let mut builder = Request::get("/ngsi-ld/v1/jsonldContexts");
        for (name, value) in fields {
            builder = builder.header(*name, *value);
        }
        let (mut parts, ()) = builder.body(())?.into_parts();
        if let Some(trust) = trust {
            parts.extensions.insert(trust);
        }
        Ok(parts)
    }

    /// The origin the extractor reconstructs from `parts`, as wire text.
    async fn origin_from(mut parts: Parts) -> Result<Option<String>, Box<dyn Error>> {
        let api_root = ApiRoot::from_request_parts(&mut parts, &()).await.map_err(|_| "origin rejected")?;
        Ok(api_root.url().map(Url::to_string))
    }

    /// The origin the extractor reconstructs, as wire text.
    async fn origin_of(fields: &[(&'static str, &str)], trust: Option<ForwardedHeaderTrust>) -> Result<Option<String>, Box<dyn Error>> {
        let parts = parts(fields, trust)?;
        origin_from(parts).await
    }

    /// The origin reconstructed for a request that arrived on a listener
    /// speaking `scheme`.
    async fn origin_on_listener(
        scheme: ApiRootScheme,
        fields: &[(&'static str, &str)],
        trust: Option<ForwardedHeaderTrust>,
    ) -> Result<Option<String>, Box<dyn Error>> {
        let mut parts = parts(fields, trust)?;
        parts.extensions.insert(ListenerScheme::from(scheme));
        origin_from(parts).await
    }

    #[tokio::test]
    async fn a_request_without_a_host_field_yields_no_origin() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(origin_of(&[], None).await?, None);
        Ok(())
    }

    /// A request that names no listener scheme is treated as plaintext, so
    /// that is the scheme of a minted URL when nothing trusted says otherwise.
    #[tokio::test]
    async fn the_listener_scheme_is_used_when_no_proxy_is_trusted() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            origin_of(&[("host", "broker.example:1056")], None).await?,
            Some("http://broker.example:1056/".to_owned())
        );
        Ok(())
    }

    /// The security property: a client-supplied forwarded field cannot change
    /// the scheme of a URL the broker mints and persists unless an operator has
    /// declared the edge trusted.
    #[tokio::test]
    async fn a_forwarded_protocol_is_ignored_while_the_edge_is_untrusted() -> Result<(), Box<dyn Error>> {
        for trust in [None, Some(ForwardedHeaderTrust::Untrusted)] {
            let fields = [("host", "broker.example"), ("x-forwarded-proto", "https"), ("forwarded", "proto=https")];
            pretty_assertions::assert_eq!(
                origin_of(&fields, trust).await?,
                Some("http://broker.example/".to_owned()),
                "an untrusted edge must not choose the scheme"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn a_trusted_edge_decides_the_scheme_of_a_minted_url() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            origin_of(
                &[("host", "broker.example"), ("x-forwarded-proto", "https")],
                Some(ForwardedHeaderTrust::Trusted)
            )
            .await?,
            Some("https://broker.example/".to_owned())
        );
        Ok(())
    }

    /// RFC 7239 clause 1 makes `Forwarded` the standards-track field, so a
    /// trusted edge's `Forwarded` outranks its `X-Forwarded-Proto`.
    #[tokio::test]
    async fn the_rfc_7239_field_outranks_the_de_facto_one_at_a_trusted_edge() -> Result<(), Box<dyn Error>> {
        let fields = [("host", "broker.example"), ("forwarded", "proto=https"), ("x-forwarded-proto", "http")];
        pretty_assertions::assert_eq!(
            origin_of(&fields, Some(ForwardedHeaderTrust::Trusted)).await?,
            Some("https://broker.example/".to_owned())
        );
        Ok(())
    }

    /// A field the broker cannot read leaves it on its listener scheme rather
    /// than failing the operation the request asked for.
    #[tokio::test]
    async fn a_malformed_forwarded_field_falls_back_to_the_listener_scheme() -> Result<(), Box<dyn Error>> {
        let fields = [("host", "broker.example"), ("forwarded", "definitely not rfc 7239")];
        pretty_assertions::assert_eq!(
            origin_of(&fields, Some(ForwardedHeaderTrust::Trusted)).await?,
            Some("http://broker.example/".to_owned())
        );
        Ok(())
    }

    /// A trusted edge that forwards plaintext keeps the plaintext scheme, so
    /// the trust switch is not a blanket upgrade to `https`.
    #[tokio::test]
    async fn a_trusted_edge_forwarding_plaintext_keeps_the_plaintext_scheme() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            origin_of(
                &[("host", "broker.example"), ("x-forwarded-proto", "http")],
                Some(ForwardedHeaderTrust::Trusted)
            )
            .await?,
            Some("http://broker.example/".to_owned())
        );
        Ok(())
    }

    /// `headers::Host` admits some authorities a URL cannot be built from --
    /// an out-of-range port, an empty host, a malformed IPv6 literal -- so the
    /// reconstruction can genuinely fail and says so rather than inventing an
    /// origin.
    #[tokio::test]
    async fn a_host_field_that_cannot_form_an_origin_is_rejected() -> Result<(), Box<dyn Error>> {
        for authority in ["host:99999", ":80", "[]", "[fe80::1%25eth0]:80"] {
            let mut parts = parts(&[("host", authority)], None)?;
            let rejection = ApiRoot::from_request_parts(&mut parts, &()).await;
            assert!(
                matches!(rejection, Err(ExtractorRejection::Classified(_))),
                "{authority} does not form an origin"
            );
        }
        Ok(())
    }

    /// The rejection carries the URL parser's own typed reason as its source.
    #[test]
    fn an_origin_rejection_chains_the_url_parse_error() -> Result<(), Box<dyn Error>> {
        let parts = parts(&[("host", "host:99999")], None)?;
        let rejection = resolve_api_root(&parts);
        assert!(
            matches!(rejection, Err(ApiRootError::InvalidOrigin(ParseError::InvalidPort))),
            "an out-of-range port is the URL parser's invalid-port error"
        );
        Ok(())
    }

    /// A `Host` field that is not an authority at all is no `Host` field, so
    /// the request yields no origin rather than a rejection -- the same answer
    /// as a request that carried none.
    #[tokio::test]
    async fn a_host_field_that_is_not_an_authority_yields_no_origin() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(origin_of(&[("host", "not a host")], None).await?, None);
        Ok(())
    }

    /// A TLS-terminating listener mints `https` URLs, including the durable
    /// clause 5.13 `@context` URLs, with no proxy involved.
    #[tokio::test]
    async fn an_https_listener_scheme_is_used_when_no_proxy_is_trusted() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            origin_on_listener(ApiRootScheme::Https, &[("host", "broker.example")], None).await?,
            Some("https://broker.example/".to_owned())
        );
        Ok(())
    }

    /// An untrusted edge can neither upgrade nor downgrade the listener's scheme.
    #[tokio::test]
    async fn an_untrusted_forwarded_http_cannot_downgrade_an_https_listener() -> Result<(), Box<dyn Error>> {
        for trust in [None, Some(ForwardedHeaderTrust::Untrusted)] {
            pretty_assertions::assert_eq!(
                origin_on_listener(ApiRootScheme::Https, &[("host", "broker.example"), ("x-forwarded-proto", "http")], trust).await?,
                Some("https://broker.example/".to_owned())
            );
        }
        Ok(())
    }

    /// A trusted edge knows the scheme the client used, which may differ from
    /// the hop between the edge and the broker.
    #[tokio::test]
    async fn a_trusted_edge_outranks_the_listener_scheme() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            origin_on_listener(
                ApiRootScheme::Https,
                &[("host", "broker.example"), ("forwarded", "proto=http")],
                Some(ForwardedHeaderTrust::Trusted)
            )
            .await?,
            Some("http://broker.example/".to_owned())
        );
        Ok(())
    }

    /// An HTTP/2 or HTTP/3 request names its authority in `:authority`, which
    /// arrives as the request target's authority with no `Host` field at all.
    #[tokio::test]
    async fn an_authority_pseudo_header_names_the_origin_without_a_host_field() -> Result<(), Box<dyn Error>> {
        let (mut parts, ()) = Request::get("https://broker.example:1056/ngsi-ld/v1/jsonldContexts").body(())?.into_parts();
        parts.extensions.insert(ListenerScheme::from(ApiRootScheme::Https));
        pretty_assertions::assert_eq!(origin_from(parts).await?, Some("https://broker.example:1056/".to_owned()));
        Ok(())
    }

    /// RFC 9112 clause 3.2.2: the authority of an absolute-form target
    /// outranks a disagreeing `Host` field.
    #[tokio::test]
    async fn a_target_authority_outranks_the_host_field() -> Result<(), Box<dyn Error>> {
        let (parts, ()) = Request::get("http://broker.example/ngsi-ld/v1/jsonldContexts")
            .header("host", "elsewhere.example")
            .body(())?
            .into_parts();
        pretty_assertions::assert_eq!(origin_from(parts).await?, Some("http://broker.example/".to_owned()));
        Ok(())
    }

    /// Clause 6.2: a configured `apiRoot` is the one URLs are minted under,
    /// its path prefix included, whatever authority the request addressed.
    #[tokio::test]
    async fn a_configured_api_root_outranks_the_request_origin_and_keeps_its_prefix() -> Result<(), Box<dyn Error>> {
        let mut parts = parts(&[("host", "internal.example:1026")], None)?;
        parts.extensions.insert(AdvertisedApiRoot::new(Url::parse("https://public.example/broker")?));
        pretty_assertions::assert_eq!(origin_from(parts).await?, Some("https://public.example/broker".to_owned()));
        Ok(())
    }

    /// A configured `apiRoot` needs no authority from the request at all.
    #[tokio::test]
    async fn a_configured_api_root_serves_a_request_naming_no_authority() -> Result<(), Box<dyn Error>> {
        let mut parts = parts(&[], None)?;
        parts.extensions.insert(AdvertisedApiRoot::new(Url::parse("https://public.example/")?));
        pretty_assertions::assert_eq!(origin_from(parts).await?, Some("https://public.example/".to_owned()));
        Ok(())
    }
}
