//! The NGSI-LD version profile a request announces through its `Link` header.
//!
//! Clause 6.3.21 lets a client say which NGSI-LD version its payload is
//! written against, by an IETF RFC 6906 profile link naming
//! `https://uri.etsi.org/ngsi-ld/profile/<major>.<minor>`. It is a statement
//! about the request, not a preference about the response: the response
//! version is negotiated by `Prefer: ngsi-ld=<version>` alone (clause 6.3.6),
//! so a profile never turns a 200 into a 203.

use andromeda_http_headers::link::header::Link;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use axum::{
    extract::FromRequestParts,
    http::{header::LINK, request::Parts},
};
use std::{
    convert::Infallible,
    future::{Future, ready},
    str::FromStr,
};

/// The NGSI-LD version the request's payload is announced to conform to
/// (clause 6.3.21), snapped to the nearest version the clause 4.3.6.8 fallback
/// tables describe, or `None` when no `Link` carries an NGSI-LD profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PayloadProfile(pub Option<SpecVersion>);

impl PayloadProfile {
    /// The announced version, if any.
    #[must_use]
    pub const fn version(self) -> Option<SpecVersion> {
        self.0
    }
}

/// Reads the profile off every `Link` header present.
///
/// A `Link` header that does not parse is skipped here rather than rejected:
/// the context extractor owns the clause 5.5.4 rejection of a malformed
/// `Link`, and reporting it twice would serve no one.
fn announced_profile(parts: &Parts) -> PayloadProfile {
    let announced = parts
        .headers
        .get_all(LINK)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|raw| Link::from_str(raw).ok())
        .find_map(|link| link.ngsi_ld_profile_version());
    let profile = announced.map(SpecVersion::nearest_supported);
    if let Some(version) = profile {
        tracing::debug!(%version, "request payload announced an NGSI-LD version profile");
    }
    PayloadProfile(profile)
}

impl<S> FromRequestParts<S> for PayloadProfile
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(Ok(announced_profile(parts)))
    }
}

#[cfg(test)]
mod tests {
    use crate::profile::PayloadProfile;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use axum::{extract::FromRequestParts, http::Request};

    /// Extract the profile from a request carrying the given `Link` headers.
    async fn profile_of(links: &[&str]) -> PayloadProfile {
        let mut builder = Request::builder().uri("/ngsi-ld/v1/entities");
        for link in links {
            builder = builder.header("Link", *link);
        }
        let request = builder.body(()).expect("a valid request");
        let (mut parts, ()) = request.into_parts();
        match PayloadProfile::from_request_parts(&mut parts, &()).await {
            Ok(profile) => profile,
            Err(never) => match never {},
        }
    }

    #[tokio::test]
    async fn a_profile_link_beside_the_context_link_announces_its_version() {
        let profile = profile_of(&[
            r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
            r#"<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile""#,
        ])
        .await;
        pretty_assertions::assert_eq!(profile.version(), Some(SpecVersion::V1_8));
    }

    #[tokio::test]
    async fn a_version_the_fallback_tables_do_not_name_snaps_to_the_nearest_supported_one() {
        let profile = profile_of(&[r#"<https://uri.etsi.org/ngsi-ld/profile/1.5>; rel="profile""#]).await;
        pretty_assertions::assert_eq!(profile.version(), Some(SpecVersion::V1_4));
    }

    #[tokio::test]
    async fn a_version_newer_than_the_conformant_one_snaps_to_the_conformant_one() {
        let profile = profile_of(&[r#"<https://uri.etsi.org/ngsi-ld/profile/2.0>; rel="profile""#]).await;
        pretty_assertions::assert_eq!(profile.version(), Some(SpecVersion::CONFORMANT));
    }

    #[tokio::test]
    async fn a_context_link_alone_announces_no_profile() {
        let profile = profile_of(&[r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#]).await;
        pretty_assertions::assert_eq!(profile, PayloadProfile::default());
    }

    #[tokio::test]
    async fn no_link_header_announces_no_profile() {
        pretty_assertions::assert_eq!(profile_of(&[]).await.version(), None);
    }

    #[tokio::test]
    async fn a_malformed_link_is_left_to_the_context_extractor() {
        let profile = profile_of(&["not a link", r#"<https://uri.etsi.org/ngsi-ld/profile/1.3>; rel="profile""#]).await;
        pretty_assertions::assert_eq!(profile.version(), Some(SpecVersion::V1_3));
    }
}
