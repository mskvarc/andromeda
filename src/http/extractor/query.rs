//! Generic validated query parameter extractor for NGSI-LD API handlers.
//!
//! Wraps [`serde_urlencoded`] deserialization with NGSI-LD error classification,
//! mapping parse failures to the correct `InvalidRequest` or `BadRequestData` type.

use crate::http::extractor::error::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::de::DeserializeOwned;

/// Axum extractor that deserializes and validates query parameters.
///
/// Wraps the deserialized value of type `T` after parsing the URI query string
/// with [`serde_urlencoded`]. Parse failures are classified into NGSI-LD error
/// types based on the error message content.
pub struct ValidatedQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        let query_str = parts.uri.query().unwrap_or("");

        let params = serde_urlencoded::from_str::<T>(query_str).map_err(|error| {
            // serde_urlencoded only provides string error messages (no structured error kinds).
            // Serde's "unknown field" message is stable across versions, so this heuristic
            // is reliable for distinguishing unsupported parameters (-> InvalidRequest)
            // from invalid parameter values (-> BadRequestData).
            let error_str = error.to_string();
            let error_msg = format!("Invalid query parameters: {error_str}");
            classify_query_parse_error(&error_str, error_msg, uri)
        })?;

        Ok(ValidatedQuery(params))
    }
}

/// Classify a `serde_urlencoded` error into the appropriate NGSI-LD rejection.
///
/// The `serde_urlencoded` crate only provides unstructured string error messages.
/// Serde's "unknown field" message is stable across versions, making this heuristic
/// reliable for distinguishing unsupported parameters (`InvalidRequest`) from
/// invalid parameter values (`BadRequestData`).
fn classify_query_parse_error(error_str: &str, error_msg: String, uri: axum::http::Uri) -> ExtractorRejection {
    if error_str.contains("unknown field") {
        ExtractorRejection::invalid_request(error_msg, uri)
    } else {
        ExtractorRejection::bad_request_data(error_msg, uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::FromRequestParts, http::Request};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestParams {
        limit: Option<u32>,
    }

    #[tokio::test]
    async fn valid_input_deserializes_correctly() {
        let req = Request::builder().uri("/test?limit=10").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        let query = result.expect("should succeed");
        pretty_assertions::assert_eq!(query.0.limit, Some(10));
    }

    #[tokio::test]
    async fn unknown_field_produces_invalid_request() {
        let req = Request::builder().uri("/test?unknown=42").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn invalid_value_produces_bad_request_data() {
        let req = Request::builder().uri("/test?limit=abc").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
