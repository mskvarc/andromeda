//! NGSI-LD response type for single entities with pre-rendered payload.

use crate::http::response::{
    headers::{CommonHeadersOptions, apply_common_headers},
    sonic_json::SonicJson,
};
use andromeda_ngsi_ld_http_headers::{ngsild_warning::NgsiLdWarning, preference_applied::PreferenceApplied};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::{response::RenderedOutput, version::VersionAdaptation};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// NGSI-LD response wrapper for a single entity.
///
/// Constructed from a [`RenderedOutput`] produced by the rendering pipeline:
///
/// ```ignore
/// let rendered = start_rendering(format, &context, prefer)
///     .with_payload(ResponsePayload::Single(entity_value))
///     .render()?;
/// NgsiLdResponse::ok(rendered)
///     .with_location(location)
///     .into_response()
/// ```
pub struct NgsiLdResponse {
    status: StatusCode,
    rendered: RenderedOutput,
    location: Option<String>,
    entity_map_uri: Option<Uri>,
    content_range: Option<String>,
    snapshot_uri: Option<Uri>,
    warnings: Vec<NgsiLdWarning>,
}

impl NgsiLdResponse {
    /// Create a response with a specific status code.
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
    pub const fn ok(rendered: RenderedOutput) -> NgsiLdResponse {
        NgsiLdResponse::new(StatusCode::OK, rendered)
    }

    /// Set the Location header.
    pub fn with_location(mut self, location: String) -> Self {
        self.location = Some(location);
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

    /// Set the Content-Range header value for temporal pagination.
    pub fn with_content_range(mut self, content_range: String) -> Self {
        self.content_range = Some(content_range);
        self
    }
}

impl IntoResponse for NgsiLdResponse {
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
                location: self.location.as_deref(),
                entity_map_uri: self.entity_map_uri.as_ref(),
                snapshot_uri: self.snapshot_uri.as_ref(),
                warnings: &self.warnings,
                context_location: self.rendered.context_location,
            },
        );

        if let Some(ref cr) = self.content_range
            && let Ok(val) = HeaderValue::from_str(cr)
        {
            headers.insert("Content-Range", val);
        }

        (status, headers, SonicJson(self.rendered.value)).into_response()
    }
}
