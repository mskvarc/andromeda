//! NGSI-LD response type for batch operation results with pre-rendered payload.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    version_negotiation::apply_version_negotiation,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::response::RenderedOutput;
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for batch operation results (207 Multi-Status).
pub struct NgsiLdBatchResponse {
    /// HTTP status returned for the batch operation.
    status: StatusCode,
    /// Pre-rendered NGSI-LD response body and media type.
    rendered: RenderedOutput,
    /// Optional snapshot URI advertised by the response.
    snapshot_uri: Option<Uri>,
    /// Distributed-operation warnings emitted as response headers.
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdBatchResponse {
    /// Create a batch response with the given status and pre-rendered data.
    pub const fn new(status: StatusCode, rendered: RenderedOutput) -> NgsiLdBatchResponse {
        NgsiLdBatchResponse {
            status,
            rendered,
            snapshot_uri: None,
            warnings: Vec::new(),
        }
    }
}

impl IntoResponse for NgsiLdBatchResponse {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.typed_insert(self.rendered.media_type);
        let status = apply_version_negotiation(self.status, self.rendered.version_adaptation, &mut headers);

        apply_common_headers(
            &mut headers,
            &self.rendered.context_links,
            CommonHeadersOptions {
                location: None,
                entity_map_uri: None,
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
                context_location: self.rendered.context_location,
            },
        );

        (status, headers, Json(self.rendered.value)).into_response()
    }
}
