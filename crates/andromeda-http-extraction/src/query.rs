//! Generic validated query parameter extractor for NGSI-LD API handlers.
//!
//! Wraps [`serde_urlencoded`] deserialization with NGSI-LD error classification,
//! mapping parse failures to the correct `InvalidRequest` or `BadRequestData` type.

use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::query::deserialization::GEOREL_WITHOUT_GEOMETRY;
use axum::{
    extract::FromRequestParts,
    http::{Uri, request::Parts},
};
use serde::de::DeserializeOwned;
use std::future::{Future, ready};

/// Axum extractor that deserializes and validates query parameters.
///
/// Wraps the deserialized value of type `T` after parsing the URI query string
/// with [`serde_urlencoded`]. Parse failures are classified into NGSI-LD error
/// types based on the error message content.
pub struct ValidatedQuery<T>(pub T);

/// Deserialize the request's query string into `T`, classifying parse failures.
fn deserialize_query<T>(parts: &Parts) -> Result<ValidatedQuery<T>, ExtractorRejection>
where
    T: DeserializeOwned,
{
    let uri = parts.uri.clone();
    let query_str = parts.uri.query().unwrap_or("");

    serde_urlencoded::from_str::<T>(query_str).map(ValidatedQuery).map_err(|error| {
        // serde_urlencoded only provides string error messages (no structured error kinds).
        // Serde's "unknown field" message is stable across versions, so this heuristic
        // is reliable for distinguishing unsupported parameters (-> InvalidRequest)
        // from invalid parameter values (-> BadRequestData).
        let error_str = error.to_string();
        let error_msg = format!("Invalid query parameters: {error_str}");
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

/// Classify a `serde_urlencoded` error into the appropriate NGSI-LD rejection.
///
/// The `serde_urlencoded` crate only provides unstructured string error messages.
/// Serde's "unknown field" message is stable across versions, making this heuristic
/// reliable for distinguishing unsupported parameters (`InvalidRequest`) from
/// invalid parameter values (`BadRequestData`).
pub fn classify_query_parse_error(error_str: &str, error_msg: String, uri: Uri) -> ExtractorRejection {
    // `format` and `options` are syntax-level representation selectors. Their
    // custom deserializers report invalid tokens as ordinary serde strings,
    // so preserve the clause distinction here instead of treating them like
    // invalid data values such as `limit=abc`. A `georel` with nothing to
    // relate to is a parameter the operation cannot use (clause 6.3.20,
    // `InvalidRequest`); a reference geometry without a relation is a geoquery
    // that is not valid as per clause 4.10, which clauses 5.7.2.4 and 5.10.2.4
    // make a `BadRequestData`.
    if error_str.contains("unknown field")
        || error_str.contains("invalid format")
        || error_str.contains("invalid options")
        || error_str.contains("invalid temporal format")
        || error_str.contains(GEOREL_WITHOUT_GEOMETRY)
    {
        ExtractorRejection::invalid_request(error_msg, uri)
    } else {
        ExtractorRejection::bad_request_data(error_msg, uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_request::query::deserialization::INCOMPLETE_GEOQUERY;
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
    fn invalid_representation_format_is_invalid_request() {
        let rejection = classify_query_parse_error(
            "invalid format value: bananas",
            "invalid format".to_owned(),
            "/test".parse().expect("valid URI"),
        );
        assert!(matches!(rejection, ExtractorRejection::InvalidRequest { .. }));
    }

    #[test]
    fn a_reference_geometry_without_a_relation_is_bad_request_data() {
        // Clauses 5.7.2.4 and 5.10.2.4: a geoquery that is not valid as per
        // clause 4.10 is `BadRequestData`.
        let rejection = classify_query_parse_error(INCOMPLETE_GEOQUERY, "incomplete geo query".to_owned(), "/test".parse().expect("valid URI"));
        assert!(matches!(rejection, ExtractorRejection::BadRequestData { .. }));
    }

    #[test]
    fn a_relation_without_a_reference_geometry_is_invalid_request() {
        // Clause 6.3.20: a `georel` alone is a parameter the operation cannot use.
        let rejection = classify_query_parse_error(GEOREL_WITHOUT_GEOMETRY, "georel alone".to_owned(), "/test".parse().expect("valid URI"));
        assert!(matches!(rejection, ExtractorRejection::InvalidRequest { .. }));
    }

    #[test]
    fn invalid_representation_options_are_invalid_request() {
        let rejection = classify_query_parse_error(
            "invalid options value: bananas",
            "invalid options".to_owned(),
            "/test".parse().expect("valid URI"),
        );
        assert!(matches!(rejection, ExtractorRejection::InvalidRequest { .. }));
    }
}
