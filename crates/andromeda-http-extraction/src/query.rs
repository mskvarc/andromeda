//! Generic validated query parameter extractor for NGSI-LD API handlers.
//!
//! Wraps [`serde_urlencoded`] deserialization with NGSI-LD error classification,
//! mapping parse failures to the correct `InvalidRequest` or `BadRequestData`
//! type (clause 6.3.20).

use crate::{addressed_uri::addressed_uri, query_decode_error::QueryDecodeError};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::de::DeserializeOwned;
use std::future::{Future, ready};

/// Axum extractor that deserializes and validates query parameters.
///
/// Wraps the deserialized value of type `T` after parsing the URI query string
/// with [`serde_urlencoded`]. Parse failures are classified by
/// [`QueryDecodeError`].
pub struct ValidatedQuery<T>(pub T);

/// Deserialize the request's query string into `T`, classifying parse failures.
fn deserialize_query<T>(parts: &Parts) -> Result<ValidatedQuery<T>, ExtractorRejection>
where
    T: DeserializeOwned,
{
    let uri = addressed_uri(parts);
    let query_str = parts.uri.query().unwrap_or("");

    serde_urlencoded::from_str::<T>(query_str)
        .map(ValidatedQuery)
        .map_err(|error| QueryDecodeError(error).into_rejection(uri))
}

impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    // `Send` is what the trait asks of the returned future, which carries the
    // deserialized value; every axum handler needs it of an extractor anyway.
    T: DeserializeOwned + Send,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(deserialize_query::<T>(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use andromeda_ngsi_ld_request::{
        options::EntityRepresentationOptions,
        query::deserialization::deserialize_optional_join_level,
        representation_parameter::WrittenValue,
    };
    use axum::{extract::FromRequestParts, http::Request};
    use serde::Deserialize;
    use std::num::NonZeroU32;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestParams {
        limit: Option<u32>,
    }

    #[tokio::test]
    async fn valid_input_deserializes_correctly() {
        let req = Request::builder().uri("/test?limit=10").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        let query = result.expect("should succeed");
        pretty_assertions::assert_eq!(query.0.limit, Some(10));
    }

    /// The error type a query is refused with, or `None` when it is admitted.
    fn refusal_type<T>(result: Result<ValidatedQuery<T>, ExtractorRejection>) -> Option<NgsiLdErrorType> {
        result.err().and_then(|rejection| rejection.problem().map(ProblemDetail::error_type))
    }

    #[tokio::test]
    async fn unknown_field_produces_invalid_request() {
        let req = Request::builder().uri("/test?unknown=42").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        pretty_assertions::assert_eq!(refusal_type(result), Some(NgsiLdErrorType::InvalidRequest));
    }

    #[tokio::test]
    async fn invalid_value_produces_bad_request_data() {
        let req = Request::builder().uri("/test?limit=abc").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        pretty_assertions::assert_eq!(refusal_type(result), Some(NgsiLdErrorType::BadRequestData));
    }

    /// Table 5.5.2-1: a `joinLevel` that is no integer is a syntactically
    /// invalid request, an integer below 1 data that does not meet the
    /// requirements of the operation.
    #[tokio::test]
    async fn a_non_integer_join_level_is_invalid_request_and_zero_bad_request_data() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct JoinParams {
            #[serde(rename = "joinLevel", default, deserialize_with = "deserialize_optional_join_level")]
            _join_level: Option<NonZeroU32>,
        }

        for (query, invalid_request) in [
            ("joinLevel=abc", true),
            ("joinLevel=1.5", true),
            ("joinLevel=", true),
            ("joinLevel=0", false),
            ("joinLevel=-2", false),
        ] {
            let req = Request::builder().uri(format!("/test?{query}")).body(()).expect("valid request");
            let (mut parts, ()) = req.into_parts();
            let result = ValidatedQuery::<JoinParams>::from_request_parts(&mut parts, &()).await;
            let expected = if invalid_request {
                NgsiLdErrorType::InvalidRequest
            } else {
                NgsiLdErrorType::BadRequestData
            };
            pretty_assertions::assert_eq!(refusal_type(result), Some(expected), "{query}");
        }
    }

    /// Clause 6.3.20 gives an unsupported `format` or `options` value the same
    /// `InvalidRequest` as an unknown parameter, but neither reaches this
    /// classification: both deserialize as written and are refused by the
    /// parameter set's `validate`, which names the error type itself.
    #[tokio::test]
    async fn a_representation_parameter_never_fails_deserialization() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RepresentationParams {
            format: Option<WrittenValue<ResponseFormat>>,
            options: Option<WrittenValue<EntityRepresentationOptions>>,
        }

        let req = Request::builder()
            .uri("/test?format=invalidFormat&options=keyValues,invalid")
            .body(())
            .expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let query = ValidatedQuery::<RepresentationParams>::from_request_parts(&mut parts, &())
            .await
            .expect("an unsupported value still deserializes");
        assert!(query.0.format.expect("present").supported().is_err());
        assert!(query.0.options.expect("present").supported().is_err());
    }
}
