//! Composite extractor for NGSI-LD protocol headers.
//!
//! Bundles the language, preference, snapshot, and Via headers into a single
//! extractor to reduce handler parameter counts (ETSI GS CIM 009 clauses
//! 6.3.5, 6.3.6, 6.3.18, 6.3.22).

use crate::http::{
    extractor::{error::ExtractorRejection, language::LanguageNegotiation, prefer::PreferHeader, snapshot::SnapshotHeader, via::IncomingVia},
    header::{ngsild_entity_map::NgsiLdEntityMap, via::Via},
    model::language::LanguageFilter,
};
use axum::{extract::FromRequestParts, http::request::Parts};

/// Composite extractor for NGSI-LD protocol-level request headers.
///
/// Extracts language negotiation, Prefer header, NGSILD-Snapshot header,
/// NGSILD-EntityMap header, and the Via header in a single pass.
pub struct NgsiLdProtocolHeaders {
    /// Resolved language filter from `lang` query param or `NGSILD-Language` header.
    pub language_filter: Option<LanguageFilter>,
    /// Parsed Prefer header (body format, version negotiation).
    pub prefer: PreferHeader,
    /// Optional snapshot identifier from `NGSILD-Snapshot` header.
    pub snapshot: SnapshotHeader,
    /// Incoming `Via` header value for forwarding loop detection (clause 6.3.18).
    pub incoming_via: Option<Via>,
    /// Optional entity map identifier from `NGSILD-EntityMap` header (clause 5.5.9.3).
    pub entity_map: Option<NgsiLdEntityMap>,
}

impl<S> FromRequestParts<S> for NgsiLdProtocolHeaders
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<NgsiLdProtocolHeaders, Self::Rejection> {
        let LanguageNegotiation(language_filter) = LanguageNegotiation::from_request_parts(parts, state).await?;
        // PreferHeader is infallible (Rejection = Infallible), so unwrap via match.
        let prefer = match PreferHeader::from_request_parts(parts, state).await {
            Ok(p) => p,
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

        Ok(NgsiLdProtocolHeaders {
            language_filter,
            prefer,
            snapshot,
            incoming_via,
            entity_map,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn extracts_defaults_with_no_headers() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let headers = result.unwrap();
        assert!(headers.language_filter.is_none());
        assert!(headers.incoming_via.is_none());
        assert!(headers.entity_map.is_none());
    }

    #[tokio::test]
    async fn extracts_language_from_query_param() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities?lang=en").body(()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        let headers = result.unwrap();
        assert!(headers.language_filter.is_some());
    }

    #[tokio::test]
    async fn rejects_invalid_snapshot_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("ngsild-snapshot", "not-a-valid-urn")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = NgsiLdProtocolHeaders::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
