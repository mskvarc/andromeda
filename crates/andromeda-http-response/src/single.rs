//! NGSI-LD response type for single resources with a body already written.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    rendered_response::RenderedResponse,
    version_negotiation::apply_version_negotiation,
};
use andromeda_http_headers::{location::Location, ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for a single entity.
///
/// Constructed from a typed body already written into a [`RenderedResponse`]:
///
/// ```ignore
/// let host = context_hosting.for_request(&api_root);
/// let renderer = start_rendering(media_type, engine.presented_context(&context).await?, &host, prefer, format).await?;
/// let rendered = RenderedResponse::resource(renderer.with_body(&resource))?;
/// NgsiLdResponse::ok(rendered)
///     .with_location(location)
///     .into_response()
/// ```
pub struct NgsiLdResponse {
    /// HTTP status returned for the resource.
    status: StatusCode,
    /// Pre-rendered NGSI-LD response body and media type.
    rendered: RenderedResponse,
    /// Optional resource location advertised by the response.
    location: Option<Location>,
    /// Optional entity-map URI advertised by the response.
    entity_map_uri: Option<Identifier>,
    /// Optional pagination range advertised by the response.
    content_range: Option<NgsiLdContentRange>,
    /// Optional snapshot URI advertised by the response.
    snapshot_uri: Option<Identifier>,
    /// Distributed-operation warnings emitted as response headers.
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdResponse {
    /// Create a response with a specific status code.
    #[must_use]
    pub const fn new(status: StatusCode, rendered: RenderedResponse) -> NgsiLdResponse {
        NgsiLdResponse {
            status,
            rendered,
            location: None,
            entity_map_uri: None,
            content_range: None,
            snapshot_uri: None,
            warnings: Vec::new(),
        }
    }

    /// Create a 200 OK response.
    #[must_use]
    pub const fn ok(rendered: RenderedResponse) -> NgsiLdResponse {
        NgsiLdResponse::new(StatusCode::OK, rendered)
    }

    /// Set the Location header.
    #[must_use]
    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// Set the NGSILD-EntityMap header URI.
    #[must_use]
    pub fn with_entity_map_uri(mut self, uri: Identifier) -> Self {
        self.entity_map_uri = Some(uri);
        self
    }

    /// Set NGSILD-Warning headers from distributed forwarding (clause 6.3.17).
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<NgsiLdWarning>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Set the NGSILD-Snapshot header for snapshot-scoped responses (clause 6.3.22).
    #[must_use]
    pub fn with_snapshot_uri(mut self, uri: Identifier) -> Self {
        self.snapshot_uri = Some(uri);
        self
    }

    /// Set the Content-Range header value for temporal pagination.
    #[must_use]
    pub const fn with_content_range(mut self, content_range: NgsiLdContentRange) -> Self {
        self.content_range = Some(content_range);
        self
    }
}

impl IntoResponse for NgsiLdResponse {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        let RenderedResponse { body, head } = self.rendered;
        headers.typed_insert(head.media_type);
        let status = apply_version_negotiation(self.status, head.version_adaptation, &mut headers);

        apply_common_headers(
            &mut headers,
            head.context_link,
            &CommonHeadersOptions {
                location: self.location.as_ref(),
                entity_map_uri: self.entity_map_uri.as_ref(),
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
            },
        );

        if let Some(ref cr) = self.content_range {
            headers.typed_insert(cr.clone());
        }

        (status, headers, body.into_body()).into_response()
    }
}
