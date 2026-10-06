//! NGSI-LD response type for batch operation results with a body already written.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    rendered_response::RenderedResponse,
    version_negotiation::apply_version_negotiation,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for batch operation results (207 Multi-Status).
pub struct NgsiLdBatchResponse {
    /// HTTP status returned for the batch operation.
    status: StatusCode,
    /// The written NGSI-LD response body and its head.
    rendered: RenderedResponse,
    /// Optional snapshot URI advertised by the response.
    snapshot_uri: Option<Identifier>,
    /// Distributed-operation warnings emitted as response headers.
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdBatchResponse {
    /// Create a batch response with the given status and written body.
    #[must_use]
    pub const fn new(status: StatusCode, rendered: RenderedResponse) -> NgsiLdBatchResponse {
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
        let RenderedResponse { body, head } = self.rendered;
        headers.typed_insert(head.media_type);
        let status = apply_version_negotiation(self.status, head.version_adaptation, &mut headers);

        apply_common_headers(
            &mut headers,
            head.context_link,
            &CommonHeadersOptions {
                location: None,
                entity_map_uri: None,
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
            },
        );

        (status, headers, body.into_body()).into_response()
    }
}
