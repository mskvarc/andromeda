//! Generic validated query parameter extractor for NGSI-LD API handlers.
//!
//! Wraps [`serde_urlencoded`] deserialization with NGSI-LD error classification,
//! mapping parse failures to the correct `InvalidRequest` or `BadRequestData`
//! type (clause 6.3.20).

use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::FromRequestParts,
    http::{Uri, request::Parts},
};
use serde::de::DeserializeOwned;
use std::future::{Future, ready};

/// Axum extractor that deserializes and validates query parameters.
///
/// Wraps the deserialized value of type `T` after parsing the URI query string
/// with [`serde_urlencoded`]. Parse failures are classified by
/// [`classify_query_parse_error`].
pub struct ValidatedQuery<T>(pub T);

/// Deserialize the request's query string into `T`, classifying parse failures.
fn deserialize_query<T>(parts: &Parts) -> Result<ValidatedQuery<T>, ExtractorRejection>
where
    T: DeserializeOwned,
{
    let uri = parts.uri.clone();
    let query_str = parts.uri.query().unwrap_or("");

    serde_urlencoded::from_str::<T>(query_str).map(ValidatedQuery).map_err(|error| {
        let error_str = error.to_string();
        let error_msg = format!("invalid query parameters: {error_str}");
        classify_query_parse_error(&error_str, error_msg, uri)
    })
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

/// Classify a `serde_urlencoded` failure into the NGSI-LD rejection it names.
///
/// Two failures can reach here and clause 6.3.20 separates them. A parameter
/// the operation does not define is one of the "parameters that are
/// incompatible with the operation", answered with `InvalidRequest`; serde
/// reports it through `deny_unknown_fields`, whose message is the only signal
/// the crate offers and is stable across versions. Everything else is a value
/// inside a parameter the operation does define -- a `limit` that is not a
/// number, a half-specified geoquery (clauses 4.10, 5.7.2.4, 5.10.2.4) -- which
/// is input that does not meet the requirements of the operation, answered with
/// `BadRequestData`.
///
/// The two parameters whose *values* clause 6.3.20 also gives to
/// `InvalidRequest`, `format` and `options`, never fail here: they deserialize
/// as written and are refused by the parameter set's `validate`, where the
/// error type is typed rather than read out of a message. See
/// [`andromeda_ngsi_ld_request::representation_parameter`].
pub fn classify_query_parse_error(error_str: &str, error_msg: String, uri: Uri) -> ExtractorRejection {
    if error_str.contains(UNKNOWN_PARAMETER) {
        ExtractorRejection::invalid_request(error_msg, uri)
    } else {
        ExtractorRejection::bad_request_data(error_msg, uri)
    }
}

/// Serde's report of a parameter the deserialized parameter set does not define.
const UNKNOWN_PARAMETER: &str = "unknown field";

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use andromeda_ngsi_ld_request::{
        options::EntityRepresentationOptions,
        query::deserialization::{GEOREL_WITHOUT_GEOMETRY, INCOMPLETE_GEOQUERY},
        representation_parameter::WrittenValue,
    };
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
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        let query = result.expect("should succeed");
        pretty_assertions::assert_eq!(query.0.limit, Some(10));
    }

    #[tokio::test]
    async fn unknown_field_produces_invalid_request() {
        let req = Request::builder().uri("/test?unknown=42").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn invalid_value_produces_bad_request_data() {
        let req = Request::builder().uri("/test?limit=abc").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = ValidatedQuery::<TestParams>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }

    #[test]
    fn a_reference_geometry_without_a_relation_is_bad_request_data() {
        // Clauses 5.7.2.4 and 5.10.2.4: a geoquery that is not valid as per
        // clause 4.10 is `BadRequestData`.
        let rejection = classify_query_parse_error(INCOMPLETE_GEOQUERY, "incomplete geo query".to_owned(), "/test".parse().expect("valid URI"));
        assert!(matches!(rejection, ExtractorRejection::BadRequestData { .. }));
    }

    #[test]
    fn a_relation_without_a_reference_geometry_is_bad_request_data() {
        // Clause 4.10 needs all three of `georel`, `geometry` and `coordinates`
        // for a geoquery, so a lone `georel` is as incomplete as a lone
        // geometry, and clauses 5.7.2.4 and 5.10.2.4 make both `BadRequestData`.
        let rejection = classify_query_parse_error(GEOREL_WITHOUT_GEOMETRY, "georel alone".to_owned(), "/test".parse().expect("valid URI"));
        assert!(matches!(rejection, ExtractorRejection::BadRequestData { .. }));
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
