//! NGSI-LD response type for batch operation results with pre-rendered payload.

use crate::{
    domain::ngsi_ld_urn::SnapshotId,
    http::{
        header::{ngsild_warning::NgsiLdWarning, preference_applied::PreferenceApplied},
        response::{
            headers::{CommonHeadersOptions, apply_common_headers},
            sonic_json::SonicJson,
        },
    },
    renderer::{VersionAdaptation, response::RenderedOutput},
};
use axum::{
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for batch operation results (207 Multi-Status).
pub struct NgsiLdBatchResponse {
    status: StatusCode,
    rendered: RenderedOutput,
    snapshot_uri: Option<SnapshotId>,
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
        // 203 Non-Authoritative Information when version adaptation was applied
        let status = if self.rendered.version_adaptation == VersionAdaptation::Applied {
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        } else {
            self.status
        };

        let mut headers = HeaderMap::new();
        if let Ok(ct) = self.rendered.content_type.parse() {
            headers.insert(CONTENT_TYPE, ct);
        }

        if self.rendered.version_adaptation == VersionAdaptation::Applied {
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
                context_location: self.rendered.context_location,
            },
        );

        (status, headers, SonicJson(self.rendered.value)).into_response()
    }
}
