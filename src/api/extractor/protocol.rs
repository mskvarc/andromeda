//! Composite extractor for NGSI-LD protocol headers.
//!
//! Bundles the language, preference, snapshot, and Via headers into a single
//! extractor to reduce handler parameter counts (ETSI GS CIM 009 clauses
//! 6.3.5, 6.3.6, 6.3.18, 6.3.22).

use crate::{
    api::extractor::{error::ExtractorRejection, language::LanguageNegotiation, prefer::PreferHeader, snapshot::SnapshotHeader},
    query::lang::LanguageFilter,
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
    pub incoming_via: Option<String>,
    /// Optional entity map identifier from `NGSILD-EntityMap` header (clause 5.5.9.3).
    pub entity_map: Option<String>,
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
        let incoming_via = parts.headers.get("via").and_then(|v| v.to_str().ok()).map(String::from);
        let entity_map = parts.headers.get("NGSILD-EntityMap").and_then(|v| v.to_str().ok()).map(String::from);

        Ok(NgsiLdProtocolHeaders {
            language_filter,
            prefer,
            snapshot,
            incoming_via,
            entity_map,
        })
    }
}
