//! Shared header logic for NGSI-LD responses.

use andromeda_http_headers::{
    link::header::Link,
    location::Location,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_snapshot::NgsiLdSnapshot,
    ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::context::ContextLocation;
use axum::{http::HeaderMap, response::Response};
use headers::HeaderMapExt;
use iri_rs::IriRefBuf;

/// Options for applying common NGSI-LD headers to a response.
pub struct CommonHeadersOptions<'a> {
    /// `Location` header value for created resources.
    pub location: Option<&'a str>,
    /// NGSILD-EntityMap header URI for entity map scoping (clause 5.7.3.4).
    pub entity_map_uri: Option<&'a Uri>,
    /// NGSILD-Snapshot header URN for snapshot-scoped responses (clause 6.3.22).
    pub snapshot_uri: Option<&'a Uri>,
    /// NGSILD-Warning header values from distributed forwarding (clause 6.3.17).
    pub warnings: &'a [NgsiLdWarning],
    /// Where the JSON-LD `@context` should appear: in the `Link` header or in the body.
    pub context_location: ContextLocation,
}

/// Apply common NGSI-LD headers to a `HeaderMap`.
///
/// `context_links` is what the rendering already resolved; it is advertised
/// only when the rendering put the `@context` in the headers rather than in
/// the body, which is what `options.context_location` reports.
pub fn apply_common_headers(headers: &mut HeaderMap, context_links: &[IriRefBuf], options: &CommonHeadersOptions<'_>) {
    if let Some(loc) = options.location.and_then(Location::new) {
        headers.typed_insert(loc);
    }

    if let Some(id) = options.entity_map_uri {
        headers.typed_insert(NgsiLdEntityMap::new(id.clone()));
    }

    if let Some(snap_urn) = options.snapshot_uri {
        headers.typed_insert(NgsiLdSnapshot::new(snap_urn.clone()));
    }

    if !options.warnings.is_empty() {
        headers.typed_insert(NgsiLdWarningHeader::new(options.warnings.to_vec()));
    }

    if options.context_location == ContextLocation::LinkHeader {
        Link::insert_jsonld_contexts(headers, context_links.iter().cloned());
    }
}

/// Attach a validated `Location` header to an existing response.
pub fn with_location(mut response: Response, location: impl AsRef<str>) -> Response {
    if let Some(location) = Location::new(location) {
        response.headers_mut().typed_insert(location);
    }
    response
}
