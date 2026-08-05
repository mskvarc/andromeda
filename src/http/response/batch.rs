//! NGSI-LD response type for batch operation results with pre-rendered payload.

use crate::http::response::{
    headers::{CommonHeadersOptions, apply_common_headers},
    sonic_json::SonicJson,
};
use andromeda_ngsi_ld_http_headers::{ngsild_warning::NgsiLdWarning, preference_applied::PreferenceApplied};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::SnapshotId;
use andromeda_ngsi_ld_render::{response::RenderedOutput, version::VersionAdaptation};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
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
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(self.rendered.media_type.as_str()));

        if self.rendered.version_adaptation == VersionAdaptation::Applied {
            headers.typed_insert(PreferenceApplied::version_conformance());
        }

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

        (status, headers, SonicJson(self.rendered.value)).into_response()
    }
}
