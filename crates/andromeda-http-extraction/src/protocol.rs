//! Composite extractor for NGSI-LD protocol headers.
//!
//! Bundles the language, preference, payload profile, snapshot, entity map and
//! Via headers into a single extractor to reduce handler parameter counts
//! (ETSI GS CIM 009 clauses 5.5.9.3, 6.3.5, 6.3.6, 6.3.18, 6.3.21, 6.3.22).

use crate::{
    addressed_uri::addressed_uri,
    language::LanguageNegotiation,
    prefer::PreferHeader,
    profile::PayloadProfile,
    snapshot::SnapshotHeader,
    via::IncomingVia,
};
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, via::Via};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use thiserror::Error;

/// Composite extractor for NGSI-LD protocol-level request headers.
///
/// Extracts language negotiation, Prefer header, the `Link` version profile,
/// NGSILD-Snapshot header, NGSILD-EntityMap header, and the Via header in a
/// single pass.
pub struct NgsiLdProtocolHeaders {
    /// Language filter from an accepted `Accept-Language`; a `lang` parameter
    /// outranks it.
    pub language_filter: Option<LanguageFilter>,
    /// Parsed Prefer header (body format, version negotiation).
    pub prefer: PreferHeader,
    /// The NGSI-LD version the request payload announces through a profile
    /// `Link` (clause 6.3.21). Informational for this broker: it interprets
    /// every version the clause 4.3.6.8 tables describe, so no payload needs
    /// amending to be accepted, and a profile never changes the response.
    pub payload_profile: PayloadProfile,
    /// Optional snapshot identifier from `NGSILD-Snapshot` header.
    pub snapshot: SnapshotHeader,
    /// Incoming `Via` header value for forwarding loop detection (clause 6.3.18).
    pub incoming_via: Option<Via>,
    /// Optional entity map identifier from `NGSILD-EntityMap` header (clause 5.5.9.3).
    pub entity_map: Option<NgsiLdEntityMap>,
}

/// An `NGSILD-EntityMap` header whose value is not a URI.
#[derive(Debug, Error)]
#[error("the ngsild-entitymap header is not a uri")]
struct InvalidEntityMapHeader(#[source] headers::Error);

/// The entity map the request names, refusing a present header that is not a
/// URI (Table 6.4.3.2-2) as `BadRequestData` rather than reading it as absent.
fn entity_map_header(parts: &Parts) -> Result<Option<NgsiLdEntityMap>, ExtractorRejection> {
    parts
        .headers
        .typed_try_get::<NgsiLdEntityMap>()
        .map_err(|error| ExtractorRejection::bad_request_data(InvalidEntityMapHeader(error).to_string(), addressed_uri(parts)))
}

impl<S> FromRequestParts<S> for NgsiLdProtocolHeaders
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<NgsiLdProtocolHeaders, Self::Rejection> {
        // LanguageNegotiation is infallible, so unwrap via match.
        let LanguageNegotiation(language_filter) = match LanguageNegotiation::from_request_parts(parts, state).await {
            Ok(negotiation) => negotiation,
            Err(never) => match never {},
        };
        // PreferHeader is infallible (Rejection = Infallible), so unwrap via match.
        let prefer = match PreferHeader::from_request_parts(parts, state).await {
            Ok(p) => p,
            Err(never) => match never {},
        };
        let payload_profile = PayloadProfile::from_request_parts(parts, state).await?;
        let snapshot = SnapshotHeader::from_request_parts(parts, state).await?;
        // IncomingVia is infallible, so unwrap via match.
        let incoming_via = match IncomingVia::from_request_parts(parts, state).await {
            Ok(v) => v.into_inner(),
            Err(never) => match never {},
        };
        let entity_map = entity_map_header(parts)?;

        Ok(NgsiLdProtocolHeaders {
            language_filter,
            prefer,
            payload_profile,
            snapshot,
            incoming_via,
            entity_map,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::NgsiLdProtocolHeaders;
    use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use axum::{
        extract::FromRequestParts,
        http::{HeaderValue, Request},
    };

    #[tokio::test]
    async fn extracts_defaults_with_no_headers() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let headers = result.unwrap();
        assert!(headers.language_filter.is_none());
        assert!(headers.payload_profile.version().is_none());
        assert!(headers.incoming_via.is_none());
        assert!(headers.entity_map.is_none());
    }

    #[tokio::test]
    async fn leaves_query_language_to_the_request_model() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities?lang=en").body(()).unwrap();
        let (mut parts, ()) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let headers = result.unwrap();
        assert!(headers.language_filter.is_none());
    }

    /// Clause 6.3.21: a profile `Link` rides beside the context `Link` and is
    /// read as the payload's version, not as a response preference.
    #[tokio::test]
    async fn reads_the_payload_profile_beside_the_context_link() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("link", r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
            .header("link", r#"<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile""#)
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();

        let headers = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await.expect("headers extract");
        pretty_assertions::assert_eq!(headers.payload_profile.version(), Some(SpecVersion::V1_8));
        pretty_assertions::assert_eq!(headers.prefer.requested_version, None);
    }

    /// The protocol headers a request carrying one `NGSILD-EntityMap` value
    /// extracts to.
    async fn with_entity_map(value: &str) -> Result<NgsiLdProtocolHeaders, ExtractorRejection> {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("ngsild-entitymap", value)
            .body(())
            .expect("valid request");
        let (mut parts, ()) = req.into_parts();
        NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn an_entity_map_urn_is_extracted() {
        let headers = with_entity_map("urn:ngsi-ld:EntityMap:abc").await.expect("headers extract");
        pretty_assertions::assert_eq!(headers.entity_map.as_ref().map(NgsiLdEntityMap::as_str), Some("urn:ngsi-ld:EntityMap:abc"));
    }

    /// Table 6.4.3.2-2 makes the header a URI: an empty value, a bare word and
    /// a value outside visible ASCII each name no entity map.
    #[tokio::test]
    async fn an_entity_map_header_that_is_not_a_uri_is_bad_request_data_naming_the_header() {
        let non_ascii = HeaderValue::from_bytes("urn:ngsi-ld:EntityMap:caf\u{e9}".as_bytes()).expect("opaque header bytes");
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("ngsild-entitymap", non_ascii)
            .body(())
            .expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let non_ascii_result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;

        for (value, result) in [
            ("", with_entity_map("").await),
            ("not-a-uri", with_entity_map("not-a-uri").await),
            ("non-ascii", non_ascii_result),
        ] {
            match result {
                Err(ExtractorRejection::BadRequestData { detail, instance }) => {
                    pretty_assertions::assert_eq!(detail, "the ngsild-entitymap header is not a uri", "{value}");
                    pretty_assertions::assert_eq!(instance, "/ngsi-ld/v1/entities", "{value}");
                }
                Err(other) => panic!("{value}: refused as {other:?}"),
                Ok(_) => panic!("{value}: extracted as an entity map"),
            }
        }
    }

    #[tokio::test]
    async fn rejects_invalid_snapshot_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("ngsild-snapshot", "not-a-valid-urn")
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
