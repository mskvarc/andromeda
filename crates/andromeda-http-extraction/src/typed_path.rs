//! Typed route parameters with NGSI-LD problem-detail rejections.

use crate::addressed_uri::addressed_uri;
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::{FromRequestParts, Path, rejection::PathRejection},
    http::request::Parts,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

/// A route parameter parsed once into its canonical domain type.
///
/// Axum performs percent-decoding before Serde deserialization. Any decoding or
/// domain parsing failure is mapped to the clause-5.5.2 `BadRequestData`
/// problem shape and names the complete request URI as its `instance`.
pub struct TypedPath<T>(pub T);

impl<S, T> FromRequestParts<S> for TypedPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<TypedPath<T>, ExtractorRejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| TypedPath(value))
            .map_err(|rejection: PathRejection| InvalidPathParameter(rejection).into_rejection(addressed_uri(parts)))
    }
}

/// A route parameter that axum could not decode or the domain type refused.
#[derive(Debug, Error)]
#[error("invalid URI path parameter: {0}")]
struct InvalidPathParameter(#[source] PathRejection);

impl RequestRefusal for InvalidPathParameter {
    fn error_type(&self) -> NgsiLdErrorType {
        NgsiLdErrorType::BadRequestData
    }
}

#[cfg(test)]
mod tests {
    use crate::typed_path::TypedPath;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, identifier_length::IDENTIFIER_BYTE_LIMIT, uri::UriView};
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
        routing::{MethodRouter, get},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn uri_value(TypedPath(id): TypedPath<Identifier>) -> String {
        match id.view() {
            UriView::Urn(value) => format!("urn:{value}"),
            UriView::Url(value) => format!("url:{value}"),
            UriView::Iri(value) => format!("iri:{value}"),
        }
    }

    async fn attribute_value(TypedPath(attribute): TypedPath<AttributeName>) -> String {
        attribute.to_string()
    }

    async fn instance_value(TypedPath(instance): TypedPath<Identifier>) -> String {
        instance.to_string()
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

    #[tokio::test]
    async fn uri_paths_accept_urns_and_https_urls_without_changing_their_spelling() {
        let urn = response("/entities/urn%3Angsi-ld%3ABuilding%3A001", "/entities/{id}", get(uri_value)).await;
        let url = response("/entities/https%3A%2F%2Fexample.com%2Fentities%2F1", "/entities/{id}", get(uri_value)).await;

        pretty_assertions::assert_eq!(body_text(urn).await, "urn:urn:ngsi-ld:Building:001");
        pretty_assertions::assert_eq!(body_text(url).await, "url:https://example.com/entities/1");
    }

    #[tokio::test]
    async fn malformed_uri_paths_return_bad_request_data_with_the_request_uri() {
        for path in ["/entities/bare-name", "/entities/%20", "/entities/%ZZ"] {
            let response = response(path, "/entities/{id}", get(uri_value)).await;
            pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = body_text(response).await;
            let problem: Value = serde_json::from_str(&body).expect("problem detail JSON");
            pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
            pretty_assertions::assert_eq!(problem["instance"], path);
        }
    }

    /// An identifier past the identifier bound is refused at the path, before
    /// any store is asked about it (clause 5.5.4).
    #[tokio::test]
    async fn an_over_long_uri_path_returns_bad_request_data() {
        let path = format!("/entities/urn%3Angsi-ld%3ABuilding%3A{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let response = response(&path, "/entities/{id}", get(uri_value)).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let problem: Value = serde_json::from_str(&body_text(response).await).expect("problem detail JSON");
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
}
