//! Composite extractor for NGSI-LD protocol headers.
//!
//! Bundles the language, preference, payload profile, snapshot, and Via
//! headers into a single extractor to reduce handler parameter counts (ETSI GS
//! CIM 009 clauses 6.3.5, 6.3.6, 6.3.18, 6.3.21, 6.3.22).

use crate::{language::LanguageNegotiation, prefer::PreferHeader, profile::PayloadProfile, snapshot::SnapshotHeader, via::IncomingVia};
use andromeda_http_headers::{
    language::LanguageFilter,
    ngsild_dataset_id::NgsiLdDatasetId,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_join::NgsiLdJoin,
    ngsild_join_level::NgsiLdJoinLevel,
    ngsild_response_format::NgsiLdResponseFormat,
    ngsild_scope::NgsiLdScope,
    via::Via,
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::protocol::ProtocolExtensions;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Composite extractor for NGSI-LD protocol-level request headers.
///
/// Extracts language negotiation, Prefer header, the `Link` version profile,
/// NGSILD-Snapshot header, NGSILD-EntityMap header, and the Via header in a
/// single pass.
pub struct NgsiLdProtocolHeaders {
    /// Language filter from the `NGSILD-Language` fallback header.
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
    /// Optional scope query expression from `NGSILD-Scope` extension header.
    pub scope: Option<NgsiLdScope>,
    /// Optional response format token from `NGSILD-Response-Format` extension header.
    pub response_format: Option<NgsiLdResponseFormat>,
    /// Optional dataset filter from `NGSILD-Dataset-Id` extension header.
    pub dataset_id: Option<NgsiLdDatasetId>,
    /// Optional join mode from `NGSILD-Join` extension header.
    pub join: Option<NgsiLdJoin>,
    /// Optional join depth from `NGSILD-Join-Level` extension header.
    pub join_level: Option<NgsiLdJoinLevel>,
}

impl NgsiLdProtocolHeaders {
    /// The clause 6.3.x extension headers, handed to the request model as the
    /// typed values they decoded to.
    ///
    /// The header crate already parsed and validated each one, so this passes
    /// the typed values straight through -- the scope, the format and the
    /// dataset ids by reference, the join by its owner enum -- rather than
    /// re-stringifying them for the merge to re-parse.
    pub fn extensions(&self) -> ProtocolExtensions<'_> {
        ProtocolExtensions {
            scope: self.scope.as_ref(),
            response_format: self.response_format,
            dataset_ids: self.dataset_id.as_ref(),
            join: self.join.map(NgsiLdJoin::into_inner),
            join_level: self.join_level.map(NgsiLdJoinLevel::into_inner),
        }
    }
}

impl<S> FromRequestParts<S> for NgsiLdProtocolHeaders
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<NgsiLdProtocolHeaders, Self::Rejection> {
        let uri = parts.uri.clone();
        let LanguageNegotiation(language_filter) = LanguageNegotiation::from_request_parts(parts, state).await?;
        // PreferHeader is infallible (Rejection = Infallible), so unwrap via match.
        let prefer = match PreferHeader::from_request_parts(parts, state).await {
            Ok(p) => p,
            Err(never) => match never {},
        };
        // PayloadProfile is infallible, so unwrap via match.
        let payload_profile = match PayloadProfile::from_request_parts(parts, state).await {
            Ok(profile) => profile,
            Err(never) => match never {},
        };
        let snapshot = SnapshotHeader::from_request_parts(parts, state).await?;
        // IncomingVia is infallible, so unwrap via match.
        let incoming_via = match IncomingVia::from_request_parts(parts, state).await {
            Ok(v) => v.into_inner(),
            Err(never) => match never {},
        };
        let entity_map: Option<NgsiLdEntityMap> = {
            let mut values = parts.headers.get_all("ngsild-entitymap").iter();
            headers::Header::decode(&mut values).ok()
        };
        let scope = parts
            .headers
            .typed_try_get::<NgsiLdScope>()
            .map_err(|_| ExtractorRejection::bad_request_data("NGSILD-Scope header has invalid syntax", uri.clone()))?;
        let response_format = parts
            .headers
            .typed_try_get::<NgsiLdResponseFormat>()
            .map_err(|_| ExtractorRejection::bad_request_data("NGSILD-Response-Format header has invalid value", uri.clone()))?;
        let dataset_id = parts
            .headers
            .typed_try_get::<NgsiLdDatasetId>()
            .map_err(|_| ExtractorRejection::bad_request_data("NGSILD-Dataset-Id header must contain URN values", uri.clone()))?;
        let join = parts
            .headers
            .typed_try_get::<NgsiLdJoin>()
            .map_err(|_| ExtractorRejection::bad_request_data("NGSILD-Join header has invalid value", uri.clone()))?;
        let join_level = parts
            .headers
            .typed_try_get::<NgsiLdJoinLevel>()
            .map_err(|_| ExtractorRejection::bad_request_data("NGSILD-Join-Level header must be an unsigned integer", uri.clone()))?;

        Ok(NgsiLdProtocolHeaders {
            language_filter,
            prefer,
            payload_profile,
            snapshot,
            incoming_via,
            entity_map,
            scope,
            response_format,
            dataset_id,
            join,
            join_level,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use axum::http::Request;

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
        assert!(headers.scope.is_none());
        assert!(headers.response_format.is_none());
        assert!(headers.dataset_id.is_none());
        assert!(headers.join.is_none());
        assert!(headers.join_level.is_none());
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

    #[tokio::test]
    async fn rejects_invalid_dataset_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("ngsild-dataset-id", "invalid")
            .body(())
            .unwrap();
        let (mut parts, ()) = req.into_parts();
        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
