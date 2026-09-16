//! Reconstruction of the broker origin from an incoming request.

use crate::classification::classify_request_origin_error;
use andromeda_http_headers::{
    forwarded_header_trust::ForwardedHeaderTrust,
    forwarded_protocol::{ForwardedProtocol, forwarded_protocol},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use std::future::{Future, ready};
use thiserror::Error;
use url::Url;

/// The scheme the broker's own listener speaks.
///
/// Andromeda serves plaintext HTTP and is deployed behind a TLS terminator
/// (`docs/tls-deployment.md`), so this is the scheme of every URL it mints
/// unless a proxy the operator has declared trusted says otherwise.
const LISTENER_PROTOCOL: ForwardedProtocol = ForwardedProtocol::Http;

/// A `Host` header that cannot identify the broker origin.
#[derive(Debug, Error)]
pub enum RequestOriginError {
    /// The header text cannot form an HTTP origin.
    #[error("host header does not identify a valid origin: {detail}")]
    InvalidOrigin {
        /// URL parsing detail.
        detail: String,
    },
}

/// The origin the broker mints its own absolute URLs against.
///
/// Clause 6.2 makes the `apiRoot` "the scheme (`http` or `https`), host and
/// optional port": this is that origin as the request reveals it. It is `None`
/// for a request carrying no `Host` header, and the resources that would embed
/// an origin are then stored without one.
pub struct RequestOrigin(pub Option<Url>);

impl RequestOrigin {
    /// Consume the extractor and return the optional origin.
    #[must_use]
    pub fn into_inner(self) -> Option<Url> {
        self.0
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
fn forwarded_scheme(parts: &Parts) -> Option<ForwardedProtocol> {
    match parts.extensions.get::<ForwardedHeaderTrust>() {
        Some(ForwardedHeaderTrust::Trusted) => forwarded_protocol(&parts.headers),
        Some(ForwardedHeaderTrust::Untrusted) | None => None,
    }
}

/// Reconstruct the origin from the request's `Host` field and, when the edge is
/// trusted, from the protocol it forwarded.
fn resolve_origin(parts: &Parts) -> Result<RequestOrigin, RequestOriginError> {
    let Some(host) = parts.headers.typed_get::<headers::Host>() else {
        return Ok(RequestOrigin(None));
    };
    let scheme = match forwarded_scheme(parts) {
        Some(forwarded) => forwarded,
        None => LISTENER_PROTOCOL,
    };
    Url::parse(&format!("{}://{host}", scheme.as_scheme()))
        .map(|origin| RequestOrigin(Some(origin)))
        .map_err(|error| RequestOriginError::InvalidOrigin { detail: error.to_string() })
}

impl<S> FromRequestParts<S> for RequestOrigin
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_origin(parts).map_err(|error| ExtractorRejection::Classified(classify_request_origin_error(&error).with_instance(parts.uri.clone()))))
    }
}

#[cfg(test)]
mod tests {
    use crate::origin::RequestOrigin;
    use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use axum::{
        extract::FromRequestParts,
        http::{Request, request::Parts},
    };
    use std::error::Error;

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

    /// The origin the extractor reconstructs, as wire text.
    async fn origin_of(fields: &[(&'static str, &str)], trust: Option<ForwardedHeaderTrust>) -> Result<Option<String>, Box<dyn Error>> {
        let mut parts = parts(fields, trust)?;
        let origin = RequestOrigin::from_request_parts(&mut parts, &()).await.map_err(|_| "origin rejected")?;
        Ok(origin.into_inner().map(|url| url.to_string()))
    }

    #[tokio::test]
    async fn a_request_without_a_host_field_yields_no_origin() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(origin_of(&[], None).await?, None);
        Ok(())
    }

    /// The broker's own listener speaks plaintext HTTP, so that is the scheme
    /// of a minted URL when nothing trusted says otherwise.
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
            let rejection = RequestOrigin::from_request_parts(&mut parts, &()).await;
            assert!(
                matches!(rejection, Err(ExtractorRejection::Classified(_))),
                "{authority} does not form an origin"
            );
        }
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
}
