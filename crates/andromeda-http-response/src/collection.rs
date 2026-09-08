//! NGSI-LD response type for entity collections with pre-rendered payload.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    version_negotiation::apply_version_negotiation,
};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_results_count::NgsiLdResultsCount, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::response::RenderedOutput;
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for a collection of entities.
///
/// Constructed from a [`RenderedOutput`] produced by the rendering pipeline:
///
/// ```ignore
/// let rendered = start_rendering(format, &context, prefer)
///     .with_payload(ResponsePayload::Collection(entity_values))
///     .render()?;
/// NgsiLdCollectionResponse::ok(rendered)
///     .with_total_count(count)
///     .into_response()
/// ```
pub struct NgsiLdCollectionResponse {
    /// HTTP status returned for the collection.
    status: StatusCode,
    /// Pre-rendered NGSI-LD response body and media type.
    rendered: RenderedOutput,
    /// Optional total collection count advertised by the response.
    total_count: Option<usize>,
    /// Optional pagination range advertised by the response.
    content_range: Option<NgsiLdContentRange>,
    /// Optional entity-map URI advertised by the response.
    entity_map_uri: Option<Uri>,
    /// Optional snapshot URI advertised by the response.
    snapshot_uri: Option<Uri>,
    /// Distributed-operation warnings emitted as response headers.
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdCollectionResponse {
    /// Create a 200 OK response for a collection.
    #[must_use]
    pub const fn ok(rendered: RenderedOutput) -> NgsiLdCollectionResponse {
        NgsiLdCollectionResponse {
            status: StatusCode::OK,
            rendered,
            total_count: None,
            content_range: None,
            entity_map_uri: None,
            snapshot_uri: None,
            warnings: Vec::new(),
        }
    }

    /// Override the HTTP status code.
    #[must_use]
    pub const fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Set the total count for pagination (NGSILD-Results-Count header).
    #[must_use]
    pub const fn with_total_count(mut self, count: usize) -> Self {
        self.total_count = Some(count);
        self
    }

    /// Set the Content-Range header value for temporal pagination.
    #[must_use]
    pub const fn with_content_range(mut self, content_range: NgsiLdContentRange) -> Self {
        self.content_range = Some(content_range);
        self
    }

    /// Set the NGSILD-EntityMap header URI.
    #[must_use]
    pub fn with_entity_map_uri(mut self, uri: Uri) -> Self {
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
    pub fn with_snapshot_uri(mut self, uri: Uri) -> Self {
        self.snapshot_uri = Some(uri);
        self
    }
}

impl IntoResponse for NgsiLdCollectionResponse {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.typed_insert(self.rendered.media_type);
        let status = apply_version_negotiation(self.status, self.rendered.version_adaptation, &mut headers);

        if let Some(count) = self.total_count {
            headers.typed_insert(NgsiLdResultsCount::new(count));
        }

        if let Some(ref cr) = self.content_range {
            headers.typed_insert(cr.clone());
        }

        apply_common_headers(
            &mut headers,
            &self.rendered.context_links,
            &CommonHeadersOptions {
                location: None,
                entity_map_uri: self.entity_map_uri.as_ref(),
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
                context_location: self.rendered.context_location,
            },
        );

        (status, headers, Json(self.rendered.value)).into_response()
    }
}
