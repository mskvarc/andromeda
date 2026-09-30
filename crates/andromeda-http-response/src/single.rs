//! NGSI-LD response type for single entities with pre-rendered payload.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    version_negotiation::apply_version_negotiation,
};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_render::response::RenderedOutput;
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for a single entity.
///
/// Constructed from a [`RenderedOutput`] produced by the rendering pipeline:
///
/// ```ignore
/// let host = context_hosting.for_request(&api_root);
/// let rendered = start_rendering(media_type, engine.presented_context(&context).await?, &host, prefer, format)
///     .await?
///     .with_payload(ResponsePayload::Single(entity_value))
///     .render()?;
/// NgsiLdResponse::ok(rendered)
///     .with_location(location)
///     .into_response()
/// ```
pub struct NgsiLdResponse {
    /// HTTP status returned for the resource.
    status: StatusCode,
    /// Pre-rendered NGSI-LD response body and media type.
    rendered: RenderedOutput,
    /// Optional resource location advertised by the response.
    location: Option<String>,
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
    pub const fn new(status: StatusCode, rendered: RenderedOutput) -> NgsiLdResponse {
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
    pub const fn ok(rendered: RenderedOutput) -> NgsiLdResponse {
        NgsiLdResponse::new(StatusCode::OK, rendered)
    }

    /// Set the Location header.
    #[must_use]
    pub fn with_location(mut self, location: String) -> Self {
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
        headers.typed_insert(self.rendered.media_type);
        let status = apply_version_negotiation(self.status, self.rendered.version_adaptation, &mut headers);

        apply_common_headers(
            &mut headers,
            self.rendered.context_link,
            &CommonHeadersOptions {
                location: self.location.as_deref(),
                entity_map_uri: self.entity_map_uri.as_ref(),
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
            },
        );

        if let Some(ref cr) = self.content_range {
            headers.typed_insert(cr.clone());
        }

        (status, headers, Json(self.rendered.value)).into_response()
    }
}
