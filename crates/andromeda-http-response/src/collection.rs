//! NGSI-LD response type for entity collections with pre-rendered payload.

use crate::{
    headers::{CommonHeadersOptions, apply_common_headers},
    sonic_json::SonicJson,
};
use andromeda_http_headers::{ngsild_results_count::NgsiLdResultsCount, ngsild_warning::NgsiLdWarning, preference_applied::PreferenceApplied};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::{response::RenderedOutput, version::VersionAdaptation};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
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
    content_range: Option<String>,
    /// Optional entity-map URI advertised by the response.
    entity_map_uri: Option<Uri>,
    /// Optional snapshot URI advertised by the response.
    snapshot_uri: Option<Uri>,
    /// Distributed-operation warnings emitted as response headers.
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdCollectionResponse {
    /// Create a 200 OK response for a collection.
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
    pub const fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Set the total count for pagination (NGSILD-Results-Count header).
    pub const fn with_total_count(mut self, count: usize) -> Self {
        self.total_count = Some(count);
        self
    }

    /// Set the Content-Range header value for temporal pagination.
    pub fn with_content_range(mut self, content_range: String) -> Self {
        self.content_range = Some(content_range);
        self
    }

    /// Set the NGSILD-EntityMap header URI.
    pub fn with_entity_map_uri(mut self, uri: Uri) -> Self {
        self.entity_map_uri = Some(uri);
        self
    }

    /// Set NGSILD-Warning headers from distributed forwarding (clause 6.3.17).
    pub fn with_warnings(mut self, warnings: Vec<NgsiLdWarning>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Set the NGSILD-Snapshot header for snapshot-scoped responses (clause 6.3.22).
    pub fn with_snapshot_uri(mut self, uri: Uri) -> Self {
        self.snapshot_uri = Some(uri);
        self
    }
}

impl IntoResponse for NgsiLdCollectionResponse {
    fn into_response(self) -> Response {
        // 203 Non-Authoritative Information when version adaptation was applied
        let status = if self.rendered.version_adaptation == VersionAdaptation::Applied {
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        } else {
            self.status
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(self.rendered.media_type.as_str()));

        if let Some(count) = self.total_count {
            headers.typed_insert(NgsiLdResultsCount::new(count));
        }

        if let Some(ref cr) = self.content_range
            && let Ok(val) = HeaderValue::from_str(cr)
        {
            headers.insert("Content-Range", val);
        }

        if self.rendered.version_adaptation == VersionAdaptation::Applied {
            headers.typed_insert(PreferenceApplied::version_conformance());
        }

        apply_common_headers(
            &mut headers,
            &self.rendered.context_links,
            CommonHeadersOptions {
                location: None,
                entity_map_uri: self.entity_map_uri.as_ref(),
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
                context_location: self.rendered.context_location,
            },
        );

        (status, headers, SonicJson(self.rendered.value)).into_response()
    }
}
