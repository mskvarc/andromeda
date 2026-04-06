//! NGSI-LD response type for batch operation results with pre-rendered payload.

use crate::{
    http::{
        header::{ngsild_warning::NgsiLdWarning, preference_applied::PreferenceApplied},
        response::headers::{CommonHeadersOptions, apply_common_headers},
    },
    renderer::response::RenderedOutput,
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use urn::Urn;

/// NGSI-LD response wrapper for batch operation results (207 Multi-Status).
pub struct NgsiLdBatchResponse {
    status: StatusCode,
    rendered: RenderedOutput,
    snapshot_uri: Option<Urn>,
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdBatchResponse {
    /// Create a batch response with the given status and pre-rendered data.
    pub fn new(status: StatusCode, rendered: RenderedOutput) -> NgsiLdBatchResponse {
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
        // 203 Non-Authoritative Information when version adaptation was applied
        let status = if self.rendered.version_adapted {
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        } else {
            self.status
        };

        let mut headers = HeaderMap::new();
        if let Ok(ct) = self.rendered.content_type.parse() {
            headers.insert(CONTENT_TYPE, ct);
        }

        if self.rendered.version_adapted {
            headers.typed_insert(PreferenceApplied::version_conformance());
        }

        apply_common_headers(
            &mut headers,
            &self.rendered.context,
            CommonHeadersOptions {
                location: None,
                entity_map_uri: None,
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
                include_context_in_link: self.rendered.include_context_in_link,
            },
        );

        (status, headers, Json(self.rendered.value)).into_response()
    }
}
