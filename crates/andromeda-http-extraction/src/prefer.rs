//! Prefer header extraction per RFC 7240 and NGSI-LD clause 6.3.6.
//!
//! Extracts the typed `Prefer` header from axum requests and exposes
//! a convenience `PreferHeader` struct for API handlers.

use andromeda_http_headers::{geo_json_body::GeoJsonBodyPreference, prefer::Prefer, preference::RespondMode, version::NgsiLdVersion};
use andromeda_http_response::render::RenderingPreferences;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Parsed Prefer request header per RFC 7240 and NGSI-LD clause 6.3.6.
///
/// An axum-shaped view of the typed [`Prefer`] header: the three tokens a
/// handler acts on, already resolved, so no handler decodes the header itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferHeader {
    /// GeoJSON body preference: whether @context goes in body or Link header.
    pub geo_json_body: GeoJsonBodyPreference,
    /// Requested NGSI-LD version for backwards-compatible payload adaptation.
    pub requested_version: Option<NgsiLdVersion>,
    /// Response-mode hint (`respond-async` / `respond-sync`) per RFC 7240 sec. 4.1.
    /// Used by `/entityOperations` batch endpoints to choose between inline
    /// side-effect processing and the durable queue path.
    pub respond_mode: Option<RespondMode>,
}

impl<S> FromRequestParts<S> for PreferHeader
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let prefer = parts.headers.typed_get::<Prefer>().map_or_else(PreferHeader::default, |p| PreferHeader {
            geo_json_body: p.geo_json_body(),
            requested_version: p.requested_version(),
            respond_mode: p.respond_mode(),
        });
        Ok(prefer)
    }
}

impl PreferHeader {
    /// The subset of preferences consumed by shared response rendering.
    pub const fn rendering_preferences(self) -> RenderingPreferences {
        RenderingPreferences {
            geo_json_body: self.geo_json_body,
            requested_version: self.requested_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn parse_from_header(value: Option<&str>) -> PreferHeader {
        let mut builder = Request::builder().uri("/test");
        if let Some(v) = value {
            builder = builder.header("Prefer", v);
        }
        let req = builder.body(()).expect("valid request");
        let (parts, _) = req.into_parts();
        parts.headers.typed_get::<Prefer>().map_or_else(PreferHeader::default, |p| PreferHeader {
            geo_json_body: p.geo_json_body(),
            requested_version: p.requested_version(),
            respond_mode: p.respond_mode(),
        })
    }

    #[test]
    fn absent_header_returns_defaults() {
        let prefer = parse_from_header(None);
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn body_json_preference() {
        let prefer = parse_from_header(Some("body=json"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        pretty_assertions::assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn body_ld_json_preference() {
        let prefer = parse_from_header(Some("body=ld+json"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn version_1_0() {
        let prefer = parse_from_header(Some("ngsi-ld=1.0"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_0));
    }

    #[test]
    fn version_1_3() {
        let prefer = parse_from_header(Some("ngsi-ld=1.3"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_3));
    }

    #[test]
    fn version_1_9() {
        let prefer = parse_from_header(Some("ngsi-ld=1.9"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_9));
    }

    #[test]
    fn combined_preferences() {
        let prefer = parse_from_header(Some("body=json, ngsi-ld=1.4"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        pretty_assertions::assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_4));
    }

    #[test]
    fn unknown_version_ignored() {
        let prefer = parse_from_header(Some("ngsi-ld=2.5"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        pretty_assertions::assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn unknown_token_ignored() {
        let prefer = parse_from_header(Some("wait=5, body=json"));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        pretty_assertions::assert_eq!(prefer.requested_version, None);
        pretty_assertions::assert_eq!(prefer.respond_mode, None);
    }

    #[test]
    fn respond_async_extracted() {
        let prefer = parse_from_header(Some("respond-async"));
        pretty_assertions::assert_eq!(prefer.respond_mode, Some(RespondMode::Async));
    }

    #[test]
    fn respond_sync_extracted() {
        let prefer = parse_from_header(Some("respond-sync, body=json"));
        pretty_assertions::assert_eq!(prefer.respond_mode, Some(RespondMode::Sync));
        pretty_assertions::assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
    }
}
