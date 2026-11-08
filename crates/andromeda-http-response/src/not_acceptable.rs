//! The body ETSI GS CIM 009 v1.9.1 clause 6.3.4 puts in a `406 Not Acceptable`.
//!
//! Clause 6.3.4 exempts three HTTP-binding precondition failures from the
//! `InvalidRequest` problem the rest of the clause raises. Two of them are
//! bodyless: an absent `Content-Length` results in "just a 411 HTTP status
//! code (without any payload body)" and an unsupported `Content-Type` in "just
//! a 415 HTTP status code (without any payload body)". The third is not:
//! "Not Acceptable Media Type, i.e. 'Accept' header does not imply
//! 'application/json' nor 'application/ld+json', shall result in a 406 HTTP
//! status code and the body of the message shall contain the list of the
//! available representations of the resources". The same sentence closes the
//! clause's `application/geo+json` rule: for every operation other than
//! Retrieve Entity and Query Entity, "the request will result in a Not
//! Acceptable Media Type error, returning a 406 HTTP status code and the body
//! of the message shall contain the list of the available representations of
//! the resources".
//!
//! The list is the refusing endpoint's own. Clause 6.3.15 gives no `GeoJSON`
//! form to the temporal, subscription, registration or discovery resources, so
//! their `406` must not advertise one: a client that retried with the
//! advertised media type would be refused again.

use andromeda_http_headers::media_type::{GeoJsonSupport, ResponseMediaType};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// The representations one endpoint can answer with, as its `406` publishes them.
///
/// Carrying the endpoint's [`GeoJsonSupport`] rather than a list of media types
/// keeps the advertised list and the list content negotiation actually selects
/// from as one value: an endpoint cannot negotiate over one set and advertise
/// another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvailableRepresentations {
    /// Whether the endpoint that refused the request answers `application/geo+json`.
    geo_json: GeoJsonSupport,
}

impl AvailableRepresentations {
    /// The representations advertised by an endpoint with this `GeoJSON` support.
    #[must_use]
    pub const fn of(geo_json: GeoJsonSupport) -> AvailableRepresentations {
        AvailableRepresentations { geo_json }
    }

    /// The advertised representations, in the significant order of clause 6.3.4.
    #[must_use]
    pub const fn media_types(self) -> &'static [ResponseMediaType] {
        self.geo_json.available_representations()
    }
}

/// The JSON object a `406` carries.
///
/// Clause 6.3.4 mandates the content of the body, "the list of the available
/// representations of the resources", and not a name for it, so the list is
/// published under a member rather than as a bare array: a bare array of media
/// types is indistinguishable in shape from the array of entities the same
/// endpoint answers a query with. The member is camelCased like every other
/// member crossing the NGSI-LD API boundary. Clause 6.3.3 fixes the media type
/// of an error body as `application/json`, which [`Json`] sets.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotAcceptableBody {
    /// The media types the refusing endpoint can answer with.
    available_representations: Vec<&'static str>,
}

impl IntoResponse for AvailableRepresentations {
    fn into_response(self) -> Response {
        let body = NotAcceptableBody {
            available_representations: self.media_types().iter().map(ResponseMediaType::as_str).collect(),
        };
        (StatusCode::NOT_ACCEPTABLE, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use crate::not_acceptable::AvailableRepresentations;
    use andromeda_http_headers::media_type::GeoJsonSupport;
    use axum::{
        body::to_bytes,
        http::{StatusCode, header::CONTENT_TYPE},
        response::IntoResponse,
    };
    use serde_json::{Value, json};
    use std::io::Error as IoError;

    /// Render a `406` and decode the body clause 6.3.4 requires of it.
    async fn rendered(representations: AvailableRepresentations) -> Result<(StatusCode, Option<String>, Value), serde_json::Error> {
        let response = representations.into_response();
        let status = response.status();
        let content_type = response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(str::to_owned);
        let value = match to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) => return Err(serde_json::Error::io(IoError::other(error))),
        };
        Ok((status, content_type, value))
    }

    /// Clause 6.3.4: the `406` body "shall contain the list of the available
    /// representations of the resources", and clause 6.3.3 sends error bodies
    /// as `application/json`.
    #[tokio::test]
    async fn a_geojson_endpoint_publishes_all_three_representations() -> Result<(), serde_json::Error> {
        let (status, content_type, value) = rendered(AvailableRepresentations::of(GeoJsonSupport::Allowed)).await?;

        pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
        pretty_assertions::assert_eq!(content_type.as_deref(), Some("application/json"));
        pretty_assertions::assert_eq!(
            value,
            json!({"availableRepresentations": ["application/json", "application/ld+json", "application/geo+json"]})
        );
        Ok(())
    }

    /// Clause 6.3.15 gives no `GeoJSON` form to the non-entity resources, so
    /// their refusal must not advertise one.
    #[tokio::test]
    async fn an_endpoint_without_geojson_publishes_only_the_two_json_representations() -> Result<(), serde_json::Error> {
        let (status, _, value) = rendered(AvailableRepresentations::of(GeoJsonSupport::Disallowed)).await?;

        pretty_assertions::assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
        pretty_assertions::assert_eq!(value, json!({"availableRepresentations": ["application/json", "application/ld+json"]}));
        Ok(())
    }

    /// The advertised list is the negotiable list: what a `406` publishes is
    /// exactly what [`GeoJsonSupport`] would have let the endpoint select.
    #[test]
    fn the_published_list_is_the_endpoints_own_negotiable_list() {
        for support in [GeoJsonSupport::Allowed, GeoJsonSupport::Disallowed] {
            pretty_assertions::assert_eq!(AvailableRepresentations::of(support).media_types(), support.available_representations());
        }
    }
}
