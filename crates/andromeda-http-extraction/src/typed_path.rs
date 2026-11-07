//! Typed route parameters with NGSI-LD problem-detail rejections.

use crate::{
    addressed_uri::addressed_uri,
    path_segment::{InvalidPathSegment, PathSegments},
};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::{FromRequestParts, Path, rejection::PathRejection},
    http::request::Parts,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

/// The route parameters of a request, each parsed once into its domain type.
///
/// Axum percent-decodes the segments; each is then read through its
/// [`crate::path_segment::PathSegment`], so a refusal names the specification
/// parameter at fault. Every failure is the clause 5.5.2 `BadRequestData`
/// problem, naming the complete request URI as its `instance`.
pub struct TypedPath<T>(pub T);

impl<S, T> FromRequestParts<S> for TypedPath<T>
where
    S: Send + Sync,
    T: PathSegments + Send,
    T::Raw: DeserializeOwned + Send,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<TypedPath<T>, ExtractorRejection> {
        let Path(raw) = Path::<T::Raw>::from_request_parts(parts, state)
            .await
            .map_err(|rejection| PathRefusal::Undecodable(rejection).into_rejection(addressed_uri(parts)))?;
        T::read_segments(raw)
            .map(TypedPath)
            .map_err(|refusal| PathRefusal::Segment(refusal).into_rejection(addressed_uri(parts)))
    }
}

/// Why the route parameters of a request are refused.
#[derive(Debug, Error)]
enum PathRefusal {
    /// The router could not hand the segments over as text.
    #[error("the request path does not decode: {0}")]
    Undecodable(#[source] PathRejection),
    /// A segment is not a value of its parameter's type.
    #[error(transparent)]
    Segment(#[from] InvalidPathSegment),
}

impl RequestRefusal for PathRefusal {
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            PathRefusal::Undecodable(_) | PathRefusal::Segment(_) => NgsiLdErrorType::BadRequestData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        path_segment::{AttrId, EntityId, InstanceId},
        typed_path::TypedPath,
    };
    use andromeda_ngsi_ld_lexicon::{identifier_length::IDENTIFIER_BYTE_LIMIT, uri::UriView};
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
        routing::{MethodRouter, get},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn uri_value(TypedPath(EntityId(id)): TypedPath<EntityId>) -> String {
        match id.view() {
            UriView::Urn(value) => format!("urn:{value}"),
            UriView::Url(value) => format!("url:{value}"),
            UriView::Iri(value) => format!("iri:{value}"),
        }
    }

    async fn attribute_value(TypedPath(AttrId(attribute)): TypedPath<AttrId>) -> String {
        attribute.to_string()
    }

    async fn instance_value(TypedPath(InstanceId(instance)): TypedPath<InstanceId>) -> String {
        instance.to_string()
    }

    async fn attribute_of_entity(TypedPath((EntityId(_id), AttrId(attribute))): TypedPath<(EntityId, AttrId)>) -> String {
        attribute.to_string()
    }

    async fn response(path: &str, route: &str, handler: MethodRouter) -> Response {
        Router::new()
            .route(route, handler)
            .oneshot(Request::builder().uri(path).body(Body::empty()).expect("valid test request"))
            .await
            .expect("router responds")
    }

    async fn body_text(response: Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), usize::MAX).await.expect("body bytes").to_vec()).expect("UTF-8 body")
    }

    async fn problem(response: Response) -> Value {
        serde_json::from_str(&body_text(response).await).expect("problem detail JSON")
    }

    #[tokio::test]
    async fn uri_paths_accept_urns_and_https_urls_without_changing_their_spelling() {
        let urn = response("/entities/urn%3Angsi-ld%3ABuilding%3A001", "/entities/{id}", get(uri_value)).await;
        let url = response("/entities/https%3A%2F%2Fexample.com%2Fentities%2F1", "/entities/{id}", get(uri_value)).await;

        pretty_assertions::assert_eq!(body_text(urn).await, "urn:urn:ngsi-ld:Building:001");
        pretty_assertions::assert_eq!(body_text(url).await, "url:https://example.com/entities/1");
    }

    #[tokio::test]
    async fn malformed_uri_paths_return_bad_request_data_naming_the_parameter_and_the_request_uri() {
        for path in ["/entities/bare-name", "/entities/%20", "/entities/%ZZ"] {
            let response = response(path, "/entities/{id}", get(uri_value)).await;
            pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let problem = problem(response).await;
            pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
            pretty_assertions::assert_eq!(problem["instance"], path);
            assert!(
                problem["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.starts_with("invalid path parameter entityId: ")),
                "{problem}"
            );
        }
    }

    /// An identifier past the identifier bound is refused at the path, before
    /// any store is asked about it (clause 5.5.4).
    #[tokio::test]
    async fn an_over_long_uri_path_returns_bad_request_data() {
        let path = format!("/entities/urn%3Angsi-ld%3ABuilding%3A{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let response = response(&path, "/entities/{id}", get(uri_value)).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let problem = problem(response).await;
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(problem["detail"].as_str().is_some_and(|detail| detail.contains("bytes")), "{problem}");
    }

    #[tokio::test]
    async fn attribute_paths_accept_every_specified_name_form_and_reject_invalid_names() {
        for (encoded, expected) in [
            ("temperature", "temperature"),
            ("schema%3Atemperature", "schema:temperature"),
            ("urn%3Angsi-ld%3AProperty%3Atemperature", "urn:ngsi-ld:Property:temperature"),
            ("https%3A%2F%2Fexample.com%2Ftemperature", "https://example.com/temperature"),
        ] {
            let path = format!("/attributes/{encoded}");
            let response = response(&path, "/attributes/{attribute}", get(attribute_value)).await;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            pretty_assertions::assert_eq!(body_text(response).await, expected);
        }

        let response = response("/attributes/%20", "/attributes/{attribute}", get(attribute_value)).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let problem = problem(response).await;
        assert!(
            problem["detail"]
                .as_str()
                .is_some_and(|detail| detail.starts_with("invalid path parameter attrId: ")),
            "{problem}"
        );
    }

    #[tokio::test]
    async fn instance_paths_accept_urns_and_https_urls_and_reject_bare_values() {
        for (encoded, expected) in [
            ("urn%3Angsi-ld%3AAttributeInstance%3A1", "urn:ngsi-ld:AttributeInstance:1"),
            ("https%3A%2F%2Fexample.com%2Finstances%2F1", "https://example.com/instances/1"),
        ] {
            let path = format!("/instances/{encoded}");
            let response = response(&path, "/instances/{instance}", get(instance_value)).await;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            pretty_assertions::assert_eq!(body_text(response).await, expected);
        }

        let response = response("/instances/bare", "/instances/{instance}", get(instance_value)).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_route_with_two_segments_names_the_one_at_fault() {
        let ok = response(
            "/entities/urn%3Angsi-ld%3AV%3A1/attrs/speed",
            "/entities/{id}/attrs/{attr_id}",
            get(attribute_of_entity),
        )
        .await;
        pretty_assertions::assert_eq!(body_text(ok).await, "speed");

        let refused = response("/entities/bare/attrs/speed", "/entities/{id}/attrs/{attr_id}", get(attribute_of_entity)).await;
        pretty_assertions::assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let problem = problem(refused).await;
        assert!(
            problem["detail"]
                .as_str()
                .is_some_and(|detail| detail.starts_with("invalid path parameter entityId: ")),
            "{problem}"
        );
    }
}
